<!-- SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com> -->
<!-- SPDX-License-Identifier: GPL-3.0-or-later -->

# fcitx5-android 适配（开发文档）

目标：虎虚以 **fcitx5-android 插件 APK** 形式发布。先 `arm64-v8a` + GitHub Releases，本地 Gradle 构建。

## 1. 已确认决策

- **仓库**：fork `fcitx5-android`，新增 `plugin/hux`，以 git submodule 引本仓库（`hux-ime`）。
- **模型**：单独「模型插件」APK（仅 assets 携带 `usr/share/fcitx5/hux/models/sentence-ngram-mobile.bin`）。
- **ABI**：仅 `arm64-v8a`（Rust 目标 `aarch64-linux-android`）。
- **分发**：GitHub Releases。
- **构建**：本地 Gradle 为主；CI 待定。

## 2. 上游集成事实（摘要）

- **插件 = 独立 APK**：包名 `org.fcitx.fcitx5.android.plugin.<name>[.debug]`，含 `${appId}.plugin.MANIFEST` intent 与
  `res/xml/plugin.xml`（`apiVersion 0.1`）；主程序自动发现并合并其 `assets/`。
- **数据安装**：插件 `assets/` 由主程序 `DataManager` 复制进应用数据目录（设备加密存储），`descriptor.json` 做差量更新。
- **addon 布局**（与 jyutping 插件同构，CMake `install` 到 assets）：
  - `usr/lib/fcitx5/libhux.so`
  - `usr/share/fcitx5/addon/hux.conf`、`usr/share/fcitx5/inputmethod/hux.conf`（`COMPONENT config`）
  - `usr/share/fcitx5/hux/…`（数据；`COMPONENT prebuilt-assets`，`fcitxComponent { installPrebuiltAssets = true }`）
- **运行时环境**（主程序 `native-lib.cpp` 设置，先于 fcitx5 启动）：
  - `XDG_DATA_HOME=<外部 files>/data` → 可写：选项/学习库/模型；
  - `XDG_DATA_DIRS=<appData>/usr/share` → 插件数据的安装位置；
  - `FCITX_ADDON_DIRS` 等由核心处理。
- **候选点击**走 `CandidateWord::select()`（`androidfrontend.cpp`）→ 现实现直接可用。
- **构建要求**：NDK `28.0.13004108`、CMake `3.31.6`、AGP；插件模块用五个约定插件（app / plugin-app / native-app / data-descriptor / fcitx-component）。

## 3. 本仓库（hux-ime）改动

- `hux-core` 数据目录查找补 **`XDG_DATA_DIRS`**：
  顺序 `HUX_DATA_DIRS`（覆盖） > `XDG_DATA_HOME/fcitx5/hux` > `XDG_DATA_DIRS/*/fcitx5/hux` > `/usr/share/fcitx5/hux`；
  桌面行为不变（新路径只是补充）。
- 视验收结果决定是否需要 `__ANDROID__` 差异（配置 schema、状态区子菜单；见 §6）。
- 文档：`docs/usage.md` 增补 Android 安装/模型；`REUSE` 头。

## 4. fork 侧工作（`plugin/hux`）

- [ ] `settings.gradle.kts` 加 `include(":plugin:hux")`；`.gitmodules` 加 `hux-ime` 子模块。
- [ ] `plugin/hux/build.gradle.kts`：五个约定插件；`packaging.jniLibs.excludes`（`libc++_shared`、`libFcitx5*` 等）。
- [ ] `AndroidManifest.xml`、`res/xml/plugin.xml`（domain `fcitx5-hux`）、图标与文案、`plugin_resources_keep.xml`。
- [ ] `src/main/cpp/CMakeLists.txt`：
  - `find_package(fcitx5 CONFIG)` + `find_package(Fcitx5Core MODULE)`；
  - Rust：`ANDROID_ABI=arm64-v8a → aarch64-linux-android`，`cargo build --target … --release`（**staticlib 无需链接器配置**）；
  - `add_library(hux SHARED <hux-ime>/crates/hux-addon/shell/hux.cpp)` + 链接 `libhux_addon.a`、`Fcitx5::Core`（按需 `log dl m unwind`）；
  - `install(TARGETS hux LIBRARY DESTINATION /usr/lib/fcitx5 COMPONENT config)`、
    `install(FILES conf/* … COMPONENT config)`、`install(DIRECTORY data/ … COMPONENT prebuilt-assets)`（排除 `README.md`）。
- [ ] 模型插件模块：仅 `assets/usr/share/fcitx5/hux/models/sentence-ngram-mobile.bin` + `plugin.xml`。
- [ ] 本地构建：`./gradlew :plugin:hux:assembleRelease`（需 Android SDK/NDK、`rustup target add aarch64-linux-android`）。

## 5. 验收（真机）

1. 主程序 + 插件装好 → 输入法列表出现「虎虚」。
2. 打字出候选、点击上屏、翻页、数字直选。
3. 音反查/字反查（软键盘触发键可另配；硬件键盘用默认 `Alt+:` / `Alt+"`）。
4. 配置页「行为/快捷键」可读写并即时生效。
5. 选项/学习库落在 `Android/data/<pkg>/files/data/fcitx5/hux/`。
6. 装模型 APK 后整句质量提升；logcat 可见 `hux: dirs… model…` 状态。

## 6. 风险与备选

- **Rust × AGP**：若 CMake 内调 cargo 不稳 → 改为「先脚本 cargo 构建，CMake 只链接」。
- **配置页渲染**：`List|Key` 与嵌套子配置受支持（对照 Android `ConfigType`）；若异常 → Android 分支扁平 schema。
- **状态区**：`SimpleAction`+`Menu` 子菜单若不被 Android 状态区渲染 → 平铺 5 个开关。
- **模型体积**（~224 MB）：GitHub Releases 直发；F-Droid/Play 暂不做。
- **上游收编**：先 fork 自用；视情况再提 PR（其 CI 是否接受 Rust 构建待议）。

## 7. 里程碑

| 阶段 | 内容 | 预估 |
| --- | --- | --- |
| M0 | 骨架可加载（插件 APK → 虎虚出现、能打字） | 0.5–1 天 |
| M1 | 功能闭环（数据/选项/学习/配置页/状态区） | 1–2 天 |
| M2 | 模型 APK + 文档 | 0.5–1 天 |
| M3 | 发布（GitHub Releases + 使用说明） | 0.5 天 |
