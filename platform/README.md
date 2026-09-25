<!-- SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com> -->
<!-- SPDX-License-Identifier: GPL-3.0-or-later -->

# platform：平台层

分层：`hux-core` → `hux-cfg` / `hux-ffi` → `hux-scheme/tiger` → `platform/*`； \
平台优先级 linux / android → windows → macos / ios，只经 `hux-ffi` 边界接入内核（内核无平台假设）。

构建 / 安装见 [usage.md](../docs/usage.md)，安装去向与产物清单见 \
[resources.md](../docs/resources.md)，配置项见 [config.md](../docs/config.md)， \
结构规则与模块映射见 [design.md](../docs/design.md)（§1 硬规则 / §6 模块映射）， \
随包数据见 [data/README.md](../data/README.md)。

## 状态总览

| 平台 | 状态 | 落点 / 入口 | 参考实现 |
| --- | --- | --- | --- |
| Linux 桌面 | 构建 / 安装可用；打包（PKGBUILD）待做 | `fcitx5/`（C++ 薄壳 + Rust 组装）、根 `install.sh` / `uninstall.sh` | fcitx5；按键语义与提交通知器参照 librime |
| Android | **待启动**（从里程碑 M0 开始） | fork `fcitx5-android` 新增 `plugin/hux`，以 git submodule 引本仓库；<br>与桌面共用 `fcitx5/`；<br>本仓 `android/` 将放插件构建接线与模型分发说明 | fcitx5-android（addon 布局与 jyutping 插件同构） |
| Windows | 仅占位（骨架，暂缓） | `windows/` | 上游 `虎爪`（tigerclaw，win 原生）、fcitx5-windows |
| macOS | 仅占位（骨架，暂缓） | `macos/` | fcitx5-macos |
| iOS | 仅占位（骨架，暂缓） | `ios/` | fcitx5-ios |

## Linux 桌面（fcitx5）

- 构建 / 安装与落点见 [`usage.md`](../docs/usage.md)「安装」；状态：可用，**打包（PKGBUILD）待做**。
- 数据目录运行时解析见 [`design.md`](../docs/design.md) §7（实现 `fcitx5/src/paths.rs`）、 \
  安装去向见 [`resources.md`](../docs/resources.md)「落点与查找顺序」、 \
  CI 的 `addon` 作业见 [`design.md`](../docs/design.md) §3（测试与性能纪律）。

## fcitx5 addon 行为契约

- 实现：**C++ 薄壳**（`fcitx5/shell/`）+ **Rust 组装**（`fcitx5/src/`）； \
  C ABI 契约在 [`../crates/hux-ffi/`](../crates/hux-ffi/)（`hux_abi.h` 声明 ↔ `libhux.so` 导出）， \
  逻辑在 [`../crates/hux-core/`](../crates/hux-core/)、 \
  [`../crates/hux-scheme/tiger/`](../crates/hux-scheme/tiger/)（`interaction::processor` / \
  `translate`）与 `hux-cfg`。
- 装配：本层是**装配根**——构造 `TigerScheme` 后以 `dyn Scheme` 驱动（按键 / 候选 / 重建 / 学习 / \
  反查全经契约），不引用方案内部模块；桌面与 Android **共用本层**。
- 选项键（菜单 / 面板）：宿主不硬编码方案选项名——`hux_engine_option_key(role)` 按 `HUX_OPTION_*` \
  角色取键（角色序 = `hux_cfg::roles::RUNTIME_OPTION_ROLES`）； \
  键由方案 `Scheme::option_declarations()` 声明，装配处解析成角色表（**缺角色即报错**， \
  该角色无键 → 宿主跳过）；面板数字序号取运行时生效值。
- **按键与提交语义**与参照（librime）一致：组合中的可打印字符 \
  （如大写字母）先提交当前组合再交应用；宿主会消费该键并以 `forwardKey` 重发——客户端先收到提交、 \
  后收到按键。**例外**：布局转换键（如系统 colemak + 方案 us）交回核心处理， \
  由核心提交**转换后**的字符。鼠标点击候选经 `hux_engine_select_candidate` \
  按全局索引选中并上屏（同空格确认链）；提交点学习覆盖核心路径与宿主自发提交（组合中的大写字母、 \
  候选点击），与参照的提交通知器一致。

### 反查（行为契约）

- **共同机制**：两查同机制——触发键推入组合；**仅当触发键为单字符键**（无 \
  Ctrl/Alt/Super）时给出默认可上屏候选（触发字符按标点表取半 / 全角，空格上屏）， \
  带修饰键不给默认候选。
- **音反查**：输入拼音（支持拼写缩写）出虎码候选，预编辑按音节切分（`` `zhongguo `` → \
  `` `zhong guo ``）；计分对齐 librime 词典反查——缩写罚 `log 0.5`、全拼可达时缩写剪枝、 \
  补全罚 `log 0.05`、排序 = 可信度 + `ln(权重)`、上限 20。
- **字反查**：取应用侧周边文本（不可用时两排为空、不提示）， \
  显示光标左侧 1 个字——上排（排头「咅」）= 拼音、下排（排头「虍」）= 虎码（多音 / 多码以 `/` \
  连接，缺数据 `?`）；←/→/↑/↓ 交应用处理（本层不消费；查码段不下发预编辑， \
  避免 marked text 锁住光标）；Esc / 再次触发 / 其它键退出； \
  展示面为面板辅助文本条（auxUp/auxDown）。

### 析构顺序核对（真机）

`HuxEngine` 的析构契约是「**先** `sessionFactory_.unregister()`（销毁全部 `HuxSession`， \
各自调 `hux_engine_session_free`）**再** `hux_engine_free(engine_)`」； \
源码依据见 `fcitx5/shell/hux.cpp` 的 `~HuxEngine` 注释与 \
[`review-ledger.md`](../docs/review-ledger.md)「历史纪要」的 UAF 结案行。 \
真机复核靠**专属日志类别 `hux`**（`FCITX_DEFINE_LOG_CATEGORY(huxLog, "hux")` + \
`FCITX_LOGC(huxLog, Debug)`；`FCITX_DEBUG()` 走 `default` 类别，用它会要求放宽全局级别）：

```
D… hux.cpp:NNN] hux: ~HuxSession id=1
D… hux.cpp:NNN] hux: ~HuxSession id=2
D… hux.cpp:NNN] hux: ~HuxEngine
```

打开方式：日志规则**只能经命令行**给出（`fcitx5 --help`：`--verbose <logging rule>`， \
形如 `category1=level1,…`，级别 `5` = Debug；本机 5.1.22 的二进制与源码 \
`InstanceArgument::parseOption` / `fcitx::Log::setLogRule` 都只有这一条路径， \
**没有** `FCITX_LOG_RULE` 之类的环境变量）：

```bash
fcitx5 -r --verbose='hux=5'   # 前台运行：析构日志直接打在 stderr（或用 -d + journalctl -t fcitx5）
fcitx5-remote -e              # 另开终端让它退出（等价于 Ctrl+C / kill <pid>）
```

#### 实测结果（2026-09-22，fcitx5 5.1.22）

用户实跑一次（先打字建立会话，再 `fcitx5-remote -e` 退出）：

```
D 16:41:47.539570 hux.cpp:315] hux: ~HuxSession id=1
D 16:41:47.539666 hux.cpp:315] hux: ~HuxSession id=2
D 16:41:47.539687 hux.cpp:315] hux: ~HuxSession id=4
D 16:41:47.539722 hux.cpp:315] hux: ~HuxSession id=3
I 16:41:47.539734 addonmanager.cpp:306] Unloading addon hux
D 16:41:47.539737 hux.cpp:404] hux: ~HuxEngine
```

判据通过：4 个会话**全部早于** `~HuxEngine`，且其后 `~HuxSession` 计数为 **0** ⇒ \
没有会话在引擎释放后回调。规则在**进程启动期**读入，故须让带 `--verbose` \
启动的进程退出才能看到它自己的析构日志（`systemd --user` 托管时： \
给该 unit 的 `ExecStart` 加上 `--verbose=hux=5` 并重启，再 `systemctl --user stop fcitx5`）。 \
**判据**：日志里全部 `hux: ~HuxSession …` 行必须**早于** `hux: ~HuxEngine` 行； \
若顺序相反（或其后又冒出 `~HuxSession`），即命中 UAF 路径，请附日志回报。

### 安装落点（平台特有部分）

`cmake --install` 的产物 = **3 个插件文件 + 3 个图标 + `../data/MANIFEST` 列出的全部随包数据**； \
清单与去向见 [`usage.md`](../docs/usage.md)「产物清单」与 [`resources.md`](../docs/resources.md)。 \
平台侧只补插件目录三条：

- 系统级跟随 `FCITX_INSTALL_ADDONDIR`（`lib64` 或 multiarch 的 `lib/<triplet>`）， \
  兼容 `lib` / `lib64`。
- 用户级加 `-DHUX_RELATIVE_ADDON_DIR=ON`，把安装目标记为**相对**路径 \
  `lib/fcitx5`（该变量本身是绝对路径，`--prefix` 无法重定位）⇒ `<prefix>/lib/fcitx5/libhux.so`。
- 插件目录**没有用户级缺省值**，用户级安装需 `FCITX_ADDON_DIRS`（`install.sh -u` 写 \
  `~/.config/environment.d/90-hux.conf`）。

### 已知限制

会话按输入上下文隔离；失焦时由 fcitx5 核心/前端以**原文提交**客户端预编辑（fcitx5 惯例， \
不保留组合）；切换输入法/重置由本层**直接丢弃**（不提交）——上游默认切换时提交， \
本实现有意取「丢弃」契约。打包待做（见「Linux 桌面」节状态）。

- **配置页保存的落盘归 addon**：fcitx5 的 D-Bus `Controller1::SetConfig` 只调 `setConfig`、 \
  **不代写配置文件**，故本层自行 `safeSaveAsIni` 并实现 `reloadConfig()`（`readAsIni` + \
  `applyConfig`）；缺任一步，配置页改动会被下次启动的 `conf/hux.conf` \
  旧值压回（`adoptStoredRuntimeOptions` 以「文件里显式写过」的键为权威）。
- **fcitx5 版本**：CI 在 **ubuntu-26.04**（Resolute）上按 **apt 提供的版本**构建 addon（现为 \
  **5.1.19**，不钉版本；`ubuntu-latest` 仍为 24.04、apt 只有 5.1.7，待迁移后换回）； \
  下限由 CMake 明示为 **5.1.15**（指定初始化构造 `Option(OptionParameters)`； \
  `setComment` 需 ≥5.1.9），低于下限时 `find_package` 在 configure 期报错； \
  更早版本**不受支持、不参与 CI**。
- **引擎 ↔ UI 对象的生命周期**：addon 实例（含本引擎）**先于** `InputContext` 析构， \
  而候选列表 / 状态区条目归 IC ⇒ 引擎释放后 UI 仍可能持有其指针。 \
  两处加固：① `~HuxEngine` 对每个 IC `clearGroup(StatusGroup::InputMethod)`， \
  摘掉状态区里的 `&menuAction_`；② `HuxCandidateWord` 改持 `TrackableObjectReference<HuxEngine>`， \
  引用失效时 `select()` 直接返回。
- **配置页热键绑定的名字要求**：四项（音反查 / 字反查 / 上翻页 / 下翻页）经 ABI 以 \
  `keysym + 状态位` 交给引擎，按 librime 键名表解释；绑到**没有名字的 keysym**（媒体键、 \
  厂商扩展键，如 `XF86AudioPlay`）时无法解析、**会被丢弃**——不再静默： \
  引擎把 `hotkeys: 忽略无法识别的绑定 <角色>=<键名>` 写进状态串（`hux_engine_status`）， \
  本层在应用设置后落到日志（`FCITX_INFO`）。请绑常用键（字母 / 数字 / `minus` / `bracketleft` / \
  `Page_Up`）。
- **状态串指针**：`hux_engine_status` 返回的指针**在下一次状态刷新前有效**（选项保存失败 / \
  配置诊断 / 学习库错误 / 热键诊断都会替换内部串）——每次需要时重新调用； \
  本层只在构造与应用设置后读取并落日志。
- **诊断前缀**：状态串除构造期的加载说明（`dirs:` / `lexicon:` / `model:` / `punct:` / \
  `learning:`）外，运行期还有 `config:`（角色缺失 / 类型不符）、 \
  `options:`（`options.yaml` 保存失败）、`learning:`（学习库写入失败）、 \
  `hotkeys:`（无法识别的绑定）；排查报障先看 `journalctl -t fcitx5 | grep 'hux:'`。
- **宿主项与模型摘要**：状态菜单除引擎角色开关（`HUX_OPTION_*`， \
  经 `hux_engine_set_option` 切换）外还有宿主项「候选窗口显示预编辑」「重新部署」「模型」信息行 \
  （语义见 [`config.md`](../docs/config.md)）；角色的另一半在配置页（`apply_settings` \
  把设置值写回 `options.yaml`），本层翻转后镜像进 `conf/hux.conf` 并落盘， \
  两侧互不压制（启动时**没写过**的共享键沿用 `options.yaml`，`adoptStoredRuntimeOptions`）。 \
  「重新部署」走 `hux_engine_redeploy`：重走构造期读取、重置全部会话 \
  （**会话 id 不变**）并重推设置与面板；图形 schema（`HuxConfig` + `ToolTipAnnotation`）经 \
  `hux_engine_apply_settings` 应用，两个三态子菜单由 `applyConfig()` 收口， \
  装载结果由 `hux_engine_data_info` 写日志。「模型」行取 `hux_engine_model_info` \
  （`Scheme::model_info` / `Scheme::model_detail`，本层**不解析**）， \
  `hux_engine_model_path` 供打开目录，指针在**下一次重新部署前**有效。

## Android（fcitx5-android 插件，待启动）

### 已确认决策

目标：**fcitx5-android 插件 APK**，决策见下表。

| 项 | 决策 |
| --- | --- |
| 仓库 | fork `fcitx5-android`，新增 `plugin/hux`，<br>以 git submodule 引本仓库（`hux-ime`） |
| 模型 | 单独「模型插件」APK<br>（仅 assets 携带 <br>`usr/share/fcitx5/hux/models/sentence-ngram-mobile.bin`） |
| ABI | 仅 `arm64-v8a`（Rust 目标 `aarch64-linux-android`） |
| 分发 | GitHub Releases |
| 构建 | 本地 Gradle 为主；CI 待定 |

### 上游集成事实

- **插件 = 独立 APK**：包名 `org.fcitx.fcitx5.android.plugin.<name>[.debug]`， \
  `${appId}.plugin.MANIFEST` intent + `res/xml/plugin.xml`（`apiVersion 0.1`）； \
  主程序合并其 `assets/`（`DataManager` 复制进应用数据目录、`descriptor.json` 差量更新）。
- **addon 布局**（同构 jyutping）：`usr/lib/fcitx5/libhux.so`、`usr/share/fcitx5/addon/hux.conf`、 \
  `usr/share/fcitx5/inputmethod/hux.conf`（`COMPONENT config`）、 \
  `usr/share/fcitx5/hux/…`（`COMPONENT prebuilt-assets`， \
  `fcitxComponent { installPrebuiltAssets = true }`）；候选点击走 \
  `CandidateWord::select()`（`androidfrontend.cpp`）。
- **运行时环境**（`native-lib.cpp`，先于 fcitx5）：`XDG_DATA_HOME=<外部 files>/data`（可写： \
  选项 / 学习库 / 模型）、`XDG_DATA_DIRS=<appData>/usr/share`（插件数据安装位置）、 \
  `FCITX_ADDON_DIRS` 由核心处理。**构建**：NDK `28.0.13004108`、CMake `3.31.6`、AGP； \
  插件模块用五个约定插件（app / plugin-app / native-app / data-descriptor / fcitx-component）。

### 本仓库（hux-ime）改动

- 平台层数据目录查找支持 **`XDG_DATA_DIRS`**（落 `fcitx5/src/paths.rs`，内核不读环境变量）， \
  桌面行为不变；顺序见 [`design.md`](../docs/design.md) §7。
- `__ANDROID__` 差异（配置 schema、状态区子菜单）待验收决定； \
  文档 [`usage.md`](../docs/usage.md) 增补 Android 安装 / 模型与 `REUSE` 头。

### fork 侧工作（`plugin/hux`，未开始）

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

### 验收（真机）

① 装好主程序 + 插件 → 输入法列表出现「虎虚」；② 打字出候选、点击上屏、翻页、数字直选； \
③ 音反查 / 字反查（软键盘触发键可另配；硬件键盘默认 `` ` `` / `~`）； \
④ 配置页「行为/快捷键」可读写并即时生效；⑤ 选项 / 学习库落在 \
`Android/data/<pkg>/files/data/fcitx5/hux/`；⑥ 装模型 APK 后整句质量提升， \
logcat 可见 `hux: dirs… model…`。

### 风险与备选

- **Rust × AGP**：CMake 内调 cargo 不稳 → 先脚本 cargo 构建，CMake 只链接。
- **配置页渲染**：`List|Key` 与嵌套子配置受支持（对照 Android `ConfigType`）； \
  异常 → Android 分支扁平 schema。**状态区**：`SimpleAction`+`Menu` 子菜单不被渲染 → 平铺 5 个开关。
- **模型体积**（~224 MB）：GitHub Releases 直发，F-Droid/Play 暂不做； \
  **上游收编**先 fork 自用，视情况再提 PR（其 CI 是否接受 Rust 构建待议）。

### 里程碑

| 阶段 | 内容 | 预估 |
| --- | --- | --- |
| M0 | 骨架可加载（插件 APK → 虎虚出现、能打字） | 0.5–1 天 |
| M1 | 功能闭环（数据/选项/学习/配置页/状态区） | 1–2 天 |
| M2 | 模型 APK + 文档 | 0.5–1 天 |
| M3 | 发布（GitHub Releases + 使用说明） | 0.5 天 |

## Windows / macOS / iOS（骨架，暂缓）

三端均为**骨架，仅占位**（目录、状态与参考实现见「状态总览」）； \
共同约束：`hux-core` 无平台假设，经 `hux-ffi` 边界接入（骨架约定见 \
[`design.md`](../docs/design.md) §5）。
