<!-- SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com> -->
<!-- SPDX-License-Identifier: GPL-3.0-or-later -->

# Android：fcitx5-android 插件（待启动）

- 本册是 Android 落点的**计划与事实**（活文档，只写现状与做法）
- 状态见 [`../README.md`](../README.md)「状态总览」；里程碑从 M0 起（见下）
- 宿主层契约（装配、按键与提交、反查）见 [`../fcitx5/README.md`](../fcitx5/README.md)，与桌面共用

## 目标

- 交付**插件 APK**：主程序 + 插件 + 独立模型 APK 三件，各项决策见下表

| 项 | 决策 |
| --- | --- |
| 仓库 | fork `fcitx5-android`，新增 `plugin/hux`，<br>以 git submodule 引本仓库（`hux-ime`） |
| 模型 | 单独「模型插件」APK（仅 assets 携带<br>`usr/share/fcitx5/hux/models/sentence-ngram-mobile.bin`） |
| ABI | 仅 `arm64-v8a`（Rust 目标 `aarch64-linux-android`） |
| 分发 | GitHub Releases |
| 构建 | 本地 Gradle 为主；CI 待定 |

## 插件形态（同构 jyutping）

- 插件是**独立 APK**：
  - 包名 `org.fcitx.fcitx5.android.plugin.<name>[.debug]`
  - 入口为 `${appId}.plugin.MANIFEST` intent 与 `res/xml/plugin.xml`（`apiVersion 0.1`）
  - 主程序合并其 `assets/`（`DataManager` 复制进应用数据目录，`descriptor.json` 差量更新）
- addon 布局四项：
  - `usr/lib/fcitx5/libhux.so`
  - `usr/share/fcitx5/addon/hux.conf`
  - `usr/share/fcitx5/inputmethod/hux.conf`（`COMPONENT config`）
  - `usr/share/fcitx5/hux/…`：`COMPONENT prebuilt-assets`，由 \
    `fcitxComponent { installPrebuiltAssets = true }` 声明
- 运行时环境在 `native-lib.cpp` 里先于 fcitx5 设置：
  - `XDG_DATA_HOME=<外部 files>/data`：可写，放选项 / 学习库 / 模型
  - `XDG_DATA_DIRS=<appData>/usr/share`：插件数据安装位置
  - `FCITX_ADDON_DIRS` 由核心处理
- 候选点击走 `CandidateWord::select()`（`androidfrontend.cpp`）
- Rust 侧出 staticlib 供 `libhux.so` 链接

## 与桌面共用的部分

- **装配层直接复用** `platform/fcitx5/` 的 C++ 薄壳 + Rust 组装：addon 契约（选项角色、 \
  按键与提交语义、反查）不变
- 两端差异只在数据目录来源与 `__ANDROID__` 分支：
  - 本仓只改一处——平台层的数据目录查找支持 **`XDG_DATA_DIRS`**，落点 `fcitx5/src/paths.rs`
  - 内核仍不读环境变量，桌面行为不变；顺序见 \
    [`../../docs/reference.md`](../../docs/reference.md) §2
- `__ANDROID__` 下的差异（配置 schema、状态区子菜单）、`install.md` 的 Android 安装 / 模型、 \
  `REUSE` 头都**待验收决定 / 待补**

## 构建计划（fork 侧 `plugin/hux`，未开始）

- 构建环境：NDK `28.0.13004108`、CMake `3.31.6` 与 AGP
- 插件模块用五个约定插件：app / plugin-app / native-app，另加 data-descriptor / fcitx-component
- 仓库接线两条：
  - `settings.gradle.kts` 加 `include(":plugin:hux")`
  - `.gitmodules` 加 `hux-ime` 子模块
- 模块脚本 `plugin/hux/build.gradle.kts` 负责：
  - 约定插件、`packaging.jniLibs.excludes`（`libc++_shared`、`libFcitx5*` 等）
  - `AndroidManifest.xml` 与 `res/xml/plugin.xml`（插件清单 domain 为 `fcitx5-hux`）
  - 图标文案与 `plugin_resources_keep.xml`
- 原生构建 `src/main/cpp/CMakeLists.txt`：
  - `find_package(fcitx5 CONFIG)` 与 `find_package(Fcitx5Core MODULE)`
  - 按 `ANDROID_ABI=arm64-v8a → aarch64-linux-android` 调 \
    `cargo build --target … --release`（staticlib，免链接器配置）
  - 构建 `add_library(hux SHARED <hux-ime>/platform/fcitx5/shell/hux.cpp)`，链接 \
    `libhux_platform_fcitx5.a` 与 `Fcitx5::Core`，按需链接 `log dl m unwind`
- 安装目标三处：
  - `install(TARGETS hux LIBRARY DESTINATION /usr/lib/fcitx5 COMPONENT config)`
  - `install(FILES conf/* … COMPONENT config)`
  - `install(DIRECTORY data/ … COMPONENT prebuilt-assets)`（排除 `README.md`）
- 模型插件模块另有 `plugin.xml`，资产为 \
  `assets/usr/share/fcitx5/hux/models/sentence-ngram-mobile.bin`，用 \
  `./gradlew :plugin:hux:assembleRelease` 构建
  - 前置是 Android SDK/NDK 与 `rustup target add aarch64-linux-android`

## 风险与备选

- **Rust × AGP**：在 CMake 内调 cargo 不稳；备选是先由脚本 cargo 构建、CMake 只负责链接
- **配置页渲染**：`List|Key` 与嵌套子配置受支持（对照 Android 的 `ConfigType`）； \
  异常时备选 Android 分支的扁平 schema
- **状态区**：`SimpleAction` + `Menu` 子菜单不被渲染；备选是平铺 5 个开关
- **模型体积**（~224 MB）：直接走 GitHub Releases，F-Droid / Play 暂不做
- **上游收编**：先 fork 自用、视情况再提 PR；待议的是其 CI 是否接受 Rust 构建

## 验收（真机）

- 装好主程序 + 插件后，输入法列表出现「虎虚」
- 打字出候选、点击上屏、翻页、数字直选
- 音反查 / 字反查可用（软键盘触发键可另配；硬件键盘默认 `` ` `` / `~`）
- 配置页「行为 / 快捷键」可读写并即时生效
- 选项与学习库落在 `Android/data/<pkg>/files/data/fcitx5/hux/`
- 装模型 APK 后整句质量提升，logcat 可见 `hux: dirs… model…`

## 里程碑

| 阶段 | 内容 | 预估 |
| --- | --- | --- |
| M0 | 骨架可加载（插件 APK → 虎虚出现、能打字） | 0.5–1 天 |
| M1 | 功能闭环（数据 / 选项 / 学习 / 配置页 / 状态区） | 1–2 天 |
| M2 | 模型 APK + 文档 | 0.5–1 天 |
| M3 | 发布（GitHub Releases + 使用说明） | 0.5 天 |
