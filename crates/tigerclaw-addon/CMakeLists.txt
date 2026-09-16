cmake_minimum_required(VERSION 3.20)
project(tigerclaw-addon VERSION 0.1.0 LANGUAGES CXX)

set(CMAKE_CXX_STANDARD 20)
set(CMAKE_CXX_STANDARD_REQUIRED ON)

find_package(Fcitx5Core REQUIRED)

# Rust 静态库（逻辑层；cargo 自身增量，故每次构建都调用）。
if(CMAKE_BUILD_TYPE STREQUAL "Debug")
    set(TIGERCLAW_CARGO_PROFILE "debug")
    set(TIGERCLAW_CARGO_FLAGS "")
else()
    set(TIGERCLAW_CARGO_PROFILE "release")
    set(TIGERCLAW_CARGO_FLAGS "--release")
endif()
set(TIGERCLAW_WORKSPACE "${CMAKE_CURRENT_SOURCE_DIR}/../..")
set(TIGERCLAW_RUST_LIB
    "${TIGERCLAW_WORKSPACE}/target/${TIGERCLAW_CARGO_PROFILE}/libtigerclaw_addon.a")
add_custom_target(tigerclaw-rust ALL
    COMMAND cargo build -p tigerclaw-addon ${TIGERCLAW_CARGO_FLAGS}
    WORKING_DIRECTORY "${TIGERCLAW_WORKSPACE}"
    COMMENT "cargo build -p tigerclaw-addon (${TIGERCLAW_CARGO_PROFILE})"
    VERBATIM)

add_library(tigerclaw SHARED shell/tigerclaw.cpp)
add_dependencies(tigerclaw tigerclaw-rust)
target_include_directories(tigerclaw PRIVATE "${CMAKE_CURRENT_SOURCE_DIR}/shell")
target_link_libraries(tigerclaw PRIVATE Fcitx5::Core "${TIGERCLAW_RUST_LIB}" dl pthread m)
set_target_properties(tigerclaw PROPERTIES OUTPUT_NAME "tigerclaw")

install(TARGETS tigerclaw LIBRARY DESTINATION lib/fcitx5)
install(FILES conf/tigerclaw.addon.conf
        DESTINATION share/fcitx5/addon
        RENAME tigerclaw.conf)
install(FILES conf/tigerclaw.inputmethod.conf
        DESTINATION share/fcitx5/inputmethod
        RENAME tigerclaw.conf)
