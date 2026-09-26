# SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
# SPDX-License-Identifier: GPL-3.0-or-later

cmake_minimum_required(VERSION 3.20)
project(hux-platform-fcitx5 VERSION 0.1.0 LANGUAGES CXX)

set(CMAKE_CXX_STANDARD 20)
set(CMAKE_CXX_STANDARD_REQUIRED ON)

# fcitx5 下限 **5.1.15**：配置项用「指定初始化」构造 `Option(OptionParameters)`（该构造 5.1.15 才有），
# 候选注释 `CandidateWord::setComment` 需 ≥5.1.9。CI 在 **ubuntu-26.04** 上按 apt 提供的版本构建
# （26.04 当前给 5.1.19，不钉具体版本）；低于下限时在这里直接报错，而不是在 C++ 模板深处炸开。
find_package(Fcitx5Core 5.1.15 REQUIRED)

# AT-SPI 取字来源（可选）：客户端不上报周边文本时（终端等）从无障碍总线取焦点文本。
# 开发包缺失（CI 的最小镜像只装 libfcitx5core-dev / librime-dev）时自动关闭，仅退回
# 「客户端上报」来源，其余功能不受影响 —— 故 `AUTO` 是缺省，`OFF` 供最小构建显式关掉。
set(HUX_ATSPI "AUTO" CACHE STRING "AT-SPI 取字来源：AUTO（缺开发包即关闭）/ ON（缺即报错）/ OFF")
set_property(CACHE HUX_ATSPI PROPERTY STRINGS AUTO ON OFF)
if(NOT HUX_ATSPI MATCHES "^(AUTO|ON|OFF)$")
    message(FATAL_ERROR "HUX_ATSPI 只能是 AUTO / ON / OFF（当前：${HUX_ATSPI}）")
endif()

if(NOT HUX_ATSPI STREQUAL "OFF")
    # `pkg_check_modules` 只认 REQUIRED / QUIET / IMPORTED_TARGET 等关键字：**没有** STATIC /
    # MODULE 模式（见本机 cmake 4.4.3 的 `Modules/FindPkgConfig.cmake:1160`，
    # `_pkgconfig_parse_options` 会把不认识的关键字当成**包名** ⇒ `pkg-config --exists
    # atspi-2 MODULE` 失败、静默探测不到）。当前 cmake 取的是非 `--static` 的 flags
    # （实测 `-latspi -ldbus-1 -lglib-2.0`），正是这里要的「头文件 + 动态库」。
    find_package(PkgConfig QUIET)
    if(PkgConfig_FOUND)
        pkg_check_modules(ATSPI QUIET IMPORTED_TARGET atspi-2)
    endif()
    if(TARGET PkgConfig::ATSPI)
        set(HUX_HAVE_ATSPI ON)
    elseif(HUX_ATSPI STREQUAL "ON")
        message(FATAL_ERROR
            "HUX_ATSPI=ON 但未找到 atspi-2 开发包（Debian/Ubuntu 装 libatspi2.0-dev；"
            "Fedora 装 at-spi2-core-devel），或用 -DHUX_ATSPI=OFF 显式关闭取字来源")
    else()
        set(HUX_HAVE_ATSPI OFF)
        message(STATUS "未找到 atspi-2 开发包：AT-SPI 取字来源关闭（客户端不上报周边文本时无字反查）")
    endif()
endif()

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

add_library(hux SHARED shell/hux.cpp shell/atspi_source.cpp)
add_dependencies(hux hux-rust)
target_include_directories(hux PRIVATE
    "${CMAKE_CURRENT_SOURCE_DIR}/shell"
    "${HUX_WORKSPACE}/crates/hux-ffi/include")
target_link_libraries(hux PRIVATE Fcitx5::Core "${HUX_RUST_LIB}" dl pthread m)
if(HUX_HAVE_ATSPI)
    # 两个 TU 无条件进构建：关掉 AT-SPI 时 `atspi_source.cpp` 编成 stub（空实现），
    # 这样 `hux.cpp` 的接线不需要 `#ifdef`，也就不存在「开着能编、关掉编不过」的分叉。
    target_link_libraries(hux PRIVATE PkgConfig::ATSPI)
    target_compile_definitions(hux PRIVATE HUX_HAVE_ATSPI=1)
endif()
set_target_properties(hux PROPERTIES OUTPUT_NAME "hux")

# 插件安装目录：优先跟随 fcitx5 自身的 addon 目录（Fcitx5Utils 提供；Fedora 为 lib64/fcitx5），
# 未提供时退回 GNUInstallDirs 的 libdir。
#
# `HUX_RELATIVE_ADDON_DIR=ON` 把它改成**相对**路径 `lib/fcitx5`：`FCITX_INSTALL_ADDONDIR` 是
# 绝对路径，`cmake --install --prefix <用户前缀>` 无法把它重定位到 `~/.local/lib/fcitx5`；
# 相对目标按 `CMAKE_INSTALL_PREFIX` 解析，供用户级安装（`install.sh -u`）使用。缺省仍为绝对路径。
option(HUX_RELATIVE_ADDON_DIR "插件安装目录用相对路径 lib/fcitx5（供用户级前缀重定位）" OFF)
if(HUX_RELATIVE_ADDON_DIR)
    set(HUX_ADDON_DIR "lib/fcitx5")
elseif(DEFINED FCITX_INSTALL_ADDONDIR AND NOT FCITX_INSTALL_ADDONDIR STREQUAL "")
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

# 输入法条目 / 状态区图标：装进 freedesktop 图标主题目录，`Icon=hux` 按主题名解析。
# 图形是**多平台共享**资源，源在 `${HUX_WORKSPACE}/assets/branding/`；此前指向 `fcitx-tiger`
# （由 fcitx5-chinese-addons 提供）——未装该包时托盘与输入法列表显示缺图占位。
# `hux.svg` 供支持 SVG 的主题（自包含：内嵌主源位图）；22/48 px 的 PNG 是位图主题（或缩放器
# 不可用）时的回退。四者由主源 `hux.png` 派生（生成命令与校验见 `assets/branding/README.md`、
# `tools/checks/check_branding_assets.py`）。
install(FILES "${HUX_WORKSPACE}/assets/branding/hux.svg"
        DESTINATION share/icons/hicolor/scalable/apps)
install(FILES "${HUX_WORKSPACE}/assets/branding/hux-48.png"
        DESTINATION share/icons/hicolor/48x48/apps
        RENAME hux.png)
install(FILES "${HUX_WORKSPACE}/assets/branding/hux-22.png"
        DESTINATION share/icons/hicolor/22x22/apps
        RENAME hux.png)

# 随包数据（码表四件套 + 追加码表 `tiger_sentence.codes.<name>.txt` + 词先验 + 拼音索引 + 标点表）：
# 清单 `data/MANIFEST` 是**单一来源**——
# `install.sh` 按它核对落盘、`uninstall.sh` 按它删除（`tools/checks/check_data_manifest.sh` 守护三者一致）。
#
# 此前 CMake 只装 `libhux.so` 与两个 conf，只走 `cmake --install`
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

# 共享主题（fcitx5 主题形态）：清单 `assets/themes/MANIFEST` 是**单一来源**——
# `install.sh` 按它核对落盘、`uninstall.sh` 按它删除、`tools/checks/check_themes.py` 守护四者一致。
# 主题是跨平台共享资源（配色与图形），这里是它在 Linux 上的安装形态。
file(STRINGS "${HUX_WORKSPACE}/assets/themes/MANIFEST" HUX_THEME_ENTRIES ENCODING UTF-8)
set(HUX_THEME_DIRS "")
foreach(HUX_THEME_ENTRY IN LISTS HUX_THEME_ENTRIES)
    string(STRIP "${HUX_THEME_ENTRY}" HUX_THEME_ENTRY)
    if(HUX_THEME_ENTRY STREQUAL "" OR HUX_THEME_ENTRY MATCHES "^#")
        continue()
    endif()
    if(NOT IS_DIRECTORY "${HUX_WORKSPACE}/assets/themes/${HUX_THEME_ENTRY}")
        message(FATAL_ERROR "assets/themes/MANIFEST 列出的主题目录不存在：${HUX_THEME_ENTRY}")
    endif()
    list(APPEND HUX_THEME_DIRS "${HUX_WORKSPACE}/assets/themes/${HUX_THEME_ENTRY}")
endforeach()
if(NOT HUX_THEME_DIRS)
    message(FATAL_ERROR "assets/themes/MANIFEST 没有有效行（主题清单缺失或为空）")
endif()
install(DIRECTORY ${HUX_THEME_DIRS} DESTINATION share/fcitx5/themes)
