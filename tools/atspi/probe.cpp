// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later
//
// probe.cpp —— 驱动 `hux::AtspiSource`（取字来源本体）的场景探针：退出码即断言结果。
//
// 构建（由 run.sh 负责）：g++ -O0 -g -std=c++17 -DHUX_HAVE_ATSPI=1 -I platform/fcitx5/shell
// probe.cpp + platform/fcitx5/shell/atspi_source.cpp，并链 atspi-2 / gobject-2.0。
//
// 子命令：
//   wait <期望文本> <期望光标字符数> [--timeout=MS]
//       等 `snapshot()` 出现该值；同时校验 `cursorChars == 期望`。
//   null [--expect=<any|no-source|no-text>] [--timeout=MS]
//       等一个稳定窗口内始终 `nullopt`（有文本却报空 = 失败）。
//   fast [--timeout=MS]
//       无障碍总线不可用：`snapshot()` / `requestRefresh()` 必须**立即**返回（各 < 50 ms）。
//   refresh <控制文件> <新文本> [--caret=N] [--timeout=MS]
//       原子改写控制文件（`新文本<TAB>N`）后 `requestRefresh()`，量出快照更新耗时
//       （轮询等待，默认上限 2 s）。期望值是**新光标左侧的窗口**，与 wait 同语义。
//
// 计时与轮询都在本进程里做：探针不 sleep 赌结果，只按上限轮询。

#include "atspi_source.h"

#include <chrono>
#include <cstdint>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <exception>
#include <string>
#include <thread>
#include <vector>

namespace {

using Clock = std::chrono::steady_clock;

double msSince(Clock::time_point start) {
    return std::chrono::duration<double, std::milli>(Clock::now() - start).count();
}

int gChecks = 0;
int gFailures = 0;

void check(bool ok, const std::string &what, const std::string &detail = std::string()) {
    ++gChecks;
    if (!ok) {
        ++gFailures;
    }
    std::printf("  [%s] %s%s\n", ok ? "OK" : "FAIL", what.c_str(),
                detail.empty() ? "" : (" —— " + detail).c_str());
    std::fflush(stdout);
}

/// 快照的可读形式。
std::string describe(const std::optional<hux::AtspiSource::Snapshot> &snap) {
    if (!snap.has_value()) {
        return "nullopt";
    }
    return "text=\"" + snap->text + "\" cursorChars=" + std::to_string(snap->cursorChars);
}

/// 轮询取快照，直到 `pred` 成立（返回 true）或超时（返回 false）。顺手统计观察次数。
///
/// 每轮都 `requestRefresh()`：实现只在**被请求**时才推进（`requestRefresh()` 同时是惰性
/// 起线程的触发点），只读 `snapshot()` 会永远看到「还没查过」的空缓存。
template <typename Pred>
bool pollSnapshot(hux::AtspiSource &source, int timeoutMs, Pred pred) {
    const Clock::time_point deadline =
        Clock::now() + std::chrono::milliseconds(timeoutMs);
    while (true) {
        source.requestRefresh(); // 合并排队；不等它完成
        if (pred(source.snapshot())) {
            return true;
        }
        if (Clock::now() >= deadline) {
            return false;
        }
        std::this_thread::sleep_for(std::chrono::milliseconds(10));
    }
}

/// 控制文件：原子改写（临时文件 + rename），内容为 `新文本<TAB>新光标`。
bool writeControlFile(const std::string &path, const std::string &text, int caret) {
    const std::string tmp = path + ".tmp";
    std::FILE *handle = std::fopen(tmp.c_str(), "wb");
    if (handle == nullptr) {
        return false;
    }
    const std::string payload = text + "\t" + std::to_string(caret) + "\n";
    const size_t written = std::fwrite(payload.data(), 1, payload.size(), handle);
    const int closed = std::fclose(handle);
    if (written != payload.size() || closed != 0) {
        return false;
    }
    return std::rename(tmp.c_str(), path.c_str()) == 0;
}

std::vector<std::string> positional;

int timeoutFromArgs(int fallback) {
    for (const std::string &arg : positional) {
        if (arg.rfind("--timeout=", 0) == 0) {
            return std::atoi(arg.c_str() + 10);
        }
    }
    return fallback;
}

std::string valueFromArgs(const char *prefix, const std::string &fallback) {
    const size_t len = std::strlen(prefix);
    for (const std::string &arg : positional) {
        if (arg.rfind(prefix, 0) == 0) {
            return arg.substr(len);
        }
    }
    return fallback;
}

/// 有文本参数的命令：位置参数按顺序取（跳过 -- 开头的选项）。
std::string positionalAt(size_t index) {
    size_t seen = 0;
    for (const std::string &arg : positional) {
        if (arg.rfind("--", 0) == 0) {
            continue;
        }
        if (seen == index) {
            return arg;
        }
        ++seen;
    }
    return std::string();
}

// -------------------------------------------------------------------- 场景

int caseWait(const std::string &expectedText, uint32_t expectedCursor, int timeoutMs) {
    hux::AtspiSource source;
    const Clock::time_point start = Clock::now();
    bool seen = false;
    bool cursorOk = true;
    std::string cursorDetail;
    std::optional<hux::AtspiSource::Snapshot> last;

    const Clock::time_point deadline =
        Clock::now() + std::chrono::milliseconds(timeoutMs);
    while (true) {
        source.requestRefresh(); // 合并排队；不等它完成
        last = source.snapshot();
        if (last.has_value() && last->text == expectedText) {
            seen = true;
            if (last->cursorChars != expectedCursor) {
                cursorOk = false;
                cursorDetail = "cursorChars=" + std::to_string(last->cursorChars) +
                               "，期望 " + std::to_string(expectedCursor);
            }
            break;
        }
        if (Clock::now() >= deadline) {
            break;
        }
        std::this_thread::sleep_for(std::chrono::milliseconds(10));
    }

    std::printf("  等待 %.0fms 后快照 = %s\n", msSince(start), describe(last).c_str());
    check(seen, "快照文本 == 期望", "期望 \"" + expectedText + "\"，实际 " +
                                        describe(last));
    if (seen) {
        check(cursorOk, "快照光标（字符数）", cursorDetail.empty()
                                                   ? ("cursorChars=" +
                                                      std::to_string(expectedCursor))
                                                   : cursorDetail);
    }
    return gFailures == 0 ? 0 : 1;
}

int caseNull(const std::string &expect, int timeoutMs) {
    hux::AtspiSource source;
    const Clock::time_point start = Clock::now();
    // 稳定窗口：先给一个「来得及出现」的时间，再确认维持空 —— 只查一次会漏掉慢一拍的来源。
    const Clock::time_point settle = Clock::now() + std::chrono::milliseconds(700);
    bool sawValue = false;
    std::optional<hux::AtspiSource::Snapshot> last;
    while (Clock::now() < settle) {
        source.requestRefresh();
        last = source.snapshot();
        if (last.has_value()) {
            sawValue = true;
            break;
        }
        std::this_thread::sleep_for(std::chrono::milliseconds(10));
    }
    if (sawValue && expect == "any") {
        std::printf("  快照在 %.0fms 出现：%s\n", msSince(start), describe(last).c_str());
        check(true, "出现非空快照", describe(last));
        return 0;
    }
    if (sawValue) {
        check(false, "期望 " + expect + "（无非空快照）", describe(last));
        return 1;
    }
    check(true, "快照为空（" + expect + "）", "等待 " + std::to_string(timeoutMs) +
                                                 "ms 上限内未出现非空快照");
    return 0;
}

int caseFast(int timeoutMs) {
    hux::AtspiSource source;
    const Clock::time_point start = Clock::now();
    const std::optional<hux::AtspiSource::Snapshot> snap = source.snapshot();
    const double snapMs = msSince(start);
    const Clock::time_point start2 = Clock::now();
    source.requestRefresh();
    const double refreshMs = msSince(start2);
    std::printf("  snapshot() %.2fms / requestRefresh() %.2fms（上限 %dms）\n", snapMs,
                refreshMs, timeoutMs);
    check(!snap.has_value(), "无障碍总线不可用时 snapshot() 为空", describe(snap));
    check(snapMs < timeoutMs, "snapshot() 立即返回", std::to_string(snapMs) + "ms");
    check(refreshMs < timeoutMs, "requestRefresh() 立即返回",
          std::to_string(refreshMs) + "ms");
    // 再确认「等一会儿也不阻塞」：请求刷新后台线程后按键路径仍是纯缓存读。
    std::this_thread::sleep_for(std::chrono::milliseconds(200));
    const Clock::time_point start3 = Clock::now();
    const std::optional<hux::AtspiSource::Snapshot> snap2 = source.snapshot();
    const double snap2Ms = msSince(start3);
    const Clock::time_point start4 = Clock::now();
    source.requestRefresh();
    const double refresh2Ms = msSince(start4);
    std::printf("  后台线程起来后：snapshot() %.2fms / requestRefresh() %.2fms\n", snap2Ms,
                refresh2Ms);
    check(!snap2.has_value(), "后台线程起来后 snapshot() 仍为空", describe(snap2));
    check(snap2Ms < timeoutMs && refresh2Ms < timeoutMs, "后台线程起来后仍立即返回",
          std::to_string(snap2Ms) + "ms / " + std::to_string(refresh2Ms) + "ms");
    return gFailures == 0 ? 0 : 1;
}

/// UTF-8 里前 `chars` 个字符所占的字节数（不足则整串）。
size_t bytesForChars(const std::string &text, size_t chars) {
    size_t seen = 0;
    for (size_t i = 0; i < text.size();) {
        if (seen == chars) {
            return i;
        }
        const unsigned char lead = static_cast<unsigned char>(text[i]);
        size_t len = 1;
        if ((lead & 0xE0) == 0xC0) {
            len = 2;
        } else if ((lead & 0xF0) == 0xE0) {
            len = 3;
        } else if ((lead & 0xF8) == 0xF0) {
            len = 4;
        }
        i += len;
        ++seen;
    }
    return text.size();
}

int caseRefresh(const std::string &controlFile, const std::string &newText, int newCaret,
                int timeoutMs) {
    hux::AtspiSource source;
    // 先确保来源已经就绪（有非空快照），否则「更新」无从谈起。
    if (!pollSnapshot(source, timeoutMs, [](const std::optional<hux::AtspiSource::Snapshot> &s) {
            return s.has_value();
        })) {
        check(false, "刷新前置：来源从未就绪", "等待上限 " + std::to_string(timeoutMs) + "ms");
        return 1;
    }
    // 期望值：新文本在**光标左侧的窗口**（与 wait 模式同一语义 —— 取的是窗口，不是整串）。
    const size_t chars = static_cast<size_t>(newCaret);
    const std::string expectedWindow = newText.substr(0, bytesForChars(newText, chars));
    // 改文件与请求刷新必须紧邻：否则中间那一次周期复查会先把新文本取回来，计时就没意义了。
    const Clock::time_point start = Clock::now();
    if (!writeControlFile(controlFile, newText, newCaret)) {
        check(false, "改写控制文件", controlFile);
        return 1;
    }
    source.requestRefresh();
    const bool updated = pollSnapshot(
        source, timeoutMs,
        [&expectedWindow](const std::optional<hux::AtspiSource::Snapshot> &s) {
            return s.has_value() && s->text == expectedWindow;
        });
    const double elapsed = msSince(start);
    std::printf("  改写后 %.0fms 内快照 = %s（期望窗口 \"%s\"）\n", elapsed,
                describe(source.snapshot()).c_str(), expectedWindow.c_str());
    check(updated, "requestRefresh() 后快照在 " + std::to_string(timeoutMs) +
                       "ms 内更新为新文本的窗口",
          "实际 " + std::to_string(elapsed) + "ms / " + describe(source.snapshot()));
    if (updated) {
        const auto snap = source.snapshot();
        check(snap.has_value() && snap->cursorChars == static_cast<uint32_t>(newCaret),
              "更新后的光标（字符数）",
              snap.has_value() ? ("cursorChars=" + std::to_string(snap->cursorChars) +
                                  "，期望 " + std::to_string(newCaret))
                               : "nullopt");
    }
    return gFailures == 0 ? 0 : 1;
}

} // namespace

int main(int argc, char **argv) {
    std::string mode;
    for (int i = 1; i < argc; ++i) {
        if (mode.empty() && std::strncmp(argv[i], "--", 2) != 0) {
            mode = argv[i];
            continue;
        }
        positional.emplace_back(argv[i]);
    }
    if (mode.empty()) {
        std::fprintf(stderr, "用法：probe <wait|null|fast|refresh> [参数]\n");
        return 2;
    }
    std::printf("probe: 模式 %s，compiled()=%d\n", mode.c_str(),
                hux::AtspiSource::compiled() ? 1 : 0);
    if (!hux::AtspiSource::compiled()) {
        std::fprintf(stderr, "probe: 本构建没有 AT-SPI（HUX_HAVE_ATSPI 未定义）\n");
        return 2;
    }
    try {
        if (mode == "wait") {
            const std::string text = positionalAt(0);
            const uint32_t cursor = static_cast<uint32_t>(std::atoi(positionalAt(1).c_str()));
            if (text.empty()) {
                std::fprintf(stderr, "probe: wait 需要期望文本\n");
                return 2;
            }
            return caseWait(text, cursor, timeoutFromArgs(5000));
        }
        if (mode == "null") {
            return caseNull(valueFromArgs("--expect=", "no-source"), timeoutFromArgs(5000));
        }
        if (mode == "fast") {
            return caseFast(timeoutFromArgs(50));
        }
        if (mode == "refresh") {
            const std::string controlFile = positionalAt(0);
            const std::string newText = positionalAt(1);
            if (controlFile.empty() || newText.empty()) {
                std::fprintf(stderr, "probe: refresh 需要控制文件与新文本\n");
                return 2;
            }
            const int caret = std::atoi(valueFromArgs("--caret=", "1").c_str());
            return caseRefresh(controlFile, newText, caret, timeoutFromArgs(2000));
        }
        std::fprintf(stderr, "probe: 未知模式 %s\n", mode.c_str());
        return 2;
    } catch (const std::exception &exc) {
        std::fprintf(stderr, "probe: 异常 %s\n", exc.what());
        return 1;
    }
}
