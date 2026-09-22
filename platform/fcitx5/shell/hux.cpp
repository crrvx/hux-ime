// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

// hux-ime（虎虚）fcitx5 addon 的 C++ 薄壳：只做 fcitx5 接口适配，逻辑在 Rust（libhux_platform_fcitx5）。
#include <fcitx-config/configuration.h>
#include <fcitx-config/enum.h>
#include <fcitx-config/option.h>
#include <fcitx-config/iniparser.h>
#include <fcitx-utils/i18n.h>
#include <fcitx/action.h>
#include <fcitx/addonfactory.h>
#include <fcitx/addoninstance.h>
#include <fcitx/candidatelist.h>
#include <fcitx/event.h>
#include <fcitx/inputcontext.h>
#include <fcitx/inputmethodengine.h>
#include <fcitx/inputmethodentry.h>
#include <fcitx/instance.h>
#include <fcitx/menu.h>
#include <fcitx/statusarea.h>
#include <fcitx/surroundingtext.h>
#include <fcitx/inputpanel.h>
#include <fcitx/text.h>
#include <fcitx/userinterface.h>
#include <fcitx/userinterfacemanager.h>
#include <fcitx-utils/key.h>
#include <fcitx-utils/log.h>
#include <fcitx-utils/trackableobject.h>

#include <algorithm>
#include <iterator>
#include <memory>
#include <string>
#include <vector>

#include "hux_abi.h"

namespace {

/// 本层专属日志类别 `hux`。
///
/// `fcitx::Log::setLogRule` 按**类别名**匹配规则，而 `FCITX_DEBUG()` 走的是名为 `default`
/// 的类别（`log.h`：`FCITX_LOG(LEVEL)` → `FCITX_LOGC(::fcitx::Log::defaultCategory, LEVEL)`；
/// `log.cpp` 里默认类别名就是 `"default"`）。因此这里显式定义一个名为 `hux` 的类别，
/// 让 `fcitx5 --verbose='hux=5'` 只打开本层 DEBUG。
FCITX_DEFINE_LOG_CATEGORY(huxLog, "hux");

/// 本层调试日志（默认级别 Info ⇒ 平时静默；`hux=5` 时输出）。
#define HUX_DEBUG() FCITX_LOGC(huxLog, Debug)

/// 注意：`CommonCandidateList::setCursorIndex` 是**页内索引**（越界抛异常），
/// 绝对索引必须用 `setGlobalCursorIndex` + `setPage`；页大小来自配置 `PageSize`。
/// 页大小范围与 core `host::MAX_PAGE_SIZE` 一致（数字直选 0=第 10 个）。
constexpr int kPageSizeMin = 1;
constexpr int kPageSizeMax = 10;

/// 数字直选键序（1–9、0=第 10 个）：候选面板序号显示用。
const fcitx::KeyList &digitSelectionKeys() {
    static const fcitx::KeyList keys = {
        fcitx::Key(FcitxKey_1), fcitx::Key(FcitxKey_2), fcitx::Key(FcitxKey_3),
        fcitx::Key(FcitxKey_4), fcitx::Key(FcitxKey_5), fcitx::Key(FcitxKey_6),
        fcitx::Key(FcitxKey_7), fcitx::Key(FcitxKey_8), fcitx::Key(FcitxKey_9),
        fcitx::Key(FcitxKey_0)};
    return keys;
}

/// fcitx5 `KeyList` → C ABI 键位列表（超出上限的项忽略）。
void fillKeyList(hux_key_list *dest, const fcitx::KeyList &keys) {
    dest->count = 0;
    for (const fcitx::Key &key : keys) {
        if (dest->count >= HUX_MAX_KEYS) {
            break;
        }
        dest->sym[dest->count] = static_cast<int32_t>(key.sym());
        dest->states[dest->count] =
            static_cast<int32_t>(key.states().toInteger());
        ++dest->count;
    }
}

/// 候选排列（参照 `style` 语义；默认跟随 fcitx5 全局「候选竖排」设置）。
enum class HuxCandidateLayout { FollowGlobal, Horizontal, Vertical };
FCITX_CONFIG_ENUM_NAME(HuxCandidateLayout, "跟随全局", "横排", "竖排");
FCITX_CONFIG_ENUM_I18N_ANNOTATION(HuxCandidateLayout, "跟随全局", "横排", "竖排");

/// 预编辑内容。
enum class HuxPreeditMode { CandidateCode, RawInput, Hidden };
FCITX_CONFIG_ENUM_NAME(HuxPreeditMode, "候选分码", "原始输入", "不显示");
FCITX_CONFIG_ENUM_I18N_ANNOTATION(HuxPreeditMode, "候选分码", "原始输入", "不显示");

/// 枚举注解 + 悬浮说明（`EnumI18n` 与 `Tooltip` 并存）。
template <typename EnumAnnotation>
struct EnumAnnotationWithTooltip : EnumAnnotation {
    explicit EnumAnnotationWithTooltip(std::string tooltip)
        : tooltip_(std::move(tooltip)) {}

    bool skipDescription() const { return false; }
    bool skipSave() const { return false; }
    void dumpDescription(fcitx::RawConfig &config) const {
        EnumAnnotation::dumpDescription(config);
        config.setValueByPath("Tooltip", tooltip_);
    }

private:
    std::string tooltip_;
};

/// 行为设置（配置页「行为」分区）。
FCITX_CONFIGURATION(
    HuxBehaviorConfig,
    fcitx::OptionWithAnnotation<bool, fcitx::ToolTipAnnotation> earlyCommit{{
        .parent = this,
        .path{"EarlyCommit"},
        .description{"提前上屏"},
        .defaultValue = true,
        .annotation{"组合中证据成熟即提交当前候选；关闭后仅在空格/回车确认时上屏。"}}};
    fcitx::OptionWithAnnotation<bool, fcitx::ToolTipAnnotation>
        earlyCommitToPreedit{{
            .parent = this,
            .path{"EarlyCommitToPreedit"},
            .description{"提前上屏至预编辑"},
            .defaultValue = false,
            .annotation{"提前上屏改为写入预编辑（缓冲，不直接提交），继续输入可修正。"}}};
    fcitx::OptionWithAnnotation<bool, fcitx::ToolTipAnnotation>
        allowDuplicateSingle{{
            .parent = this,
            .path{"AllowDuplicateSingle"},
            .description{"单字重码参与组句"},
            .defaultValue = true,
            .annotation{"允许同一单字的重码候选参与整句解码；关闭可减少同字重复。"}}};
    fcitx::OptionWithAnnotation<bool, fcitx::ToolTipAnnotation> fullShape{{
        .parent = this,
        .path{"FullShape"},
        .description{"全角标点"},
        .defaultValue = false,
        .annotation{"标点输出全角形式（如 `,` → `，`）。"}}};
    fcitx::OptionWithAnnotation<bool, fcitx::ToolTipAnnotation> asciiPunct{{
        .parent = this,
        .path{"AsciiPunct"},
        .description{"ASCII 标点直通"},
        .defaultValue = false,
        .annotation{"标点不做中文映射，直接输出 ASCII。"}}};
    fcitx::OptionWithAnnotation<bool, fcitx::ToolTipAnnotation> learningOnTab{{
        .parent = this,
        .path{"TabLearning"},
        .description{"Tab 选字写入学习库"},
        .defaultValue = true,
        .annotation{"用 Tab 选字时记录学习事件，参与后续候选排序。"}}};
    fcitx::OptionWithAnnotation<bool, fcitx::ToolTipAnnotation> digitSelect{{
        .parent = this,
        .path{"DigitSelect"},
        .description{"数字直选"},
        .defaultValue = true,
        .annotation{"开启后菜单可见时 `1`–`9` 直接上屏当前页候选、`0` = 第 10 个；"
                    "关闭时数字仍作编码选重后缀。"}}};
    fcitx::OptionWithAnnotation<bool, fcitx::ToolTipAnnotation> pageCycle{{
        .parent = this,
        .path{"PageCycle"},
        .description{"翻页循环"},
        .defaultValue = false,
        .annotation{"候选翻页在末页再翻回首页、首页向上翻到末页。"}}};
    fcitx::OptionWithAnnotation<bool, fcitx::ToolTipAnnotation> panelPreedit{{
        .parent = this,
        .path{"PanelPreedit"},
        .description{"候选窗口显示预编辑"},
        .defaultValue = false,
        .annotation{"仅宿主显示项，不经引擎：候选窗口是否显示预编辑文本。"}}};
    // 值选项（`int` 等）列在布尔选项之后。
    fcitx::Option<int, fcitx::IntConstrain, fcitx::DefaultMarshaller<int>,
                  fcitx::ToolTipAnnotation>
        highFreqLimit{{
            .parent = this,
            .path{"HighFreqLimit"},
            .description{"高频字过滤上限"},
            .defaultValue = 1500,
            .constrain = fcitx::IntConstrain(0, 20000),
            .annotation{"仅使用最优码组句的高频字数量上限；0 = 不限制。修改后需重启生效。"}}};
    fcitx::Option<int, fcitx::IntConstrain, fcitx::DefaultMarshaller<int>,
                  fcitx::ToolTipAnnotation>
        pageSize{{
            .parent = this,
            .path{"PageSize"},
            .description{"每页候选个数"},
            .defaultValue = 5,
            .constrain = fcitx::IntConstrain(kPageSizeMin, kPageSizeMax),
            .annotation{"候选列表每页个数（1–10）；数字直选 `0` 对应第 10 个。"}}};
    fcitx::OptionWithAnnotation<
        HuxCandidateLayout,
        EnumAnnotationWithTooltip<HuxCandidateLayoutI18NAnnotation>>
        candidateLayout{{
            .parent = this,
            .path{"CandidateLayout"},
            .description{"候选排列"},
            .defaultValue = HuxCandidateLayout::FollowGlobal,
            .annotation{"跟随全局：候选窗排列随 fcitx5 全局「候选竖排」；"
                        "横排：↑↓ 选字、←→ 移动；竖排：←→ 选字、↑↓ 移动。"}}};
    fcitx::OptionWithAnnotation<HuxPreeditMode,
                                EnumAnnotationWithTooltip<HuxPreeditModeI18NAnnotation>>
        preeditMode{{
            .parent = this,
            .path{"PreeditMode"},
            .description{"预编辑内容"},
            .defaultValue = HuxPreeditMode::CandidateCode,
            .annotation{"候选分码：按词分段显示（如 `ab cd`）；原始输入：按输入原文；"
                        "不显示：仅候选与注释。"}}};
    fcitx::Option<int, fcitx::IntConstrain, fcitx::DefaultMarshaller<int>,
                  fcitx::ToolTipAnnotation>
        minRetainedInputLength{{
            .parent = this,
            .path{"MinRetainedRawLength"},
            .description{"提前上屏最短保留码数"},
            .defaultValue = 0,
            .constrain = fcitx::IntConstrain(0, 20),
            .annotation{"提前上屏与空码上屏共用的最短保留编码数；0 = 不额外限制"
                        "（概率型早提交仍不少于 3）。"}}};);

/// 快捷键设置（配置页「快捷键」分区；`KeyList` 可多项，与全局设置同款）。
FCITX_CONFIGURATION(
    HuxHotkeyConfig,
    fcitx::KeyListOptionWithAnnotation<fcitx::ToolTipAnnotation>
        reverseLookupPronunciationKeys{{
            .parent = this,
            .path{"SoundToCharShapeKey"},
            .description{"音反查"},
            .defaultValue = fcitx::KeyList{
                fcitx::Key(FcitxKey_colon, fcitx::KeyState::Alt)},
            .constrain = fcitx::KeyListConstrain(
                fcitx::KeyConstrainFlag::AllowModifierLess),
            .annotation{"可多项。按下后输入拼音（支持拼写缩写），候选为对应词语、"
                        "注释显示虎码。"}}};
    fcitx::KeyListOptionWithAnnotation<fcitx::ToolTipAnnotation>
        reverseLookupCharacterKeys{{
            .parent = this,
            .path{"CharToSoundShapeKey"},
            .description{"字反查"},
            .defaultValue = fcitx::KeyList{
                fcitx::Key(FcitxKey_quotedbl, fcitx::KeyState::Alt)},
            .constrain = fcitx::KeyListConstrain(
                fcitx::KeyConstrainFlag::AllowModifierLess),
            .annotation{"可多项。按下后显示光标左侧汉字的拼音与虎码"
                        "（需应用支持周边文本）。"}}};
    fcitx::KeyListOptionWithAnnotation<fcitx::ToolTipAnnotation> pageUpKeys{{
        .parent = this,
        .path{"PageUpKey"},
        .description{"上翻页"},
        .defaultValue = fcitx::KeyList{
            fcitx::Key(FcitxKey_minus, fcitx::KeyState::NoState),
            fcitx::Key(FcitxKey_bracketleft, fcitx::KeyState::NoState)},
        .constrain =
            fcitx::KeyListConstrain(fcitx::KeyConstrainFlag::AllowModifierLess),
        .annotation{"可多项。有候选时生效（首屏不动）；Page_Up 键始终可用。"}}};
    fcitx::KeyListOptionWithAnnotation<fcitx::ToolTipAnnotation> pageDownKeys{{
        .parent = this,
        .path{"PageDownKey"},
        .description{"下翻页"},
        .defaultValue = fcitx::KeyList{
            fcitx::Key(FcitxKey_equal, fcitx::KeyState::NoState),
            fcitx::Key(FcitxKey_bracketright, fcitx::KeyState::NoState)},
        .constrain =
            fcitx::KeyListConstrain(fcitx::KeyConstrainFlag::AllowModifierLess),
        .annotation{"可多项。有候选时生效；Page_Down 键始终可用。"}}};);

/// 配置 schema：fcitx5-configtool 依据它自动生成设置页（fcitx://config/addon/hux）；
/// 分区结构参照全局设置（`Option<SubConfig>` → 分组标题，选项带悬浮说明）。
FCITX_CONFIGURATION(
    HuxConfig,
    fcitx::Option<HuxBehaviorConfig> behavior{this, "Behavior", "行为"};
    fcitx::Option<HuxHotkeyConfig> hotkeys{this, "Hotkey", "快捷键"};);

/// 「虎虚」状态菜单开关：勾选态取自引擎运行时选项（`options.yaml`），激活即翻转并落盘。
class HuxToggleAction : public fcitx::Action {
public:
    HuxToggleAction(hux_engine *engine, const char *option, const char *label)
        : engine_(engine), option_(option), label_(label) {
        setCheckable(true);
    }

    std::string shortText(fcitx::InputContext * /*unused*/) const override {
        return label_;
    }

    std::string icon(fcitx::InputContext * /*unused*/) const override {
        return {};
    }

    bool isChecked(fcitx::InputContext * /*unused*/) const override {
        return hux_engine_option_value(engine_, option_.c_str()) == 1;
    }

    void activate(fcitx::InputContext * /*unused*/) override {
        const int32_t value = hux_engine_option_value(engine_, option_.c_str());
        if (value >= 0) {
            hux_engine_set_option(engine_, option_.c_str(), value == 0 ? 1 : 0);
        }
    }

private:
    hux_engine *engine_;
    std::string option_;
    std::string label_;
};

class HuxEngine;

/// 每输入上下文会话（fcitx5 `InputContextProperty`）：持引擎侧会话 id，销毁时释放。
/// 组合/候选/学习暂存按会话隔离，互不干扰。
class HuxSession : public fcitx::InputContextProperty {
public:
    HuxSession(hux_engine *engine, uint64_t id) : engine_(engine), id_(id) {}

    /// 析构时引擎**必须仍存活**：本类由 `HuxEngine::~HuxEngine` 的
    /// `sessionFactory_.unregister()` 统一销毁（见该析构函数的契约注释），
    /// 那时 `hux_engine_free` 尚未执行。
    ~HuxSession() override {
        HUX_DEBUG() << "hux: ~HuxSession id=" << id_;
        hux_engine_session_free(engine_, id_);
    }

    uint64_t id() const { return id_; }

private:
    hux_engine *engine_;
    uint64_t id_;
};

/// 面板候选：点击（`select`）按全局索引选中并上屏（与空格相同的确认/学习链）。
///
/// **生命周期**：候选列表归 `InputContext` 的输入面板所有，而 IC **晚于** addon 实例
/// （含本引擎）析构（`InstancePrivate` 先声明 `icManager_`、后声明 `addonManager_`，
/// 故 `~HuxEngine` 运行时 IC 全部存活）⇒ 引擎释放后，面板里可能仍留着本对象，
/// 用户点一下就走到已释放的引擎上。故这里只持 `TrackableObjectReference`（弱引用，
/// fcitx5 惯用法，见 `cloudpinyin_public.h`），失效即早退。
class HuxCandidateWord : public fcitx::CandidateWord {
public:
    HuxCandidateWord(fcitx::Text text, fcitx::Text comment,
                     fcitx::TrackableObjectReference<HuxEngine> owner,
                     int32_t index)
        : CandidateWord(std::move(text)), owner_(owner), index_(index) {
        setComment(std::move(comment));
    }

    void select(fcitx::InputContext *inputContext) const override;

private:
    fcitx::TrackableObjectReference<HuxEngine> owner_;
    int32_t index_;
};

/// 引擎 addon。
///
/// 继承 `fcitx::TrackableObject<HuxEngine>`：让「归 IC / 面板所有、可能比引擎活得久」的
/// UI 对象（`HuxCandidateWord`）持弱引用，避免悬垂（见 `HuxCandidateWord` 注释）。
class HuxEngine : public fcitx::InputMethodEngine,
                  public fcitx::TrackableObject<HuxEngine> {
public:
    explicit HuxEngine(fcitx::Instance *instance)
        : instance_(instance),
          sessionFactory_([this](fcitx::InputContext & /*unused*/) {
              return new HuxSession(engine_, hux_engine_session_new(engine_));
          }) {
        fcitx::readAsIni(config_, "conf/hux.conf");
        hux_host host = {};
        host.user = this;
        host.commit = &HuxEngine::commitCallback;
        host.update = &HuxEngine::updateCallback;
        engine_ = hux_engine_new(&host);
        if (const char *status = hux_engine_status(engine_)) {
            FCITX_INFO() << "hux: " << status;
        }
        // 每输入上下文一个会话（现存的与后续新建的都会经工厂创建）。
        // 注册成功是 `~HuxEngine` 里 `unregister()` 能**销毁全部会话**的前提
        // （名字冲突时 fcitx5 直接返回 false 且不创建任何会话）；失败必须显式可见，
        // 否则「会话从不释放」会静默（见 `~HuxEngine` 契约注释）。
        if (!instance_->inputContextManager().registerProperty("huxSession",
                                                               &sessionFactory_)) {
            FCITX_WARN() << "hux: 会话属性注册失败（huxSession 名字冲突？）";
        }
        applyConfig();
        setupStatusMenu();
    }
    /// 析构契约：**必须先 `sessionFactory_.unregister()`，再 `hux_engine_free(engine_)`**。
    ///
    /// 这不是风格问题，而是「每个 `HuxSession` 析构时都要拿**仍存活**的引擎调
    /// `hux_engine_session_free`」这一前提。依据（按 fcitx5 源码逐层核对；所用版本
    /// 5.1.22 与 master 在这几个文件上**逐字节相同**，master 最后改动这些文件的提交
    /// 早于 5.1.22 tag）：
    ///   1. `src/lib/fcitx/inputcontextproperty.cpp`：`InputContextPropertyFactory::unregister()`
    ///      → `d->manager_->unregisterProperty(d->name_)`（析构函数亦调 `unregister()`）；
    ///   2. `src/lib/fcitx/inputcontextmanager.cpp`：`InputContextManagerPrivate::unregisterProperty(name)`
    ///      **遍历 `inputContexts_`** 逐个 `inputContext.d_func()->unregisterProperty(slot)`；
    ///   3. `src/lib/fcitx/inputcontext_p.h`：`InputContextPrivate::unregisterProperty(slot)` 是
    ///      `properties_[slot] = std::move(properties_.back()); properties_.pop_back();`，
    ///      而 `properties_` 是 `std::vector<std::unique_ptr<InputContextProperty>>`
    ///      ⇒ 被覆盖的 `unique_ptr` 在赋值当场析构 ⇒ **每个 `HuxSession` 当场析构**
    ///      （各自跑 `hux_engine_session_free(engine_, id_)`，此刻引擎仍存活）；
    ///   4. `src/lib/fcitx/inputcontextproperty.h` 注释：工厂「must unregister before the
    ///      destruction of `InputContextManager`」——addon 实例（含本引擎）先死、IC 后死
    ///      （`InstancePrivate` 先声明 `icManager_` 再声明 `addonManager_`，反向析构
    ///      ⇒ `~AddonManager`（`unload()` → 删 addon 实例）在 IC 之前）。
    /// 若调换（先 `free` 再 `unregister`）：每个 `HuxSession` 会对已释放的引擎调
    /// `hux_engine_session_free` ⇒ UAF。**因此本顺序是安全性的前提，改动前请重读本节。**
    /// 真机核对方法（析构顺序）：`fcitx5 -r --verbose='hux=5'` 前台运行，退出后确认全部
    /// `~HuxSession` 日志**早于** `~HuxEngine`（顺序相反即命中悬垂路径）。
    ~HuxEngine() override {
        HUX_DEBUG() << "hux: ~HuxEngine";
        // 1) 注销工厂 ⇒ fcitx5 立刻销毁全部已注册会话（见上方契约注释）。
        sessionFactory_.unregister();
        // 2) 摘掉 IC 状态区里指向本对象成员的裸指针（此时本对象成员仍存活）。
        clearStatusAreas();
        // 3) 引擎最后释放；指针置空，任何迟到的宿主回调都只是无操作。
        hux_engine_free(engine_);
        engine_ = nullptr;
    }

    /// 配置 schema（fcitx5-configtool 生成设置页；保存到 ~/.config/fcitx5/conf/hux.conf）。
    const fcitx::Configuration *getConfig() const override { return &config_; }

    /// 用户在配置工具中保存后：落盘由框架负责，这里应用到引擎（即时生效项）。
    void setConfig(const fcitx::RawConfig &raw) override {
        config_.load(raw, true);
        applyConfig();
    }

    void keyEvent(const fcitx::InputMethodEntry &entry,
                  fcitx::KeyEvent &keyEvent) override {
        FCITX_UNUSED(entry);
        const auto &key = keyEvent.key();
        fcitx::InputContext *inputContext = keyEvent.inputContext();
        HuxSession *huxSession = session(inputContext);
        if (huxSession == nullptr) {
            return;
        }
        context_ = inputContext;
        // 应用侧周边文本（字反查用；应用不支持时 valid=0）。
        const auto &surrounding = inputContext->surroundingText();
        if (surrounding.isValid()) {
            hux_engine_set_surrounding(engine_, huxSession->id(),
                                       surrounding.text().c_str(),
                                       static_cast<int32_t>(surrounding.cursor()), 1);
        } else {
            hux_engine_set_surrounding(engine_, huxSession->id(), nullptr, 0, 0);
        }
        const int32_t disposition =
            hux_engine_key(engine_, huxSession->id(), key.sym(),
                           key.states().toInteger(),
                           keyEvent.isRelease() ? 1 : 0);
        context_ = nullptr;
        if (disposition & HUX_KEY_FORWARD_AFTER_COMMIT) {
            // 布局转换键（如系统 colemak + 方案自定义 us 布局）：交回核心处理——
            // 核心在 ReservedLast 阶段会提交**转换后**的字符并消费该键；若本层
            // 自行 forwardKey，客户端会按系统布局重新解释该键，从而得到未经
            // fcitx5 映射的字符。
            if (!(keyEvent.forward() &&
                  keyEvent.rawKey().sym() != keyEvent.origKey().sym())) {
                // 已提交且未消费：先让提交送达，再由本层重发按键（与核心
                // `KeyEventOrderFix` 修法一致），避免前端在 keyEvent 返回后立刻
                // 转发按键导致「字母先于候选上屏」。
                keyEvent.filterAndAccept();
                inputContext->forwardKey(keyEvent.rawKey(), keyEvent.isRelease(),
                                         keyEvent.time());
            }
        } else if (disposition & HUX_KEY_CONSUMED) {
            keyEvent.filterAndAccept();
        }
    }

    void activate(const fcitx::InputMethodEntry &entry,
                  fcitx::InputContextEvent &event) override {
        FCITX_UNUSED(entry);
        // 会话在输入上下文注册时已建；激活只刷新状态区。
        updateStatusArea(event.inputContext());
    }

    void deactivate(const fcitx::InputMethodEntry &entry,
                    fcitx::InputContextEvent &event) override {
        FCITX_UNUSED(entry);
        // 失焦：由核心/前端把客户端预编辑以原文提交（fcitx5 惯例，不保留组合）；
        // 切换输入法/重置：本层直接丢弃（不提交；上游默认在切换时提交，本实现取丢弃）。
        resetSession(event);
    }

    void reset(const fcitx::InputMethodEntry &entry,
               fcitx::InputContextEvent &event) override {
        FCITX_UNUSED(entry);
        resetSession(event);
    }

    /// 面板候选点击：以该输入上下文交给引擎（提交/预编辑/候选经回调送出）。
    void selectCandidate(fcitx::InputContext *inputContext, int32_t index) {
        HuxSession *huxSession = session(inputContext);
        if (huxSession == nullptr) {
            return;
        }
        context_ = inputContext;
        hux_engine_select_candidate(engine_, huxSession->id(), index);
        context_ = nullptr;
    }

private:
    /// 该输入上下文对应会话（构造时注册的工厂保证已存在）。
    HuxSession *session(fcitx::InputContext *inputContext) const {
        if (inputContext == nullptr) {
            return nullptr;
        }
        return static_cast<HuxSession *>(inputContext->property(&sessionFactory_));
    }

    /// 状态菜单：注册「虎虚」子菜单与核心开关（构造时一次）。
    ///
    /// **选项键经 ABI 取自引擎**（`hux_engine_option_key`，与 `HUX_OPTION_*` 角色一一对应），
    /// 宿主只保留 UI 文案——方案改名或换方案时菜单自动跟随，不会静默失效。
    /// 文案表按 **ABI 角色下标**取（`kLabels[role]`），故长度必须等于 `HUX_OPTION_COUNT`：
    /// 角色数增加而文案漏补时，这里是越界读（UB）；`static_assert` 把它变成编译失败
    /// （角色**调序**由 Rust 侧 `option_role_order_matches_the_abi_header` 抓）。
    void setupStatusMenu() {
        static constexpr const char *kLabels[] = {
            "提前上屏", "提前上屏至预编辑", "单字重码组句", "全角标点", "数字直选",
        };
        static_assert(std::size(kLabels) == HUX_OPTION_COUNT,
                      "状态菜单文案表长度必须等于 HUX_OPTION_COUNT（ABI 角色数）");
        menuAction_.setShortText("虎虚");
        const int32_t roles = hux_engine_option_role_count();
        for (int32_t role = 0; role < roles; ++role) {
            const char *option = hux_engine_option_key(engine_, role);
            if (option == nullptr) {
                continue;
            }
            auto action = std::make_unique<HuxToggleAction>(engine_, option,
                                                            kLabels[role]);
            instance_->userInterfaceManager().registerAction(
                std::string("hux-") + option, action.get());
            menu_.addAction(action.get());
            toggleActions_.push_back(std::move(action));
        }
        menuAction_.setMenu(&menu_);
        instance_->userInterfaceManager().registerAction("hux-menu",
                                                         &menuAction_);
    }

    /// 把「虎虚」子菜单挂到当前输入上下文的状态区（仅在本输入法激活时显示）。
    void updateStatusArea(fcitx::InputContext *inputContext) {
        if (inputContext == nullptr) {
            return;
        }
        auto &statusArea = inputContext->statusArea();
        statusArea.clearGroup(fcitx::StatusGroup::InputMethod);
        statusArea.addAction(fcitx::StatusGroup::InputMethod, &menuAction_);
    }

    /// 摘掉**所有**输入上下文状态区里指向本对象成员的裸指针（`&menuAction_` 及其子菜单）。
    ///
    /// 归 IC 所有的对象比 addon 实例活得久（见 `~HuxEngine` 契约注释），故引擎释放前
    /// 必须把「虎虚」子菜单从每个 IC 摘除。这里用 `InputContextManager::foreach`
    /// （`inputcontextmanager.cpp`：遍历 `inputContexts_`，visitor 返回 false 即中止）
    /// + `StatusArea::clearGroup`（`statusarea.cpp`：逐个 `removeAction`）。
    ///
    /// 与 fcitx5 自带清理的关系：`StatusArea::addAction` 也连了 `Action::ObjectDestroyed`
    /// （动作析构时自摘 + `d->update()`），`~Element` 还会把自身从父节点摘除——本调用
    /// **不替代**它们，而是在自身成员仍有效时先把 UI 刷新一次，使状态区不再引用本对象
    /// 的任何成员；不把安全性寄托在「析构中途才触发的自清」上。
    void clearStatusAreas() {
        if (instance_ == nullptr) {
            return;
        }
        instance_->inputContextManager().foreach([](fcitx::InputContext *ic) {
            ic->statusArea().clearGroup(fcitx::StatusGroup::InputMethod);
            return true;
        });
    }

    /// 清空会话与面板（deactivate/reset 共用）。
    void resetSession(fcitx::InputContextEvent &event) {
        fcitx::InputContext *inputContext = event.inputContext();
        HuxSession *huxSession = session(inputContext);
        if (huxSession == nullptr) {
            return;
        }
        context_ = inputContext;
        hux_engine_reset(engine_, huxSession->id());
        context_ = nullptr;
    }

    /// 引擎 → 宿主的两个回调（`hux_host.user` 就是 `this`）。
    ///
    /// `user` 指针安全性：Rust 侧只在 `hux_engine_*` 调用**内部**同步回调，而引擎的
    /// 整个生命周期都包在 `~HuxEngine` 内（`hux_engine_free` 是析构体的一步），
    /// 故回调期间 `this` 必存活。`~HuxEngine` 里把 `engine_` 置空，使「引擎已释放后
    /// 仍被调用」这种本不该发生的路径退化成无操作而非解引用悬垂指针。
    static void commitCallback(void *user, const char *text) {
        static_cast<HuxEngine *>(user)->applyCommit(text);
    }

    static void updateCallback(void *user, const char *preedit, int32_t cursor,
                               const char *const *texts,
                               const char *const *comments, int32_t count,
                               int32_t selected, const char *auxUp,
                               const char *auxDown) {
        static_cast<HuxEngine *>(user)->applyUpdate(
            preedit, cursor, texts, comments, count, selected, auxUp, auxDown);
    }

    void applyCommit(const char *text) {
        if (engine_ == nullptr) {
            return;
        }
        if (context_ != nullptr && text != nullptr && *text != '\0') {
            context_->commitString(text);
        }
    }

    /// 应用 UI 快照：preedit（面板 + 客户端内联）+ 候选列表与高亮。
    void applyUpdate(const char *preedit, int32_t cursor,
                     const char *const *texts, const char *const *comments,
                     int32_t count, int32_t selected, const char *auxUp,
                     const char *auxDown) {
        // 引擎已释放（理论上不会发生，见 `commitCallback` 注释）：本函数后面还要经
        // `hux_engine_option_key(engine_, …)` 触碰引擎，故先早退。
        if (engine_ == nullptr || context_ == nullptr) {
            return;
        }
        std::string preeditString = preedit != nullptr ? preedit : "";
        fcitx::Text preeditText(preeditString);
        if (cursor >= 0 &&
            static_cast<size_t>(cursor) <= preeditString.size()) {
            preeditText.setCursor(cursor);
        }
        // 候选窗口预编辑：可配置关闭（关闭后仅候选与注释）。
        context_->inputPanel().setPreedit(
            config_.behavior->panelPreedit.value() ? preeditText
                                                   : fcitx::Text());
        // 客户端内联预编辑：跟随 fcitx5 全局预编辑设置（`isPreeditEnabled`）。
        // 失焦时由核心/前端以预编辑原文提交（fcitx5 惯例），因此不做 `DontCommit` 标记——
        // 标记后 KWin（input-method v1）在提交串为空时不发 commit，会在应用内留下可被保存的
        // marked text「残影」。
        context_->inputPanel().setClientPreedit(context_->isPreeditEnabled() ? preeditText
                                                                            : fcitx::Text());
        context_->updatePreedit();

        // 候选：无候选时置 `nullptr` 清除（fcitx5 约定）。**不可**留下「存在但为空」的
        // 列表——其他组件会对它调用 `candidate(0)`（如 fcitx5-table 的
        // `TableState::keyEvent`），抛 `CommonCandidateList: invalid index` 并 abort。
        if (count <= 0) {
            context_->inputPanel().setCandidateList(nullptr);
        } else {
            auto candidateList = std::make_unique<fcitx::CommonCandidateList>();
            for (int32_t index = 0; index < count; ++index) {
                const char *text =
                    texts != nullptr && texts[index] != nullptr ? texts[index] : "";
                const char *comment =
                    comments != nullptr && comments[index] != nullptr
                        ? comments[index]
                        : "";
                candidateList->append<HuxCandidateWord>(
                    fcitx::Text(text), fcitx::Text(comment), watch(), index);
            }
            // 数字直选：面板显示 1–9 / 0 序号（与引擎页内定位一致）。
            // 取**运行时生效值**（状态菜单可切换、options.yaml 优先），而非仅读配置页设置。
            const char *digitSelectKey =
                hux_engine_option_key(engine_, HUX_OPTION_DIGIT_SELECT);
            const bool digitSelect =
                digitSelectKey != nullptr &&
                hux_engine_option_value(engine_, digitSelectKey) == 1;
            if (digitSelect) {
                candidateList->setSelectionKey(digitSelectionKeys());
            }
            // 翻页交由 fcitx5 面板（页大小与引擎一致）：绝对索引 → 全局光标 + 所在页。
            candidateList->setPageSize(std::clamp(
                config_.behavior->pageSize.value(), kPageSizeMin, kPageSizeMax));
            // 候选排列：仅显式选择横排/竖排时设置 LayoutHint（跟随全局时保持 NotSet，
            // 由 fcitx5 全局「候选竖排」设置决定）。
            const auto layout = config_.behavior->candidateLayout.value();
            if (layout != HuxCandidateLayout::FollowGlobal) {
                candidateList->setLayoutHint(
                    layout == HuxCandidateLayout::Vertical
                        ? fcitx::CandidateLayoutHint::Vertical
                        : fcitx::CandidateLayoutHint::Horizontal);
            }
            // 防御：越界不设光标索引。
            const int index = std::min(std::max(selected, 0), count - 1);
            candidateList->setGlobalCursorIndex(index);
            const int page = index / candidateList->pageSize();
            if (page < candidateList->totalPages()) {
                candidateList->setPage(page);
            }
            context_->inputPanel().setCandidateList(std::move(candidateList));
        }
        // 字反查（⑧-2）：两排辅助文本（上排 = 光标左、下排 = 光标右）；空串清除。
        const auto auxText = [](const char *value) {
            return value != nullptr && *value != '\0' ? fcitx::Text(value) : fcitx::Text();
        };
        context_->inputPanel().setAuxUp(auxText(auxUp));
        context_->inputPanel().setAuxDown(auxText(auxDown));
        context_->updateUserInterface(fcitx::UserInterfaceComponent::InputPanel);
        updateStatusArea(context_);
    }

    /// 把 schema 值经 C ABI 推给 Rust 侧（`Settings::apply_settings`）。
    void applyConfig() {
        if (engine_ == nullptr) {
            return;
        }
        hux_options options = {};
        const auto &behavior = config_.behavior.value();
        const auto &hotkeys = config_.hotkeys.value();
        options.early_commit = behavior.earlyCommit.value() ? 1 : 0;
        options.early_commit_to_preedit =
            behavior.earlyCommitToPreedit.value() ? 1 : 0;
        options.allow_duplicate_single =
            behavior.allowDuplicateSingle.value() ? 1 : 0;
        options.full_shape = behavior.fullShape.value() ? 1 : 0;
        options.ascii_punct = behavior.asciiPunct.value() ? 1 : 0;
        options.learning_on_tab = behavior.learningOnTab.value() ? 1 : 0;
        options.high_freq_limit = behavior.highFreqLimit.value();
        fillKeyList(&options.reverse_lookup_pronunciation,
                    hotkeys.reverseLookupPronunciationKeys.value());
        fillKeyList(&options.reverse_lookup_character,
                    hotkeys.reverseLookupCharacterKeys.value());
        options.page_size = behavior.pageSize.value();
        fillKeyList(&options.page_up, hotkeys.pageUpKeys.value());
        fillKeyList(&options.page_down, hotkeys.pageDownKeys.value());
        options.digit_select = behavior.digitSelect.value() ? 1 : 0;
        switch (behavior.candidateLayout.value()) {
        case HuxCandidateLayout::Horizontal:
            options.candidate_layout = 1;
            break;
        case HuxCandidateLayout::Vertical:
            options.candidate_layout = 2;
            break;
        default:
            options.candidate_layout = 0;
            break;
        }
        const auto preeditMode = behavior.preeditMode.value();
        options.preedit_mode = preeditMode == HuxPreeditMode::RawInput   ? 1
                               : preeditMode == HuxPreeditMode::Hidden ? 2
                                                                       : 0;
        options.page_cycle = behavior.pageCycle.value() ? 1 : 0;
        options.min_retained_input_length =
            behavior.minRetainedInputLength.value();
        if (hux_engine_apply_settings(engine_, &options) == 0) {
            FCITX_WARN() << "hux: apply settings failed";
        }
        // 配置页可能绑到**没有名字的 keysym**（媒体键 / 厂商扩展键）：Rust 侧无法把它解析成
        // rime 键名，该绑定会被丢弃——诊断经状态串的 `hotkeys:` 前缀送出。
        // 这里把最新状态串落到日志，使「绑定静默消失」变得可诊断；按契约指针只用一次
        // （`hux_engine_status` 在下一次状态刷新后失效，见 `hux_abi.h`）。
        if (const char *status = hux_engine_status(engine_)) {
            FCITX_INFO() << "hux: " << status;
        }
    }

    HuxConfig config_;
    fcitx::Instance *instance_;
    hux_engine *engine_ = nullptr;
    /// 会话工厂（每输入上下文一个 [`HuxSession`]）。
    fcitx::FactoryFor<HuxSession> sessionFactory_;
    /// 当前引擎调用的目标输入上下文：只在一次 `hux_engine_*` 调用期间置位、随即清空。
    /// IC 的生命周期长于本对象（见 `~HuxEngine` 契约注释），故该指针本身不会悬垂；
    /// 引擎释放后由 `applyCommit`/`applyUpdate` 的 `engine_ == nullptr` 早退兜住。
    fcitx::InputContext *context_ = nullptr;
    fcitx::Menu menu_;
    fcitx::SimpleAction menuAction_;
    std::vector<std::unique_ptr<HuxToggleAction>> toggleActions_;
};

void HuxCandidateWord::select(fcitx::InputContext *inputContext) const {
    // 引擎（addon 实例）可能已先于本候选对象析构（IC 晚于 addon，见类注释）：
    // 弱引用失效 ⇒ 直接返回，**不触碰**已释放的引擎。
    HuxEngine *owner = owner_.get();
    if (owner == nullptr) {
        return;
    }
    owner->selectCandidate(inputContext, index_);
}

class HuxFactory : public fcitx::AddonFactory {
public:
    fcitx::AddonInstance *create(fcitx::AddonManager *manager) override {
        return new HuxEngine(manager->instance());
    }
};

} // namespace

// C 布局守卫（与 Rust `crates/hux-ffi/src/lib.rs` 的 `c_layout_matches_header` 对应）：
// 本壳逐字段填充 `hux_options`、Rust 侧逐字段读取，字段顺序/宽度漂移在两侧都能编译通过，
// 故在此钉住尺寸与关键偏移——改 `hux_abi.h` 时必须同步三处。
static_assert(sizeof(hux_key_list) == 4 + 2 * HUX_MAX_KEYS * 4,
              "hux_key_list 布局与 Rust 契约不一致");
static_assert(sizeof(hux_options) == 13 * 4 + 4 * sizeof(hux_key_list),
              "hux_options 布局与 Rust 契约不一致");
static_assert(offsetof(hux_options, reverse_lookup_character) == 7 * 4 + sizeof(hux_key_list),
              "hux_options 字段顺序与 Rust 契约不一致");
static_assert(offsetof(hux_options, min_retained_input_length) == 12 * 4 + 4 * sizeof(hux_key_list),
              "hux_options 末尾字段偏移与 Rust 契约不一致");

FCITX_ADDON_FACTORY(HuxFactory);
