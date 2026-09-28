# SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
# SPDX-License-Identifier: GPL-3.0-or-later

# 共用 addon 层：Rust 组装（staticlib）+ C++ 薄壳 → `libhux.so`。
# 这里只定义**怎么编**，不定义**装到哪**——安装规则属于落点：
#   · Linux：`platform/linux/CMakeLists.txt`（发行版布局，构建入口）
#   · Android：`platform/android/CMakeLists.txt`（插件 APK 布局）
# 两个落点都以 `add_subdirectory` 引入本目录，平台差异只用下面两个缓存变量表达。
cmake_minimum_required(VERSION 3.20)
project(hux-platform-fcitx5 VERSION 0.1.0 LANGUAGES CXX)

set(CMAKE_CXX_STANDARD 20)
set(CMAKE_CXX_STANDARD_REQUIRED ON)

# 没有落点父工程时（单独 configure）只剩「编出 libhux.so、什么都不装」：明说，别让随后的
# `cmake --install` 静默装出空布局。
if(CMAKE_SOURCE_DIR STREQUAL CMAKE_CURRENT_SOURCE_DIR)
    message(FATAL_ERROR
        "platform/fcitx5 是共用层，不能单独安装；Linux 用 `cmake -S platform/linux -B build/addon`，"
        "Android 见 platform/android/README.md")
endif()

# fcitx5 下限 **5.1.15**：配置项用「指定初始化」构造 `Option(OptionParameters)`（该构造 5.1.15 才有），
# 候选注释 `CandidateWord::setComment` 需 ≥5.1.9。CI 在 **ubuntu-26.04** 上按 apt 提供的版本构建
# （26.04 当前给 5.1.19，不钉具体版本）；低于下限时在这里直接报错，而不是在 C++ 模板深处炸开。
# 落点已先找过包（Android 侧由 fork 的工程找到）时复用其结果，不重复找。
if(NOT TARGET Fcitx5::Core)
    find_package(Fcitx5Core 5.1.15 REQUIRED)
endif()
# 落点要在**自己的**作用域里用它决定插件安装目录（Fedora：lib64/fcitx5），故显式上传；
# 包配置没给这个变量时上传空值，落点照旧退回 GNUInstallDirs 的 libdir。
set(FCITX_INSTALL_ADDONDIR "${FCITX_INSTALL_ADDONDIR}" PARENT_SCOPE)

# 平台侧只补这几项，其余全在共用层：
#   · HUX_PLATFORM_SOURCES：落点专有的 C++ 源（实现 `shell/platform.h` 的宿主能力——桌面上的
#     `fork`/`execlp` 是平台事实）。落点必须给：空着就是没接上，下面直接报错。
#   · HUX_CARGO_TARGET：cargo 的目标三元组；空 = 宿主（桌面），Android 落点传
#     `aarch64-linux-android`（staticlib 免链接器配置，见 platform/android/README.md）。
#   · HUX_ADDON_LIBS：addon 链接的系统库；桌面 `-ldl -lpthread -lm`，Android `-llog -ldl -lm -lunwind`。
#     写 `-l` 形式：经变量传进来的裸库名 CMake 不会补 `-l`（只有字面量才会），会变成让
#     链接器去找名为 `pthread` 的文件。
set(HUX_PLATFORM_SOURCES "" CACHE STRING "落点专有的 C++ 源（落点必须设置，见 shell/platform.h）")
set(HUX_CARGO_TARGET "" CACHE STRING "cargo --target 的目标三元组（空 = 宿主）")
set(HUX_ADDON_LIBS "-ldl -lpthread -lm" CACHE STRING "addon 链接的系统库（带 -l 前缀；平台侧决定）")

if(NOT HUX_PLATFORM_SOURCES)
    message(FATAL_ERROR
        "落点没有设置 HUX_PLATFORM_SOURCES：平台专有的 C++ 源随落点走"
        "（见 platform/linux/CMakeLists.txt 与 platform/android/CMakeLists.txt）")
endif()

# Rust 静态库（逻辑层；cargo 自身增量，故每次构建都调用）。
if(CMAKE_BUILD_TYPE STREQUAL "Debug")
    set(HUX_CARGO_PROFILE "debug")
    set(HUX_CARGO_FLAGS "")
    set(HUX_CARGO_LABEL "debug")
else()
    set(HUX_CARGO_PROFILE "release")
    set(HUX_CARGO_FLAGS "--release")
    set(HUX_CARGO_LABEL "release")
endif()
set(HUX_WORKSPACE "${CMAKE_CURRENT_SOURCE_DIR}/../..")
if(HUX_CARGO_TARGET)
    set(HUX_CARGO_TARGET_FLAG --target "${HUX_CARGO_TARGET}")
    set(HUX_CARGO_LABEL "${HUX_CARGO_LABEL}, ${HUX_CARGO_TARGET}")
    set(HUX_CARGO_TARGET_DIR "${HUX_WORKSPACE}/target/${HUX_CARGO_TARGET}")
else()
    set(HUX_CARGO_TARGET_FLAG "")
    set(HUX_CARGO_TARGET_DIR "${HUX_WORKSPACE}/target")
endif()
set(HUX_RUST_LIB "${HUX_CARGO_TARGET_DIR}/${HUX_CARGO_PROFILE}/libhux_platform_fcitx5.a")
add_custom_target(hux-rust ALL
    COMMAND cargo build -p hux-platform-fcitx5 ${HUX_CARGO_TARGET_FLAG} ${HUX_CARGO_FLAGS}
    WORKING_DIRECTORY "${HUX_WORKSPACE}"
    COMMENT "cargo build -p hux-platform-fcitx5 (${HUX_CARGO_LABEL})"
    VERBATIM)

add_library(hux SHARED shell/hux.cpp ${HUX_PLATFORM_SOURCES})
add_dependencies(hux hux-rust)
target_include_directories(hux PRIVATE
    "${CMAKE_CURRENT_SOURCE_DIR}/shell"
    "${HUX_WORKSPACE}/crates/hux-ffi/include")
target_link_libraries(hux PRIVATE Fcitx5::Core "${HUX_RUST_LIB}" ${HUX_ADDON_LIBS})
set_target_properties(hux PROPERTIES OUTPUT_NAME "hux")
