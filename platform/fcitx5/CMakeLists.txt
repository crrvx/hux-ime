# SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
# SPDX-License-Identifier: GPL-3.0-or-later

cmake_minimum_required(VERSION 3.20)
project(hux-platform-fcitx5 VERSION 0.1.0 LANGUAGES CXX)

set(CMAKE_CXX_STANDARD 20)
set(CMAKE_CXX_STANDARD_REQUIRED ON)

find_package(Fcitx5Core REQUIRED)

# Rust 静态库（逻辑层；cargo 自身增量，故每次构建都调用）。
if(CMAKE_BUILD_TYPE STREQUAL "Debug")
    set(HUX_CARGO_PROFILE "debug")
    set(HUX_CARGO_FLAGS "")
else()
    set(HUX_CARGO_PROFILE "release")
    set(HUX_CARGO_FLAGS "--release")
endif()
set(HUX_WORKSPACE "${CMAKE_CURRENT_SOURCE_DIR}/../..")
set(HUX_RUST_LIB
    "${HUX_WORKSPACE}/target/${HUX_CARGO_PROFILE}/libhux_platform_fcitx5.a")
add_custom_target(hux-rust ALL
    COMMAND cargo build -p hux-platform-fcitx5 ${HUX_CARGO_FLAGS}
    WORKING_DIRECTORY "${HUX_WORKSPACE}"
    COMMENT "cargo build -p hux-platform-fcitx5 (${HUX_CARGO_PROFILE})"
    VERBATIM)

add_library(hux SHARED shell/hux.cpp)
add_dependencies(hux hux-rust)
target_include_directories(hux PRIVATE
    "${CMAKE_CURRENT_SOURCE_DIR}/shell"
    "${HUX_WORKSPACE}/crates/hux-ffi/include")
target_link_libraries(hux PRIVATE Fcitx5::Core "${HUX_RUST_LIB}" dl pthread m)
set_target_properties(hux PROPERTIES OUTPUT_NAME "hux")

# 插件安装目录：优先跟随 fcitx5 自身的 addon 目录（Fcitx5Utils 提供；Fedora 为 lib64/fcitx5），
# 未提供时退回 GNUInstallDirs 的 libdir。
if(DEFINED FCITX_INSTALL_ADDONDIR AND NOT FCITX_INSTALL_ADDONDIR STREQUAL "")
    set(HUX_ADDON_DIR "${FCITX_INSTALL_ADDONDIR}")
else()
    include(GNUInstallDirs)
    set(HUX_ADDON_DIR "${CMAKE_INSTALL_LIBDIR}/fcitx5")
endif()
install(TARGETS hux LIBRARY DESTINATION "${HUX_ADDON_DIR}")
install(FILES conf/hux.addon.conf
        DESTINATION share/fcitx5/addon
        RENAME hux.conf)
install(FILES conf/hux.inputmethod.conf
        DESTINATION share/fcitx5/inputmethod
        RENAME hux.conf)

# 随包数据（码表四件套 + 词先验 + 拼音索引 + 标点表）：清单 `data/MANIFEST` 是**单一来源**——
# `install.sh` 按它核对落盘、`uninstall.sh` 按它删除（`tools/checks/check_data_manifest.sh` 守护三者一致）。
#
# 复核整改第 4 批 F5：此前 CMake 只装 `libhux.so` 与两个 conf，只走 `cmake --install`
# （发行版打包、`DESTDIR` 流程、手工安装）会得到「无词库」引擎——`Lexicon` / `PunctTable`
# 静默降级，打字无输出/无标点。这里按清单安装；**n-gram 模型仍不随包**（用户自取）。
# `ENCODING UTF-8` 显式给出：CMake 默认编码会破坏非 ASCII 字节（把中文注释行拆成假文件名）；
# 清单本身按约定保持纯 ASCII（见 `data/README.md`），该选项只是把意图钉住。
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
install(FILES ${HUX_DATA_FILES} DESTINATION share/fcitx5/hux)
