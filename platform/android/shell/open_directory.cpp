// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

/*
 * Android 落点的宿主能力：**没有**「用文件管理器打开目录」这回事。
 *
 * 模型目录在应用私有数据里（宿主注入的 `$XDG_DATA_HOME`），由 fcitx5-android 自己的界面管；
 * 从 addon 里 fork/exec 出去也没有意义（Android 的 exec 语义与桌面不同）。故这里只回
 * `false`：调用方记一条日志，其它行为不变。
 */
#include <string>

#include "platform.h"

namespace hux::platform {

bool openDirectory(const std::string & /*directory*/) {
    return false;
}

} // namespace hux::platform
