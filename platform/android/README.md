<!-- SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com> -->
<!-- SPDX-License-Identifier: GPL-3.0-or-later -->

# Android：fcitx5-android 插件（待启动）

本文是 Android 平台的**计划与事实**（活文档，只写现状与做法）；状态见
[`../README.md`](../README.md)「状态总览」，里程碑从 M0 起。桌面共用的 fcitx5 契约（按键 / 候选 /
提交语义、反查、安装落点）见 [`../README.md`](../README.md)「fcitx5 addon 行为契约」与「安装落点」。

## 目标

目标：**fcitx5-android 插件 APK**（主程序 + 插件 + 独立模型 APK），决策见下表。

| 项 | 决策 |
| --- | --- |
| 仓库 | fork `fcitx5-android`，新增 `plugin/hux`，<br>以 git submodule 引本仓库（`hux-ime`） |
| 模型 | 单独「模型插件」APK<br>（仅 assets 携带 <br>`usr/share/fcitx5/hux/models/sentence-ngram-mobile.bin`） |
| ABI | 仅 `arm64-v8a`（Rust 目标 `aarch64-linux-android`） |
| 分发 | GitHub Releases |
| 构建 | 本地 Gradle 为主；CI 待定 |

## 与 tiger / fcitx5 的差异

- **插件 = 独立 APK**：包名 `org.fcitx.fcitx5.android.plugin.<name>[.debug]`， \
  `${appId}.plugin.MANIFEST` intent + `res/xml/plugin.xml`（`apiVersion 0.1`）； \
  主程序合并其 `assets/`（`DataManager` 复制进应用数据目录、`descriptor.json` 差量更新）。
- **与桌面共用装配层**：`platform/fcitx5/` 的 C++ 薄壳 + Rust 组装直接复用， \
  fcitx5 addon 契约（选项角色、按键 / 提交语义、反查）不变； \
  差异只在数据目录来源与 `__ANDROID__` 分支。
- **本仓库（hux-ime）改动**：
  - 平台层数据目录查找支持 **`XDG_DATA_DIRS`**（落 `fcitx5/src/paths.rs`，内核不读环境变量）， \
    桌面行为不变；顺序见 [`../../docs/reference.md`](../../docs/reference.md) §2。
  - `__ANDROID__` 差异（配置 schema、状态区子菜单）待验收决定； \
    `install.md` 的 Android 安装 / 模型与 `REUSE` 头**待补**。

## 数据与 API 需求

- **addon 布局**（同构 jyutping）：`usr/lib/fcitx5/libhux.so`、`usr/share/fcitx5/addon/hux.conf`、 \
  `usr/share/fcitx5/inputmethod/hux.conf`（`COMPONENT config`）、 \
  `usr/share/fcitx5/hux/…`（`COMPONENT prebuilt-assets`， \
  `fcitxComponent { installPrebuiltAssets = true }`）。
- **运行时环境**（`native-lib.cpp`，先于 fcitx5 设置）： \
  `XDG_DATA_HOME=<外部 files>/data`（可写：选项 / 学习库 / 模型）、 \
  `XDG_DATA_DIRS=<appData>/usr/share`（插件数据安装位置）、`FCITX_ADDON_DIRS` 由核心处理。
- **API**：候选点击走 `CandidateWord::select()`（`androidfrontend.cpp`）；Rust 侧出 staticlib \
  供 `libhux.so` 链接（见下节）；数据落点验收见「验收（真机）」⑤。

## 依赖与构建计划

- **构建环境**：NDK `28.0.13004108`、CMake `3.31.6`、AGP；插件模块用五个约定插件 \
  （app / plugin-app / native-app / data-descriptor / fcitx-component）。
- **fork 侧工作（`plugin/hux`，未开始）**：
  - `settings.gradle.kts` 加 `include(":plugin:hux")`；`.gitmodules` 加 `hux-ime` 子模块； \
    `plugin/hux/build.gradle.kts`：五个约定插件、`packaging.jniLibs.excludes`（`libc++_shared`、 \
    `libFcitx5*` 等）；`AndroidManifest.xml`、`res/xml/plugin.xml`（domain `fcitx5-hux`）、 \
    图标与文案、`plugin_resources_keep.xml`。
  - `src/main/cpp/CMakeLists.txt`：`find_package(fcitx5 CONFIG)` + \
    `find_package(Fcitx5Core MODULE)`；Rust `ANDROID_ABI=arm64-v8a → aarch64-linux-android` 的 \
    `cargo build --target … --release`（staticlib 免链接器配置）； \
    `add_library(hux SHARED <hux-ime>/platform/fcitx5/shell/hux.cpp)` 链接 \
    `libhux_platform_fcitx5.a`、`Fcitx5::Core`（按需 `log dl m unwind`）； \
    安装 `install(TARGETS hux LIBRARY DESTINATION /usr/lib/fcitx5 COMPONENT config)`、 \
    `install(FILES conf/* … COMPONENT config)`、 \
    `install(DIRECTORY data/ … COMPONENT prebuilt-assets)`（排除 `README.md`）。
  - 模型插件模块：`assets/usr/share/fcitx5/hux/models/sentence-ngram-mobile.bin` + `plugin.xml`； \
    构建 `./gradlew :plugin:hux:assembleRelease`（需 Android SDK/NDK、 \
    `rustup target add aarch64-linux-android`）。
- **风险与备选**：
  - **Rust × AGP**：CMake 内调 cargo 不稳 → 先脚本 cargo 构建，CMake 只链接。
  - **配置页渲染**：`List|Key` 与嵌套子配置受支持（对照 Android `ConfigType`）； \
    异常 → Android 分支扁平 schema。**状态区**：`SimpleAction`+`Menu` 子菜单 \
    不被渲染 → 平铺 5 个开关。
  - **模型体积**（~224 MB）：GitHub Releases 直发，F-Droid/Play 暂不做； \
    **上游收编**先 fork 自用，视情况再提 PR（其 CI 是否接受 Rust 构建待议）。

## 验收（真机）

① 装好主程序 + 插件 → 输入法列表出现「虎虚」；② 打字出候选、点击上屏、翻页、数字直选； \
③ 音反查 / 字反查（软键盘触发键可另配；硬件键盘默认 `` ` `` / `~`）； \
④ 配置页「行为/快捷键」可读写并即时生效；⑤ 选项 / 学习库落在 \
`Android/data/<pkg>/files/data/fcitx5/hux/`；⑥ 装模型 APK 后整句质量提升， \
logcat 可见 `hux: dirs… model…`。

## 里程碑

| 阶段 | 内容 | 预估 |
| --- | --- | --- |
| M0 | 骨架可加载（插件 APK → 虎虚出现、能打字） | 0.5–1 天 |
| M1 | 功能闭环（数据/选项/学习/配置页/状态区） | 1–2 天 |
| M2 | 模型 APK + 文档 | 0.5–1 天 |
| M3 | 发布（GitHub Releases + 使用说明） | 0.5 天 |
