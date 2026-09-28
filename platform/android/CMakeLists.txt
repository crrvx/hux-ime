# SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
# SPDX-License-Identifier: GPL-3.0-or-later

# Android 落点：为 fcitx5-android 插件编出**同一个** addon（共用层在 `../fcitx5`），
# 并落到插件 APK 的布局。源码、C ABI 与 conf 都不在这里，这里只决定「为 Android 怎么编、落到哪」。
#
# 由 fork `fcitx5-android` 的插件工程引入（`plugin/hux/src/main/cpp/CMakeLists.txt` 里
# `add_subdirectory(<hux-ime>/platform/android …)`），Gradle 的 externalNativeBuild 走那里；
# 上游集成事实、里程碑与真机验收见 `platform/android/README.md`。
# 本仓库的 CI 只构建 Linux 落点，不在这里交叉编译——本文件保证的是「两端同一份源码、两种装配」。
cmake_minimum_required(VERSION 3.20)
project(hux-platform-android VERSION 0.1.0 LANGUAGES CXX)

if(NOT ANDROID)
    message(WARNING
        "platform/android 是 Android 落点：请用 NDK 工具链配置（ANDROID_ABI / ANDROID_PLATFORM）；"
        "宿主编译请用 platform/linux")
endif()

set(HUX_WORKSPACE "${CMAKE_CURRENT_SOURCE_DIR}/../..")
set(HUX_FCITX5_DIR "${HUX_WORKSPACE}/platform/fcitx5")

# ABI：插件只出 arm64-v8a（见 `platform/android/README.md` 的 ABI 决策与真机验收）。
# 未给 ANDROID_ABI 时也按唯一支持的 ABI 走，避免静默编出别的架构。
if(DEFINED ANDROID_ABI AND NOT ANDROID_ABI STREQUAL "" AND NOT ANDROID_ABI STREQUAL "arm64-v8a")
    message(FATAL_ERROR "本插件只出 arm64-v8a（当前 ANDROID_ABI=${ANDROID_ABI}）")
endif()

# 平台侧只补这几项（共用层的缺省是桌面）：平台专有的 C++ 源、cargo 的目标三元组、要链的系统库。
# `staticlib` 免链接器配置，故交叉编译只需 rustup 目标，无需额外 linker/AR 设置。
set(HUX_PLATFORM_SOURCES "${CMAKE_CURRENT_SOURCE_DIR}/shell/open_directory.cpp"
    CACHE STRING "落点专有的 C++ 源（Android：打开目录没有对应物，只回 false）")
set(HUX_CARGO_TARGET "aarch64-linux-android" CACHE STRING "cargo --target 的目标三元组")
set(HUX_ADDON_LIBS "-llog -ldl -lm -lunwind" CACHE STRING "addon 链接的系统库（带 -l 前缀；平台侧决定）")

add_subdirectory("${HUX_FCITX5_DIR}" fcitx5)

# 插件 APK 的 addon 布局：与桌面同一套目录（`usr/lib/fcitx5`、`usr/share/fcitx5/…`），
# 差别只在这个前缀是 APK 的 assets——运行时由 fcitx5-android 指向它
# （`XDG_DATA_DIRS` / `FCITX_ADDON_DIRS`，见 `platform/android/README.md`）。
# 组件名是上游插件的打包约定：`config`（插件自身）与 `prebuilt-assets`（随包数据）。
install(TARGETS hux LIBRARY DESTINATION /usr/lib/fcitx5 COMPONENT config)
install(FILES "${HUX_FCITX5_DIR}/conf/hux.addon.conf"
        DESTINATION /usr/share/fcitx5/addon
        RENAME hux.conf COMPONENT config)
install(FILES "${HUX_FCITX5_DIR}/conf/hux.inputmethod.conf"
        DESTINATION /usr/share/fcitx5/inputmethod
        RENAME hux.conf COMPONENT config)

# 随包数据：与 Linux 落点同源——清单 `data/MANIFEST` 是**单一来源**（`tools/checks/check_data_manifest.sh`
# 守护各落点一致）。`ENCODING UTF-8` 的理由见 `platform/linux/CMakeLists.txt`。
file(STRINGS "${HUX_WORKSPACE}/data/MANIFEST" HUX_DATA_ENTRIES ENCODING UTF-8)
set(HUX_DATA_FILES "")
foreach(HUX_DATA_ENTRY IN LISTS HUX_DATA_ENTRIES)
    string(STRIP "${HUX_DATA_ENTRY}" HUX_DATA_ENTRY)
    if(HUX_DATA_ENTRY STREQUAL "" OR HUX_DATA_ENTRY MATCHES "^#")
        continue()
    endif()
    if(NOT EXISTS "${HUX_WORKSPACE}/${HUX_DATA_ENTRY}")
        message(FATAL_ERROR "data/MANIFEST 列出的文件不存在：${HUX_DATA_ENTRY}")
    endif()
    list(APPEND HUX_DATA_FILES "${HUX_WORKSPACE}/${HUX_DATA_ENTRY}")
endforeach()
if(NOT HUX_DATA_FILES)
    message(FATAL_ERROR "data/MANIFEST 没有有效行（随包数据清单缺失或为空）")
endif()
install(FILES ${HUX_DATA_FILES} DESTINATION /usr/share/fcitx5/hux COMPONENT prebuilt-assets)

# 图标与主题：Android 侧不装（宿主是 fcitx5-android 自己的 UI，不用 freedesktop 图标主题，
# 也不用 fcitx5 桌面主题）。同一批资源在桌面的安装形态见 `platform/linux/CMakeLists.txt`。
