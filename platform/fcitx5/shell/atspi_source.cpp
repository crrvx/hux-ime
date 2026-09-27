// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

// AT-SPI 取字来源：客户端不上报周边文本时（终端等），从无障碍总线取焦点对象的文本与光标。
//
// 为什么要有后台线程：这里的调用方（fcitx5 按键路径）**必须在微秒级返回** —— 一次 D-Bus
// 往返在慢应用上可以到数十毫秒。因此第一次用到才起线程，线程内 `atspi_init()` + 私有
// `GMainContext` + `GMainLoop`，取字全在那个线程上做，按键路径只读一份加锁的缓存。
//
// **为什么是轮询而不是事件监听**（原型对照实验的硬结论，改回去之前请读这段）：
//   · 工作线程跑**私有** `GMainContext`（哪怕 `atspi_set_main_context` 指过去）⇒ focus /
//     caret 事件**一个都收不到**：实测 5 s 内收 0 个，且退出主循环后事件才被冲出来
//     （排队事件要等下一次**阻塞式 D-Bus 调用**回程才投递 —— 这个假象极容易骗过验收）。
//   · 事件能通的两种形态都要求跑**进程默认** `GMainContext`，而 fcitx5 自己的 GLib 主循环
//     正占着它；插件再去抢会互相阻塞，宿主若不用 GLib 事件循环更是永远收不到（静默失效）。
//   ⇒ 用**同步阻塞式** D-Bus 调用轮询：不依赖任何主循环，结果确定、与宿主事件循环无关。
//   私有 context 只用来挂**自己 attach 的**定时器（见下）。
//
// 失败即退化：总线连不上 / 无焦点对象 / 对象无 `Text` 接口 / caret 不可用 / 超预算 ——
// 一律保留旧快照或退回 `std::nullopt`，调用方退回「无来源、提示为空」，**不报错、不阻塞**。
//
// 隐私：取到的文本只进内存缓存，**绝不写进日志**；诊断日志只记状态。

#include "atspi_source.h"

#include <algorithm>
#include <atomic>
#include <chrono>
#include <condition_variable>
#include <cstdint>
#include <mutex>
#include <optional>
#include <string>
#include <thread>
#include <utility>
#include <vector>

#ifdef HUX_HAVE_ATSPI

#include <atspi/atspi.h>
#include <glib.h>

/// 本层诊断日志（glib；取字在后台线程，不走 fcitx5 的日志类别）。
#define HUX_ATSPI_DEBUG(...) g_debug("hux-atspi: " __VA_ARGS__)
#define HUX_ATSPI_INFO(...) g_message("hux-atspi: " __VA_ARGS__)
#define HUX_ATSPI_WARN(...) g_warning("hux-atspi: " __VA_ARGS__)

#endif // HUX_HAVE_ATSPI

namespace hux {
namespace {

#ifdef HUX_HAVE_ATSPI

/// 取字窗口（字符）：引擎只用到光标左侧 1 个字（字反查的 `BEFORE`），窗口只是**成本上界**
/// —— 别为了一次取字把整篇文档拉过总线。
constexpr int kWindowChars = 64;

/// 「活跃窗」时长（毫秒）：`requestRefresh()` 之后这段时间内按 `kPollIntervalMs` 反复探测。
/// 用户按一次键就要看到两排提示，故这一小段要密；窗过后**完全空闲**（不轮询、不占 CPU）。
constexpr int64_t kActiveWindowMs = 3000;

/// 活跃窗内的探测间隔（毫秒）。再收到请求就把活跃窗**续上**（连续打字时始终在窗内）。
constexpr guint kPollIntervalMs = 200;

/// libatspi 的单次 D-Bus 调用超时（毫秒，两个参数分别是「初始化」与「方法调用」）。
///
/// **必须设**：原型实测不设超时时，一个卡死的应用能让一次 `GetChildren` 挂 16.6 s、整轮
/// 17.4 s —— 那是按键路径不可接受的量级。设成 T 之后每个节点的成本上界约为 4×T。
constexpr int kMethodTimeoutMs = 150;

/// 一轮遍历的**节点数**上限（浏览器会把整篇 DOM 挂进无障碍树，必须设界）。
constexpr int kWalkMaxNodes = 256;

/// 一轮遍历的**深度**上限。
constexpr int kWalkMaxDepth = 6;

/// 一轮遍历的**总时间预算**（毫秒）：节点数上限管不住「每个节点都很慢」，故再加一道墙钟
/// 预算，超了就放弃本轮（保留旧快照）。按节点成本上界 4×T 估，16 个节点约 9.6 s 仍在预算内，
/// 而这里给的是「宁可少取字也不拖住后台线程」的保守值。
constexpr int64_t kWalkBudgetMs = 1200;

/// `atspi_init()` 失败（返回 2 = 连不上总线）后的重试间隔（毫秒）：连不上多是环境问题
/// （无障碍未启用），别几毫秒试一次；总线也可能是稍后才起来的。
constexpr int64_t kInitRetryMs = 5000;

/// 当前单调时钟（毫秒）；仅用于节流与预算，不参与任何对外语义。
int64_t nowMs() {
    return std::chrono::duration_cast<std::chrono::milliseconds>(
               std::chrono::steady_clock::now().time_since_epoch())
        .count();
}

/// 合法 UTF-8 校验（逐码点，拒绝过长编码 / 代理区 / 越界）。
bool validUtf8(const std::string &text) {
    const unsigned char *p = reinterpret_cast<const unsigned char *>(text.data());
    const unsigned char *end = p + text.size();
    while (p < end) {
        size_t len = 0;
        uint32_t code = 0;
        if (*p < 0x80) {
            ++p;
            continue;
        }
        if ((*p & 0xE0) == 0xC0) {
            len = 2;
            code = *p & 0x1Fu;
        } else if ((*p & 0xF0) == 0xE0) {
            len = 3;
            code = *p & 0x0Fu;
        } else if ((*p & 0xF8) == 0xF0) {
            len = 4;
            code = *p & 0x07u;
        } else {
            return false;
        }
        if (p + len > end) {
            return false;
        }
        for (size_t i = 1; i < len; ++i) {
            if ((p[i] & 0xC0) != 0x80) {
                return false;
            }
            code = (code << 6) | (p[i] & 0x3Fu);
        }
        // 过长编码 / 代理区 / 超出 Unicode：按非法处理（宁可退化成「无来源」）。
        if ((len == 2 && code < 0x80) || (len == 3 && code < 0x800) ||
            (len == 4 && code < 0x10000) || code > 0x10FFFF ||
            (code >= 0xD800 && code <= 0xDFFF)) {
            return false;
        }
        p += len;
    }
    return true;
}

/// 数**字符**数（UTF-8 码点数）—— 与 `text.size()`（**字节数**）区分开：
/// 中文一个字 3 字节，混用会把字符制光标放到 3 倍远的位置。
size_t charCount(const std::string &text) {
    size_t count = 0;
    for (char byte : text) {
        if ((static_cast<unsigned char>(byte) & 0xC0) != 0x80) { // 续字节不计
            ++count;
        }
    }
    return count;
}

/// 截到**前 n 个字符**（不是字节）的 UTF-8 前缀；尾部残串与非法首字节一并丢掉。
std::string truncateChars(const std::string &text, size_t chars) {
    size_t done = 0;
    size_t i = 0;
    while (i < text.size() && done < chars) {
        const unsigned char lead = static_cast<unsigned char>(text[i]);
        size_t len = 0;
        if (lead < 0x80) {
            len = 1;
        } else if ((lead & 0xE0) == 0xC0) {
            len = 2;
        } else if ((lead & 0xF0) == 0xE0) {
            len = 3;
        } else if ((lead & 0xF8) == 0xF0) {
            len = 4;
        } else {
            break;
        }
        if (i + len > text.size()) {
            break;
        }
        i += len;
        ++done;
    }
    return text.substr(0, i);
}

/// 从 `object` 取到的「光标左侧窗口」。
///
/// `cursorChars` 是**窗口内**的光标位置，语义上等于 `text` 的**字符数**（窗口以光标结尾）。
/// 它必须和 `text` 一起算出来：只报文本、事后再数一遍的话，很容易把 `std::string::size()`
/// （**字节数**）当成字符数 —— 中文下一个字 3 字节，光标会被放到 3 倍远的位置。
struct Window {
    std::string text;
    uint32_t cursorChars;
};

/// 从对象取「光标左侧窗口」；无 `Text` 接口 / caret 不可用 / 文本为空时返回 `nullopt`。
///
/// 引用归属（本机 libatspi 2.60.7 逆向确认）：`atspi_accessible_get_text()` 走的是
/// `g_type_interface_peek()` —— `AtspiText` 是 **GTypeInterface**（`{GType g_type;
/// GType g_instance_type;}`，见 `gobject/gtype.h:471`），**不是 GObject**：它既没有引用计数，
/// 也不是独立分配的对象，而是接口实现自带的一份类结构。⇒ 其返回值是**借用的**，
/// **绝不能** `g_object_unref()`（那会把接口结构地址当 `GObject*`，读到 `GType` 数值当
/// 魔数/引用计数，直接 SIGSEGV）。此处刻意不 unref：它的生命周期跟 `object` 走，而
/// `object` 的引用由调用方持有到 `extractWindow` 返回之后。
std::optional<Window> extractWindow(AtspiAccessible *object) {
    AtspiText *text = atspi_accessible_get_text(object);
    if (text == nullptr) {
        return std::nullopt;
    }
    GError *error = nullptr;
    // 这两个读的是 D-Bus **属性**（libatspi 已封装好 `org.a11y.atspi.Text` 的
    // `CharacterCount` / `CaretOffset`），失败时返回 -1 / 0 并给 `error`。
    const gint chars = atspi_text_get_character_count(text, &error);
    if (error != nullptr) {
        g_error_free(error);
        error = nullptr;
    }
    const gint caret = atspi_text_get_caret_offset(text, &error);
    if (error != nullptr) {
        g_error_free(error);
        error = nullptr;
    }
    if (chars <= 0 || caret < 0) {
        return std::nullopt;
    }
    // 只取光标左侧的窗口（成本上界）：AT-SPI 的 offset/caret 都是**字符**偏移，
    // 而 `atspi_text_get_text` 交回来的是 UTF-8 字节串 —— 直接按字符区间取即可。
    //
    // 窗口是 [begin, caret) 这段字符，故**窗口内的光标就在窗口末尾**，即字符偏移
    // `caret - begin`（== 返回串的字符数）。注意不能把**绝对** caret 当窗口内偏移上报：
    // 窗口被 kWindowChars 截断时绝对 caret 会比窗口还长，引擎按它回退 BEFORE 个字就会取错字。
    //
    // `end == -1` 在 AT-SPI 里表示「到末尾」，实现方对越界区间一般会自己夹取；
    // 这里给的是精确区间，不依赖那个宽容行为。
    const gint begin = std::max(0, caret - kWindowChars);
    gchar *raw = atspi_text_get_text(text, begin, caret, &error);
    if (error != nullptr) {
        g_error_free(error);
        error = nullptr;
    }
    std::string window;
    if (raw != nullptr) {
        window.assign(raw);
        g_free(raw);
    }
    // 消毒 1：窗口是字符区间，返回的字节串里应当就是这些字符；个别实现把区间按**字节**
    // 解释（交回更多字节）时，按**字符数**截掉多出来的部分。
    if (window.size() > static_cast<size_t>(caret - begin)) {
        window = truncateChars(window, static_cast<size_t>(caret - begin));
    }
    // 消毒 2：合法 UTF-8 + 非空（半个字符 / 空窗一律当成「无来源」）。
    if (window.empty() || !validUtf8(window)) {
        return std::nullopt;
    }
    // 消毒 3：窗口内光标 = 窗口的**字符数**（不是 `window.size()` 的字节数）。按字符数收敛，
    // 保证上报的光标一定落在窗口内、且指向窗口末尾那个字。
    Window result;
    result.cursorChars =
        static_cast<uint32_t>(std::min<size_t>(static_cast<size_t>(caret - begin), charCount(window)));
    result.text = std::move(window);
    return result;
}

#endif // HUX_HAVE_ATSPI

} // namespace

#ifdef HUX_HAVE_ATSPI

/// 后台线程与缓存（类的私有不透明实现；接口在头文件）。
class AtspiSource::Impl {
public:
    /// 后台线程状态。
    enum class State : int {
        Idle = 0,   ///< 未启动：等第一次请求（惰性启动）
        Running,    ///< `atspi_init()` 成功
        InitFailed, ///< `atspi_init()` 失败：退避后重试
        Stopping,   ///< 析构中：已请求主循环退出
        Finished,   ///< 线程已退出
    };

    Impl() = default;
    ~Impl();
    Impl(const Impl &) = delete;
    Impl &operator=(const Impl &) = delete;

    std::optional<AtspiSource::Snapshot> snapshot() const;
    void requestRefresh();
    void invalidate();

private:
    /// 后台线程主体：`atspi_init()` → 私有主上下文 → attach 定时器 → 主循环。
    void workerMain();
    /// 定时器回调（主循环线程）：活跃窗内探测，窗外只做一次标志检查。
    static gboolean onTick(gpointer data);
    /// 探测一次：焦点对象 → `Text` 接口 → caret → 光标左侧窗口 → 缓存。
    void query();
    /// 焦点对象（**新引用**或 `nullptr`）。先按上次记下的**子索引链**快速下降验一次
    /// （焦点没挪窝时省掉整棵树），失败再有界遍历。
    AtspiAccessible *focusedObject();
    /// 沿 `chain_` 从 desktop 逐级下降；任一步失配就返回 `nullptr`（调用方回落全遍历）。
    AtspiAccessible *descendByChain();
    /// 有界深度优先：深度 / 节点数 / 总时间三重上限；命中时把**子索引链**记进 `foundChain_`。
    AtspiAccessible *walkForFocused(AtspiAccessible *node, int depth, int *budget,
                                    int64_t deadline);
    /// 写入缓存（`std::nullopt` = 本次没有可用文本，保留旧快照）。
    void store(std::optional<Window> window);

    // ---- 缓存：`snapshot()` 只读这里；`mutex_` 是按键路径与后台线程之间唯一的同步点。
    mutable std::mutex mutex_;
    std::optional<AtspiSource::Snapshot> snapshot_;
    bool bound_ = false;      ///< 是否已经成功绑定过一个焦点对象（决定首次查询的语义）
    int64_t lastQueryMs_ = 0; ///< 上次查询时刻（节流用）
    int64_t lastInitMs_ = 0;  ///< 上次 `atspi_init()` 尝试时刻（失败退避用）

    // ---- 活跃窗：`requestRefresh()` 把截止时刻往后推；主循环线程只读它。
    std::atomic<int64_t> activeUntilMs_{0};

    // ---- 焦点对象定位：不跨轮持有 `AtspiAccessible*`（应用会退出、控件会销毁），只记
    //      「从 desktop 到它的**子索引链**」—— 纯公有 API 就能重建（`get_child_at_index`）。
    //      用路径字符串的话，libatspi 里**没有**公有函数能从路径拿回对象
    //      （`atspi_accessible_get_path` 不存在；`_atspi_accessible_new` 是私有下划线 API）。
    std::vector<int> lastChain_; ///< 上次命中的子索引链（受 `mutex_` 保护）
    std::vector<int> chain_;     ///< 下降时的当前链（仅主循环线程）
    std::vector<int> foundChain_; ///< 本轮遍历命中的链（仅主循环线程）

    // ---- 线程与主循环：`context_` 受 `invokeMutex_` 保护，供按键线程唤醒主循环。
    std::thread thread_;
    std::mutex doneMutex_;
    std::condition_variable doneCv_;
    std::atomic<State> state_{State::Idle};
    std::atomic<bool> quit_{false};
    bool started_ = false; ///< worker 线程已起（起过后一直 joinable，析构时 join）
    /// 定时器 source（**主循环线程**创建与销毁；析构路径只 quit 循环，不碰它）。
    GSource *timer_ = nullptr;
    GMainLoop *loop_ = nullptr;
    GMainContext *context_ = nullptr;
    std::mutex invokeMutex_;
};

/// 析构契约：**先请求主循环退出，再有界 join**。join 成功即证明 worker 线程已不再碰 libatspi。
AtspiSource::Impl::~Impl() {
    if (!started_) {
        return;
    }
    quit_ = true;
    state_ = State::Stopping;
    {
        // `g_main_loop_quit` 线程安全（内部 attach 到该 loop 的 context 并唤醒）。
        // 与 `context_` 的写入配对：可能在 `loop_` 立起来之前就析构（那时线程自己会看到 quit_）。
        std::lock_guard<std::mutex> lock(invokeMutex_);
        if (loop_ != nullptr) {
            g_main_loop_quit(loop_);
        }
    }
    // 有界等待：真的等不到（libatspi 内部卡死，例如某个应用对整个无障碍总线不响应）也不
    // 无限期拖住 fcitx5 卸载。
    {
        std::unique_lock<std::mutex> lock(doneMutex_);
        if (!doneCv_.wait_for(lock, std::chrono::seconds(2),
                              [this] { return state_.load() == State::Finished; })) {
            HUX_ATSPI_WARN("后台线程未在 2s 内退出（忽略；仅可能在 libatspi 内部卡死时发生）");
        }
    }
    if (thread_.joinable()) {
        thread_.join();
    }
}

std::optional<AtspiSource::Snapshot> AtspiSource::Impl::snapshot() const {
    std::lock_guard<std::mutex> lock(mutex_);
    return snapshot_;
}

void AtspiSource::Impl::requestRefresh() {
    // 请求即「续上活跃窗」：连续打字时窗口一直往后推，探测保持 200ms 一次；停手 3s 后
    // 定时器回到「只检查一个原子量」的空闲态。
    activeUntilMs_.store(nowMs() + kActiveWindowMs, std::memory_order_relaxed);
    if (state_.load() == State::Idle && !started_) {
        // 惰性启动：第一次请求才起线程（插件随 fcitx5 开机加载，不在构造期连总线）。
        started_ = true;
        thread_ = std::thread([this] { workerMain(); });
    }
}

void AtspiSource::Impl::invalidate() {
    std::lock_guard<std::mutex> lock(mutex_);
    snapshot_.reset();
    bound_ = false;
    lastQueryMs_ = 0; // 下次请求立刻可查
    lastChain_.clear(); // 链属于上一个应用/控件，别拿它去验
}

void AtspiSource::Impl::workerMain() {
    // 私有主上下文：只用来挂**自己 attach 的定时器**。libatspi 的事件源即便注册到这里也
    // 收不到事件（见文件头注释），所以本实现**不注册任何事件监听** —— 所有取字都走
    // `query()` 里的同步阻塞式 D-Bus 调用，不依赖主循环。
    GMainContext *context = g_main_context_new();
    g_main_context_push_thread_default(context);
    {
        std::lock_guard<std::mutex> lock(invokeMutex_);
        context_ = context;
    }

    if (atspi_init() == 2) {
        lastInitMs_ = nowMs();
        HUX_ATSPI_INFO("未连上无障碍总线（无障碍未启用？）：字反查退回「客户端上报」来源");
        state_ = State::InitFailed;
    } else {
        // 单次 D-Bus 调用的超时：不设的话一个卡死应用能把一轮探测拖到十几秒。
        atspi_set_timeout(kMethodTimeoutMs, kMethodTimeoutMs);
        state_ = State::Running;
        HUX_ATSPI_INFO("已连上无障碍总线（同步轮询取字；不注册事件监听）");
    }

    loop_ = g_main_loop_new(context, FALSE);
    // **必须**用 `g_timeout_source_new()` + `g_source_attach(src, ctx)`：`g_timeout_add`
    // 挂的是**全局默认** context，在私有 context 里永不触发（原型实测）。
    timer_ = g_timeout_source_new(kPollIntervalMs);
    g_source_set_callback(timer_, &AtspiSource::Impl::onTick, this, nullptr);
    g_source_set_name(timer_, "hux-atspi-poll");
    g_source_attach(timer_, context);

    g_main_loop_run(loop_);

    // ---- 收尾（仅 worker 线程）
    if (timer_ != nullptr) {
        g_source_destroy(timer_);
        g_source_unref(timer_);
        timer_ = nullptr;
    }
    g_main_loop_unref(loop_);
    loop_ = nullptr;
    {
        std::lock_guard<std::mutex> lock(invokeMutex_);
        context_ = nullptr;
    }
    g_main_context_pop_thread_default(context);
    g_main_context_unref(context);
    // 注：不调 `atspi_exit()`。连接与 watch 随进程退出回收；在有界等待超时（线程仍可能在
    // libatspi 内部）时掀掉 libatspi 的全局状态比留着更危险。
    {
        std::lock_guard<std::mutex> lock(doneMutex_);
        state_ = State::Finished;
    }
    doneCv_.notify_all();
}

gboolean AtspiSource::Impl::onTick(gpointer data) {
    auto *self = static_cast<AtspiSource::Impl *>(data);
    if (self->quit_.load()) {
        return G_SOURCE_REMOVE;
    }
    if (self->state_.load() == State::InitFailed) {
        // 退避：数秒后再试一次 `atspi_init()`；期间不做任何 D-Bus 调用。
        if (nowMs() - self->lastInitMs_ >= kInitRetryMs) {
            self->lastInitMs_ = nowMs();
            if (atspi_init() != 2) {
                atspi_set_timeout(kMethodTimeoutMs, kMethodTimeoutMs);
                self->state_ = State::Running;
                HUX_ATSPI_INFO("无障碍总线已可用：AT-SPI 取字来源启用");
            }
        }
        return G_SOURCE_CONTINUE;
    }
    if (self->state_.load() != State::Running) {
        return G_SOURCE_CONTINUE;
    }
    // 空闲判定：窗外**只**读一个原子量，不做任何 D-Bus 调用（不占 CPU、不吵总线）。
    if (nowMs() >= self->activeUntilMs_.load(std::memory_order_relaxed)) {
        return G_SOURCE_CONTINUE;
    }
    self->query();
    return G_SOURCE_CONTINUE;
}

AtspiAccessible *AtspiSource::Impl::focusedObject() {
    foundChain_.clear();
    AtspiAccessible *desktop = atspi_get_desktop(0);
    if (desktop == nullptr) {
        return nullptr;
    }
    // 1) 快速路径：按上次记下的子索引链从 desktop 逐级下降（焦点没挪窝时这是常态，
    //    代价只有「深度」次调用，不用走整棵树）。
    AtspiAccessible *cached = descendByChain();
    if (cached != nullptr) {
        return cached;
    }
    // 2) 回落：有界遍历一次（换控件 / 换应用 / 应用退出 / 链失配）。
    const int64_t deadline = nowMs() + kWalkBudgetMs;
    int budget = kWalkMaxNodes;
    chain_.clear();
    AtspiAccessible *found = walkForFocused(desktop, 0, &budget, deadline);
    if (found != nullptr && !foundChain_.empty()) {
        std::lock_guard<std::mutex> lock(mutex_);
        lastChain_ = foundChain_;
    }
    return found;
}

AtspiAccessible *AtspiSource::Impl::descendByChain() {
    std::vector<int> chain;
    {
        std::lock_guard<std::mutex> lock(mutex_);
        chain = lastChain_;
    }
    if (chain.empty()) {
        return nullptr;
    }
    AtspiAccessible *node = atspi_get_desktop(0);
    if (node == nullptr) {
        return nullptr;
    }
    g_object_ref(node); // 统一由下面这条路径释放
    for (int index : chain) {
        GError *error = nullptr;
        AtspiAccessible *child = atspi_accessible_get_child_at_index(node, index, &error);
        if (error != nullptr) {
            g_error_free(error);
            error = nullptr;
        }
        g_object_unref(node);
        if (child == nullptr) {
            return nullptr; // 树变了：让调用方回落全遍历
        }
        node = child;
    }
    // 终点仍须是**焦点**对象：链命中了但焦点已经移走时，这里会否掉它。
    AtspiStateSet *states = atspi_accessible_get_state_set(node);
    const bool focused = states != nullptr && atspi_state_set_contains(states, ATSPI_STATE_FOCUSED);
    if (states != nullptr) {
        g_object_unref(states);
    }
    if (!focused) {
        g_object_unref(node);
        return nullptr;
    }
    return node;
}

/// 有界深度优先：深度 / 节点数 / 总时间三重上限。全局缓存（`org.a11y.atspi.Cache.GetItems`）
/// 在本机实测不可靠（返回空表），故只能自己走树 —— 但必须设界，浏览器会把整篇 DOM 挂进
/// 无障碍树，而单个不响应的应用每次调用要花到 `4 × atspi_set_timeout` 的量级。
AtspiAccessible *AtspiSource::Impl::walkForFocused(AtspiAccessible *node, int depth, int *budget,
                                                   int64_t deadline) {
    if (node == nullptr || depth > kWalkMaxDepth || *budget <= 0 || nowMs() >= deadline) {
        return nullptr;
    }
    if (depth > 0) {
        --*budget; // 根（desktop）不计入预算
    }
    AtspiStateSet *states = atspi_accessible_get_state_set(node);
    const bool focused =
        states != nullptr && atspi_state_set_contains(states, ATSPI_STATE_FOCUSED);
    if (states != nullptr) {
        g_object_unref(states);
    }
    if (focused) {
        foundChain_ = chain_;
        return ATSPI_ACCESSIBLE(g_object_ref(node));
    }
    GError *error = nullptr;
    const gint children = atspi_accessible_get_child_count(node, &error);
    if (error != nullptr) {
        g_error_free(error);
        error = nullptr;
    }
    for (gint i = 0; i < children && *budget > 0; ++i) {
        if (nowMs() >= deadline) {
            return nullptr; // 超预算：放弃本轮（保留旧快照），别把后台线程拖住
        }
        chain_.push_back(i);
        AtspiAccessible *child = atspi_accessible_get_child_at_index(node, i, &error);
        if (error != nullptr) {
            g_error_free(error);
            error = nullptr;
        }
        if (child == nullptr) {
            chain_.pop_back();
            continue;
        }
        AtspiAccessible *found = walkForFocused(child, depth + 1, budget, deadline);
        g_object_unref(child);
        if (found != nullptr) {
            return found; // `chain_` 此刻正是通往该对象的那条链（由 `foundChain_` 取走）
        }
        chain_.pop_back();
    }
    return nullptr;
}

void AtspiSource::Impl::store(std::optional<Window> window) {
    std::lock_guard<std::mutex> lock(mutex_);
    if (window.has_value()) {
        AtspiSource::Snapshot value;
        value.text = std::move(window->text);
        // 光标**由 `extractWindow` 一起算出来**（窗口内的字符位置），不要在这里重新数：
        // 这里能拿到的只有 `std::string`，`size()` 是**字节数**，中文下会把光标放远 3 倍。
        value.cursorChars = window->cursorChars;
        snapshot_ = std::move(value);
        bound_ = true;
    }
    // `nullopt`：**保留**上一次的快照（一次探测失败不等于「应用里没字了」；真换了应用
    // 由 `invalidate()` 清）。首次就失败时 `snapshot_` 本来就是空，语义自然成立。
}

void AtspiSource::Impl::query() {
    {
        std::lock_guard<std::mutex> lock(mutex_);
        lastQueryMs_ = nowMs();
    }
    AtspiAccessible *target = focusedObject();
    if (target == nullptr) {
        return; // 没找到焦点对象：保留旧快照
    }
    std::optional<Window> window = extractWindow(target);
    g_object_unref(target);
    store(std::move(window));
}

#else // !HUX_HAVE_ATSPI

/// 无 AT-SPI 的构建（`HUX_ATSPI=OFF`，或 `AUTO` 未找到开发包）：整类退化为空实现。
/// **不**连总线、不起线程、`snapshot()` 恒为 `nullopt`。
class AtspiSource::Impl {
public:
    std::optional<AtspiSource::Snapshot> snapshot() const { return std::nullopt; }
    void requestRefresh() {}
    void invalidate() {}
    // 空实现无成员可访问；形参无名即「未使用」（避免 -Wunused-parameter）
};

#endif // HUX_HAVE_ATSPI

AtspiSource::AtspiSource() : impl_(std::make_unique<Impl>()) {}
AtspiSource::~AtspiSource() = default;

std::optional<AtspiSource::Snapshot> AtspiSource::snapshot() const {
    return impl_->snapshot();
}

void AtspiSource::requestRefresh() { impl_->requestRefresh(); }

void AtspiSource::invalidate() { impl_->invalidate(); }

bool AtspiSource::compiled() {
#ifdef HUX_HAVE_ATSPI
    return true;
#else
    return false;
#endif
}

} // namespace hux
