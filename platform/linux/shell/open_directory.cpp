// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

/*
 * Linux 落点的宿主能力：用桌面文件管理器打开目录（`xdg-open`，exec 失败再退 `gio open`）。
 */
#include <sys/wait.h>
#include <unistd.h>

#include <cerrno>
#include <string>

#include "platform.h"

namespace hux::platform {

/// 拉起文件管理器打开 `directory`：**双 fork + `execlp`**（先 `xdg-open`，exec 失败再
/// `gio open`）。
///
/// 为什么不是 `std::system`：它经 `/bin/sh -c` 解释整串，目录名里的空格 / 元字符会变成
/// 命令注入（模型路径来自环境变量与配置，不是可信输入）；`execlp` 逐个参数传，不经 shell。
/// 为什么双 fork：文件管理器可能活很久，父进程不能等它——中间进程 fork 完立刻 `_exit`，
/// 孙进程被 init 收尸，故**没有任何僵尸**；中间进程本身必须收一下（它才是父进程的孩子），
/// 而它 fork 后立即退出，这个 wait 不会有可感阻塞。
///
/// 返回 `false` = 连 fork 都没成功（调用方只记日志）。
bool openDirectory(const std::string &directory) {
    const pid_t child = fork();
    if (child < 0) {
        return false;
    }
    if (child == 0) {
        const pid_t grandchild = fork();
        if (grandchild < 0) {
            _exit(1);
        }
        if (grandchild > 0) {
            _exit(0); // 中间进程：孙进程已脱离父进程，这里立刻退出
        }
        execlp("xdg-open", "xdg-open", directory.c_str(),
               static_cast<char *>(nullptr));
        execlp("gio", "gio", "open", directory.c_str(),
               static_cast<char *>(nullptr));
        _exit(1);
    }
    // 只等中间进程（毫秒级；不去等孙进程里的文件管理器）。
    int status = 0;
    while (waitpid(child, &status, 0) < 0 && errno == EINTR) {
    }
    return true;
}

} // namespace hux::platform
