// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

/*
 * 落点必须提供的宿主能力：与「外面那个系统」打交道的那几件事。
 *
 * 共用层（`shell/hux.cpp`）只调这里声明的函数，实现随落点走——落点的 `CMakeLists.txt` 把该
 * 平台的源文件放进 `HUX_PLATFORM_SOURCES`：
 *   · Linux：`platform/linux/shell/open_directory.cpp`（`fork` + `execlp` 交给文件管理器）
 *   · Android：`platform/android/shell/open_directory.cpp`（模型目录在应用私有数据里，宿主 UI 自己管）
 */
#ifndef HUX_PLATFORM_H_
#define HUX_PLATFORM_H_

#include <string>

namespace hux::platform {

/// 用宿主环境的方式打开目录（桌面 = 文件管理器；没这个能力的落点直接返回 `false`）。
///
/// 返回 `false` = 没拉起来，调用方只记日志、不影响其它功能；实现**不得**阻塞等待外部程序
/// 结束（文件管理器开着不走），也**不得**把目录名交给 shell 解释（路径含空格 / 元字符）。
bool openDirectory(const std::string &directory);

} // namespace hux::platform

#endif // HUX_PLATFORM_H_
