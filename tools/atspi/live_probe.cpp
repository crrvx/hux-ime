// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later
//
// live_probe.cpp —— 在**真实桌面会话**里诊断「字反查」的 AT-SPI 取字来源。
//
// 与 probe.cpp 不同：probe.cpp 跑在私有总线上的场景夹具里（退出码即断言），这里连的是
// 当前会话的无障碍总线、问的是**正在用的那个应用**，专门回答「取不到 / 取到的不跟手」：
//
//   1. `atspi_init()` 通不通（不通 ⇒ 无障碍没开，或本会话没有 at-spi 总线）；
//   2. 树里有没有 FOCUSED 节点（没有 ⇒ 应用没把焦点挂上树，多半也是无障碍没开）；
//   3. 焦点节点有没有 `Text` 接口、`CharacterCount` / `CaretOffset` 读不读得到
//      （属性不支持时 libatspi 给 -1 并带 error —— 浏览器的某些控件就是这样）；
//   4. 文本与光标**跟不跟得上**：在目标应用里移动光标，看这里的读数变不变。
//
// 取值口径与 platform/fcitx5/shell/atspi_source.cpp 一致（同一套调用、同样的**字符制**
// 偏移与「光标左侧窗口」），故结论可以直接对号：
//   · 这里能看到、光标也跟着变，插件却卡住 ⇒ 问题在插件一侧；
//   · 这里也卡住 / 找不到焦点 ⇒ 问题在应用或无障碍一侧，改插件没用。
//
// 用法（在桌面会话的终端里跑；**别**在 dbus-run-session 或私有总线里跑）：
//     bash tools/atspi/live-probe.sh [轮数]
// 直接编译（注释里别用行尾反斜杠折行，会触发 -Wcomment）：
//   g++ -std=c++17 -O1 -Wall -Wextra
//   tools/atspi/live_probe.cpp -o /tmp/live_probe
//   $(pkg-config --cflags --libs atspi-2 gobject-2.0)
//
// 退出码：0 = 正常跑完；2 = 无障碍总线不可用。

#include <atspi/atspi.h>

#include <chrono>
#include <cstdio>
#include <cstdlib>
#include <string>
#include <thread>
#include <vector>

namespace {

/// 与 atspi_source.cpp 的常量保持一致（窗口、遍历预算、超时）。
constexpr int kWindowChars = 64;
constexpr int kWalkMaxNodes = 256;
constexpr int kWalkMaxDepth = 6;
constexpr int kWalkBudgetMs = 1200;
constexpr int kMethodTimeoutMs = 150;
constexpr int kPollMs = 200;

int64_t nowMs() {
    return std::chrono::duration_cast<std::chrono::milliseconds>(
               std::chrono::steady_clock::now().time_since_epoch())
        .count();
}

/// 取字符串最后一个**完整** UTF-8 字符 —— 光标左侧那个字就是它。
std::string lastChar(const std::string &text) {
    if (text.empty()) {
        return {};
    }
    size_t start = text.size() - 1;
    while (start > 0 && (static_cast<unsigned char>(text[start]) & 0xC0) == 0x80) {
        --start;
    }
    return text.substr(start);
}

std::string nameOf(AtspiAccessible *object) {
    GError *error = nullptr;
    gchar *name = atspi_accessible_get_name(object, &error);
    if (error != nullptr) {
        g_error_free(error);
    }
    std::string result = name != nullptr ? std::string(name) : std::string("?");
    if (name != nullptr) {
        g_free(name);
    }
    return result;
}

std::string roleOf(AtspiAccessible *object) {
    GError *error = nullptr;
    gchar *role = atspi_accessible_get_role_name(object, &error);
    if (error != nullptr) {
        g_error_free(error);
    }
    std::string result = role != nullptr ? std::string(role) : std::string("?");
    if (role != nullptr) {
        g_free(role);
    }
    return result;
}

bool hasState(AtspiAccessible *object, AtspiStateType state) {
    AtspiStateSet *states = atspi_accessible_get_state_set(object);
    const bool found = states != nullptr && atspi_state_set_contains(states, state);
    if (states != nullptr) {
        g_object_unref(states);
    }
    return found;
}

/// 有界深度优先（与实现同一套预算），返回**新引用**；未命中返回 `nullptr`。
AtspiAccessible *walkForFocused(AtspiAccessible *node, int depth, int *budget, int64_t deadline) {
    if (node == nullptr || depth > kWalkMaxDepth || *budget <= 0 || nowMs() >= deadline) {
        return nullptr;
    }
    if (depth > 0) {
        --*budget;
    }
    if (hasState(node, ATSPI_STATE_FOCUSED)) {
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
            return nullptr;
        }
        AtspiAccessible *child = atspi_accessible_get_child_at_index(node, i, &error);
        if (error != nullptr) {
            g_error_free(error);
            error = nullptr;
        }
        if (child == nullptr) {
            continue;
        }
        AtspiAccessible *found = walkForFocused(child, depth + 1, budget, deadline);
        g_object_unref(child);
        if (found != nullptr) {
            return found;
        }
    }
    return nullptr;
}

/// 一轮的读数（只保留打印用的结论，不持有任何 AT-SPI 引用）。
struct Reading {
    bool found = false;      ///< 有没有找到 FOCUSED 节点
    std::string app;         ///< 命中焦点节点的那个应用（desktop 的子节点）名
    std::string role;        ///< 焦点节点的角色名
    bool editable = false;   ///< 焦点节点带 EDITABLE 状态
    bool hasText = false;    ///< 焦点节点提供 Text 接口
    int chars = -1;          ///< CharacterCount（属性；不支持时为 -1）
    int caret = -1;          ///< CaretOffset（属性；不支持时为 -1）
    bool caretError = false; ///< CaretOffset 读失败（属性不支持，与「真的是 0」区分）
    std::string window;      ///< 光标左侧窗口（与实现同口径）
    std::string apps;        ///< 桌面下的应用名清单（找不到焦点时用来判断无障碍有没有开）
};

/// 探测一次：桌面 → 各应用 → 有界遍历找 FOCUSED → Text / caret / 左侧窗口。
Reading probeOnce() {
    Reading reading;
    AtspiAccessible *desktop = atspi_get_desktop(0);
    if (desktop == nullptr) {
        return reading;
    }
    g_object_ref(desktop); // 统一由本函数末尾释放
    GError *error = nullptr;
    const gint appCount = atspi_accessible_get_child_count(desktop, &error);
    if (error != nullptr) {
        g_error_free(error);
        error = nullptr;
    }
    for (gint i = 0; i < appCount; ++i) {
        AtspiAccessible *app = atspi_accessible_get_child_at_index(desktop, i, &error);
        if (error != nullptr) {
            g_error_free(error);
            error = nullptr;
        }
        if (app == nullptr) {
            continue;
        }
        if (!reading.apps.empty()) {
            reading.apps += " / ";
        }
        reading.apps += nameOf(app);

        const int64_t deadline = nowMs() + kWalkBudgetMs;
        int budget = kWalkMaxNodes;
        AtspiAccessible *focused = walkForFocused(app, 0, &budget, deadline);
        if (focused == nullptr) {
            g_object_unref(app);
            continue;
        }
        reading.found = true;
        reading.app = nameOf(app);
        reading.role = roleOf(focused);
        reading.editable = hasState(focused, ATSPI_STATE_EDITABLE);

        AtspiText *text = atspi_accessible_get_text(focused);
        if (text != nullptr) {
            reading.hasText = true;
            reading.chars = atspi_text_get_character_count(text, &error);
            if (error != nullptr) {
                g_error_free(error);
                error = nullptr;
            }
            reading.caret = atspi_text_get_caret_offset(text, &error);
            if (error != nullptr) {
                reading.caretError = true;
                g_error_free(error);
                error = nullptr;
            }
            if (reading.chars > 0 && reading.caret >= 0) {
                const gint begin = reading.caret > kWindowChars ? reading.caret - kWindowChars : 0;
                gchar *raw = atspi_text_get_text(text, begin, reading.caret, &error);
                if (error != nullptr) {
                    g_error_free(error);
                    error = nullptr;
                }
                if (raw != nullptr) {
                    reading.window.assign(raw);
                    g_free(raw);
                }
            }
        }
        g_object_unref(focused);
        g_object_unref(app);
        break; // 与实现一致：取第一个命中的焦点对象
    }
    g_object_unref(desktop);
    return reading;
}

std::string describe(const Reading &reading) {
    if (!reading.found) {
        return "没有 FOCUSED 节点（树里的应用：" + (reading.apps.empty() ? std::string("无") : reading.apps) +
               "）";
    }
    std::string line = reading.app + " → " + reading.role;
    if (reading.editable) {
        line += "[可编辑]";
    }
    if (!reading.hasText) {
        return line + "｜焦点节点没有 Text 接口（这种控件取不到字）";
    }
    line += "｜CharacterCount=" + std::to_string(reading.chars);
    if (reading.caretError) {
        return line + "｜CaretOffset=读不到（属性不支持 ⇒ 取不到光标）";
    }
    line += " CaretOffset=" + std::to_string(reading.caret);
    if (!reading.window.empty()) {
        line += "｜光标左侧窗口=\"" + reading.window + "\"";
        line += " 该字=\"" + lastChar(reading.window) + "\"";
    }
    return line;
}

} // namespace

int main(int argc, char **argv) {
    int rounds = argc > 1 ? std::atoi(argv[1]) : 100;
    if (rounds <= 0) {
        rounds = 100;
    }
    const int init = atspi_init();
    if (init != 0) {
        std::printf("atspi_init() = %d —— 连不上无障碍总线。\n", init);
        std::printf("⇒ 无障碍没开（KDE「辅助功能」/ GNOME 的 toolkit-accessibility），"
                    "或本会话没有 at-spi-bus-launcher（装 at-spi2-core 后重新登录）。\n");
        std::printf("  这种情况下虎虚只能退回「本会话最近上屏文本」，光标移动后就停在旧值。\n");
        return 2;
    }
    atspi_set_timeout(kMethodTimeoutMs, kMethodTimeoutMs);
    std::printf("无障碍总线可用。每轮 %d ms，共 %d 轮；请在目标应用的输入框里点一下，"
                "再用 ←/→ 移动光标。\n",
                kPollMs, rounds);
    std::printf("只在读数变化时打印：\n");

    std::string last;
    int changes = 0;
    for (int round = 0; round < rounds; ++round) {
        const Reading reading = probeOnce();
        const std::string line = describe(reading);
        if (round == 0 || line != last) {
            std::printf("[%3d] %s\n", round, line.c_str());
            std::fflush(stdout);
            ++changes;
        }
        last = line;
        std::this_thread::sleep_for(std::chrono::milliseconds(kPollMs));
    }
    std::printf("\n共 %d 轮，读数变化 %d 次。\n", rounds, changes);
    if (changes <= 1) {
        std::printf("⇒ 期间读数没变：应用没把光标/文本变化暴露给无障碍（换个应用的输入框再试一次，"
                    "以区分「应用的问题」与「无障碍没开」）。\n");
    }
    std::printf("对照：插件侧的日志（`fcitx5 -r --verbose='hux=5'`）会打 `hux: 取字来源 …`，"
                "只记来源/字节数/光标，不含正文。\n");
    atspi_exit();
    return 0;
}
