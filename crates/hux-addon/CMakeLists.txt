# SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
# SPDX-License-Identifier: GPL-3.0-or-later

cmake_minimum_required(VERSION 3.20)
project(hux-addon VERSION 0.1.0 LANGUAGES CXX)

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
    "${HUX_WORKSPACE}/target/${HUX_CARGO_PROFILE}/libhux_addon.a")
add_custom_target(hux-rust ALL
    COMMAND cargo build -p hux-addon ${HUX_CARGO_FLAGS}
    WORKING_DIRECTORY "${HUX_WORKSPACE}"
    COMMENT "cargo build -p hux-addon (${HUX_CARGO_PROFILE})"
    VERBATIM)

add_library(hux SHARED shell/hux.cpp
    shell/hux_abi.h)
add_dependencies(hux hux-rust)
target_include_directories(hux PRIVATE "${CMAKE_CURRENT_SOURCE_DIR}/shell")
target_link_libraries(hux PRIVATE Fcitx5::Core "${HUX_RUST_LIB}" dl pthread m)
set_target_properties(hux PROPERTIES OUTPUT_NAME "hux")

install(TARGETS hux LIBRARY DESTINATION lib/fcitx5)
install(FILES conf/hux.addon.conf
        DESTINATION share/fcitx5/addon
        RENAME hux.conf)
install(FILES conf/hux.inputmethod.conf
        DESTINATION share/fcitx5/inputmethod
        RENAME hux.conf)
