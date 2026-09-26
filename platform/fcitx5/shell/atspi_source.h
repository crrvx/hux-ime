// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

#pragma once

#include <cstdint>
#include <memory>
#include <optional>
#include <string>

namespace hux {

/// 字反查的 AT-SPI 取字来源：客户端不上报周边文本时，从无障碍总线取焦点对象的文本与光标。
///
/// 线程契约：`snapshot()` / `requestRefresh()` / `invalidate()` 可从**任意线程**调用且**不阻塞**
/// —— 按键路径只加锁读缓存，一切 libatspi 调用都关在后台线程里（D-Bus 往返最坏可达数十毫秒，
/// 不能在输入循环里做）。未带 AT-SPI 的构建里整类退化为无操作（`compiled()` 为假）。
class AtspiSource {
public:
    /// 一次取字结果。
    struct Snapshot {
        std::string text;      ///< 光标左侧窗口（≤ 64 个字符，UTF-8）
        uint32_t cursorChars;  ///< 光标在本串中的字符位置（= text 的字符数）
    };

    AtspiSource();
    ~AtspiSource();
    AtspiSource(const AtspiSource &) = delete;
    AtspiSource &operator=(const AtspiSource &) = delete;

    /// 按键路径：读缓存快照；无来源 / 未就绪返回 `std::nullopt`。
    std::optional<Snapshot> snapshot() const;
    /// 请求刷新：**惰性启动的唯一触发点**（首次调用才起后台线程），并把活跃窗往后推。
    /// 客户端不上报周边文本时由按键路径调用；只读 `snapshot()` 而从不调用本函数，
    /// 缓存会永远是空的（后台线程根本不会起来）。合并排队，不阻塞。
    void requestRefresh();
    /// 作废（输入上下文切换 / 重置）：避免把上一个应用的文本当本应用的。
    void invalidate();
    /// 本构建是否带 AT-SPI（未定义 `HUX_HAVE_ATSPI` 时为 stub，全部退化）。
    static bool compiled();

private:
    class Impl;
    std::unique_ptr<Impl> impl_;
};

} // namespace hux
