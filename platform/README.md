<!-- SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com> -->
<!-- SPDX-License-Identifier: GPL-3.0-or-later -->

# platform：平台层

分层：`hux-core`（内核）→ `hux-cfg` / `hux-ffi`（配置 / C ABI 边界）→ `hux-scheme/tiger`（虎句方案）→ `platform/*`（平台适配）。
平台优先级：linux / android → windows → macos / ios；平台只经 `hux-ffi` 边界接入内核（内核无平台假设）。
安装、数据目录与使用见 [`../docs/usage.md`](../docs/usage.md)，配置项见 [`../docs/config.md`](../docs/config.md)；
结构规则见 [`../docs/refactor.md`](../docs/refactor.md)，模块映射见 [`../docs/design.md`](../docs/design.md)，随包数据见 [`../data/README.md`](../data/README.md)。

## 状态总览

| 平台 | 状态 | 落点 / 入口 | 参考实现 |
| --- | --- | --- | --- |
| Linux 桌面 | 构建 / 安装可用；打包（PKGBUILD）待做 | `fcitx5/`（C++ 薄壳 + Rust 组装）、根 `install.sh` / `uninstall.sh` | fcitx5；按键语义与提交通知器参照 librime |
| Android | **待启动**（从里程碑 M0 开始） | fork `fcitx5-android` 新增 `plugin/hux`，以 git submodule 引本仓库；与桌面共用 `fcitx5/`；本仓 `android/` 将放插件构建接线与模型分发说明 | fcitx5-android（addon 布局与 jyutping 插件同构） |
| Windows | 仅占位（骨架，暂缓） | `windows/` | 上游 `虎爪`（tigerclaw，win 原生）、fcitx5-windows |
| macOS | 仅占位（骨架，暂缓） | `macos/` | fcitx5-macos |
| iOS | 仅占位（骨架，暂缓） | `ios/` | fcitx5-ios |

## Linux 桌面（fcitx5）

构建 / 安装入口：

- 手工：`cmake -S platform/fcitx5 -B build/addon -DCMAKE_BUILD_TYPE=Release -DCMAKE_INSTALL_PREFIX=/usr`
  → `cmake --build build/addon -j` → `sudo cmake --install build/addon`；
- 一键：仓库根 `./install.sh` / `./uninstall.sh`（预演 `--dry-run`）；
- 数据目录：`$XDG_DATA_HOME/fcitx5/hux` 与 `$XDG_DATA_DIRS/*/fcitx5/hux`（解析见 `fcitx5/src/paths.rs`）；
  插件库目录兼容 `lib` / `lib64`（`FCITX_INSTALL_ADDONDIR` 优先）；
- 随包数据（`../data/MANIFEST`）与安装落点见下节「安装落点」；
- CI 的 `addon` 作业：configure / 构建 / 链接、`hux_abi.h` ↔ `libhux.so` 符号一致、
  `DESTDIR` 安装布局（3 个插件文件 + `../data/MANIFEST` 全部随包数据）。
- 状态：构建 / 安装可用；**打包（PKGBUILD）待做**（发行版打包脚本，不影响安装落点）。

共享实现（C++ 薄壳 + Rust 组装）见下节；Android 见「Android」节。

## fcitx5 addon 行为契约

hux-ime（虎虚）fcitx5 平台适配：**C++ 薄壳**（`fcitx5/shell/`，只做 fcitx5 接口适配）+ **Rust 组装**（`fcitx5/src/`）。
C ABI 契约在 [`../crates/hux-ffi/`](../crates/hux-ffi/)（`hux_abi.h` 声明 ↔ `libhux.so` 导出）；
逻辑在 [`../crates/hux-core/`](../crates/hux-core/)（内核）、[`../crates/hux-scheme/tiger/`](../crates/hux-scheme/tiger/)（虎句方案：`interaction::processor` / `translate`）与 `hux-cfg`。
按键 → 方案（处理器 / 翻译）→ core（会话 / 宿主链）→ 提交 / preedit / 候选 → fcitx5。

选项键（菜单 / 面板）：宿主不硬编码方案选项名——`hux_engine_option_key(role)` 按 `HUX_OPTION_*`
角色向引擎取键（角色序 = `hux_cfg::roles::RUNTIME_OPTION_ROLES`，与状态菜单同源）；
键的来源是方案声明 `Scheme::option_declarations()`，装配处解析成角色表（**缺角色即报错**，
该角色无键 → 宿主跳过该项）；面板数字序号取运行时生效值。

装配方式：本层是**装配根**——构造 `TigerScheme` 后以 `dyn Scheme` 驱动（按键 / 候选 / 重建 /
学习 / 反查全经契约），不引用方案内部模块；
桌面与 Android **共用本层**（两端同为 fcitx5；Android 接线见「Android」节）；
数据目录与模型解析见 `fcitx5/src/paths.rs`。

按键语义与参照（librime）一致：组合中的可打印字符（如大写字母）先提交当前组合，再交应用；
为保证上屏顺序，宿主层会消费该键并以 `forwardKey` 重发——客户端先收到提交、后收到按键。
**例外**：布局转换键（系统布局与方案布局不同时，如系统 colemak + 方案 us）交回核心处理，
由核心提交**转换后**的字符；自行转发会让客户端按系统布局重新解释该键。

鼠标点击候选 = 按该候选选中并上屏（与空格确认同一条确认/学习链）；提交点学习覆盖核心路径与
宿主自发提交（如组合中的大写字母、候选点击），与参照的提交通知器一致。

### 反查（行为契约）

音反查与字反查同机制：触发键推入组合；**仅当触发键为单字符键**（无 Ctrl/Alt/Super）时给出
默认可上屏候选（触发字符按标点表取半/全角，空格上屏），带修饰键的触发不给默认候选。

- **音反查**：输入拼音（支持拼写缩写）出虎码候选；预编辑按音节切分（`` `zhongguo `` → `` `zhong guo ``）。
- **字反查**：取应用侧周边文本（应用不可用时查不到内容、两排为空，不做提示）；两排显示光标
  左侧 1 个字——上排（排头「咅」）= 拼音、下排（排头「虍」）= 虎码（多音/多码以 `/` 连接，缺数据 `?`）；
  ←/→/↑/↓ 交应用处理（应用光标随动，本层不消费；查码段不下发预编辑，避免应用端 marked text 锁住光标）；
  Esc / 再次触发 / 其它键退出（打字照常输入）。展示面为输入面板辅助文本条（auxUp/auxDown）。

### 析构顺序核对（真机）

`HuxEngine` 的析构契约是「**先** `sessionFactory_.unregister()`（fcitx5 当场销毁全部
`HuxSession`，各自调 `hux_engine_session_free`）**再** `hux_engine_free(engine_)`」；
源码依据（文件 + 函数 + 结论）见 `fcitx5/shell/hux.cpp` 的 `~HuxEngine` 注释与
[`../docs/review-ledger.md`](../docs/review-ledger.md) §5.3 的「报告 §5①」段。真机可用日志复核：

两条析构日志打在**专属日志类别 `hux`** 上（`fcitx5/shell/hux.cpp` 用
`FCITX_DEFINE_LOG_CATEGORY(huxLog, "hux")` + `FCITX_LOGC(huxLog, Debug)`——`FCITX_DEBUG()`
走的是名为 `default` 的类别，用它会要求放宽全局级别）：

```
D… hux.cpp:NNN] hux: ~HuxSession id=1
D… hux.cpp:NNN] hux: ~HuxSession id=2
D… hux.cpp:NNN] hux: ~HuxEngine
```

打开方式：日志规则**只能经命令行**给出（`fcitx5 --help`：`--verbose <logging rule>`，
形如 `category1=level1,…`，级别 `5` = Debug；本机 5.1.22 的二进制与源码
`InstanceArgument::parseOption` / `fcitx::Log::setLogRule` 都只有 `--verbose` 一条路径，
**没有** `FCITX_LOG_RULE` 之类的环境变量）：

```bash
fcitx5 -r --verbose='hux=5'   # 前台运行：析构日志直接打在 stderr（或用 -d + journalctl -t fcitx5）
fcitx5-remote -e              # 另开终端让它退出（等价于 Ctrl+C / kill <pid>）
```

#### 实测结果（2026-09-22，fcitx5 5.1.22）

用户实跑一次（先在各应用里打字建立会话，再 `fcitx5-remote -e` 退出），观察到：

```
D 16:41:47.539570 hux.cpp:315] hux: ~HuxSession id=1
D 16:41:47.539666 hux.cpp:315] hux: ~HuxSession id=2
D 16:41:47.539687 hux.cpp:315] hux: ~HuxSession id=4
D 16:41:47.539722 hux.cpp:315] hux: ~HuxSession id=3
I 16:41:47.539734 addonmanager.cpp:306] Unloading addon hux
D 16:41:47.539737 hux.cpp:404] hux: ~HuxEngine
```

判据通过：4 个会话**全部早于** `~HuxEngine`，且 `~HuxEngine` 之后 `~HuxSession` 计数为 **0**
⇒ 没有任何会话在引擎释放后回调。注意该次记录里 4 个会话是**随各自 IC 在收尾时先销毁**的（因此
`~HuxEngine` 那行虽在析构体首行却排在它们之后）；`unregister()` 那条路径的安全性由源码链条保证
（见 `~HuxEngine` 注释）。

规则是**进程启动期**读入的，故必须让「带 `--verbose` 启动的那个进程」退出，才能看到它自己的
析构日志（`systemd --user` 托管的场合：先给该 unit 的 `ExecStart` 加上 `--verbose=hux=5` 并重启，
再 `systemctl --user stop fcitx5`）。

**判据**：日志里全部 `hux: ~HuxSession …` 行必须**早于** `hux: ~HuxEngine` 行。
若顺序相反（或 `~HuxEngine` 之后又冒出 `~HuxSession`），即命中 UAF 路径，请附日志回报。

### 安装落点（`cmake --install` 与 `install.sh` 等价）

`cmake --install`（前缀 `/usr`）装 **3 个插件文件 + 3 个图标 + `../data/MANIFEST` 列出的全部随包数据**；
落点随安装级别分两套，`<prefix>` = `/usr`（`install.sh -s`，需要 sudo）或 `$HOME/.local`
（`install.sh -u`，无需 sudo）：

- `<libdir>/fcitx5/libhux.so`：系统级跟随 `FCITX_INSTALL_ADDONDIR`（Fedora 等为 `lib64`，
  Debian/Ubuntu 为 multiarch 的 `lib/<triplet>`）；用户级加 `-DHUX_RELATIVE_ADDON_DIR=ON`，
  把安装目标记为**相对**路径 `lib/fcitx5`（该变量本身是绝对路径，`--prefix` 无法重定位）
  ⇒ `<prefix>/lib/fcitx5/libhux.so`。插件目录**没有用户级缺省值**，用户级安装需
  `FCITX_ADDON_DIRS`（`install.sh -u` 写 `~/.config/environment.d/90-hux.conf`）。
- `<prefix>/share/fcitx5/addon/hux.conf`、`<prefix>/share/fcitx5/inputmethod/hux.conf`；
- `<prefix>/share/fcitx5/themes/hufu-*/`：19 套共享主题（fcitx5 主题形态；清单 `../assets/themes/MANIFEST`
  与 `install.sh` / `uninstall.sh` / CI 守卫同源）；
- `<prefix>/share/icons/hicolor/{scalable,48x48,22x22}/apps/hux.{svg,png}`（输入法条目与状态区图标，
  `Icon=hux` 按主题名解析；图形主源在多平台共享目录 `../assets/branding/hux.png`，其余由它派生）；
- `<prefix>/share/fcitx5/hux/`：码表四件套 + 追加码表 `tiger_sentence.codes.huma.txt`（生僻字
  可打，主表 rank 不变）+ 词先验 + 拼音索引 + 标点表（`../data/MANIFEST` 单一来源，
  `install.sh` 装后逐条核对、`uninstall.sh` 按同一清单删除）。

**随包数据随 `cmake --install` 一并安装**（CMake 按 `../data/MANIFEST` 读出文件清单）：只装 3 个插件
文件而不装数据时，引擎的 `Lexicon` / `PunctTable` 会静默降级（打字无输出 / 无标点），发行版打包与
`DESTDIR` 流程同样如此；`../tools/checks/check_data_manifest.sh` 与 CI 的 `DESTDIR` 步骤守护
「装 / 卸 / CMake 三处清单一致」。**n-gram 模型仍不随包**（用户自取；`uninstall.sh` 缺省保留，
「是否卸载模型」一问回答 y 才删）。

### 已知限制

会话按输入上下文隔离；失焦时由 fcitx5 核心/前端把客户端预编辑以**原文提交**（fcitx5 惯例，
不保留组合）；切换输入法/重置由本层**直接丢弃**（不提交）。上游默认在切换输入法时提交
候选/预编辑，本实现有意取「丢弃」契约；打包待做（见「Linux 桌面」节状态）。

- **配置页保存的落盘归 addon**：fcitx5 的 D-Bus `Controller1::SetConfig` 只调 `setConfig`、
  **不代写配置文件**，故本层在 `setConfig` 里自行 `safeSaveAsIni`，并实现 `reloadConfig()`
  （`readAsIni` + `applyConfig`）以接收文件被外部改动的情形；缺任一步都会让配置页的改动在下次启动
  被 `conf/hux.conf` 的旧值压回（`adoptStoredRuntimeOptions` 以「文件里显式写过」的键为权威）。

- **fcitx5 版本**：CI 在 **ubuntu-26.04** 上按 **apt 提供的版本**构建 addon（26.04 = Resolute，其 apt
  当前给 **5.1.19**，不钉具体版本；`ubuntu-latest` 目前仍是 24.04、其 apt 只有 5.1.7，待 GitHub 迁移后
  再换回）。代码下限由
  CMake 明示为 **5.1.15**（配置项的指定初始化构造 `Option(OptionParameters)`；候选注释 `setComment`
  需 ≥5.1.9），低于下限时 `find_package` 在 configure 阶段直接报错。更早的版本（例：Ubuntu 24.04 的
  5.1.7）**不受支持、不参与 CI**。

- **引擎 ↔ UI 对象的生命周期**：fcitx5 中 addon 实例（含本引擎）**先于** `InputContext` 析构，
  而候选列表 / 状态区条目归 IC 所有 ⇒ 引擎释放后 UI 仍可能持有指向引擎或其成员的指针。
  本层两处加固：① `~HuxEngine` 对每个 IC `clearGroup(StatusGroup::InputMethod)`，摘掉状态区里
  的 `&menuAction_`（含子菜单）；② `HuxCandidateWord` 改持 `TrackableObjectReference<HuxEngine>`
  （fcitx5 弱引用惯用法），引用失效时 `select()` 直接返回、不触碰引擎。

- **配置页热键绑定的名字要求**：快捷键分区的四项（音反查 / 字反查 /
  上翻页 / 下翻页）经 ABI 以 `keysym + 状态位` 交给引擎，引擎按 librime 键名表解释。配置页若绑到
  **没有名字的 keysym**（媒体键、厂商扩展键，如 `XF86AudioPlay`），该绑定无法解析，
  **会被丢弃**——不再静默：引擎把 `hotkeys: 忽略无法识别的绑定 <角色>=<键名>` 写进状态串
  （`hux_engine_status`），本层在应用设置后把状态串落到日志（`FCITX_INFO`，`journalctl -t fcitx5` 可见）。
  请绑常用键（字母 / 数字 / `minus` / `bracketleft` / `Page_Up` …）。
- **状态串指针**：`hux_engine_status` 返回的指针**在下一次状态刷新前有效**
  （选项保存失败 / 配置诊断 / 学习库错误 / 热键诊断都会替换内部串）——每次需要时重新调用，
  不要缓存；本层只在构造与应用设置后立即读取并落日志。
- **宿主项与模型摘要**：状态菜单里除引擎角色开关（`HUX_OPTION_*`）外还有宿主项——
  「候选窗口显示预编辑」（写 `conf/hux.conf`，切换即按会话里的最近一次 UI 快照重放）、
  「重新部署」与「模型」信息行。角色开关的另一半在配置页（同一批项）：`apply_settings` 会把设置值
  写回 `options.yaml`，本层在状态菜单翻转后把新值镜像进 `conf/hux.conf` 并落盘——两侧互不压制；
  启动时配置文件**没写过**的共享键沿用引擎（`options.yaml`）的现存值（`adoptStoredRuntimeOptions`）。
  重新部署走 `hux_engine_redeploy`：引擎重走构造期读取（目录、模型、方案数据、选项存储、学习库）
  并重置全部会话状态（**会话 id 不变**，输入上下文不需要重建），随后本层按同一规则对齐共享开关、
  重推设置、清空各面板与会话快照并刷新菜单。
  模型行取 `hux_engine_model_info`（一行摘要：文件名 + 装载状态，由方案侧结构化产出，本层**不解析**）；
  该指针在**下一次重新部署前**有效（同状态串的用法：每次需要时重新调用）。
- **诊断前缀**：状态串除构造期的加载说明（`dirs:` / `lexicon:` / `model:` / `punct:` / `learning:`）
  外，运行期还会出现 `config:`（配置袋角色缺失/类型不符）、`options:`（`options.yaml` 保存失败）、
  `learning:`（运行期学习库写入失败）、`hotkeys:`（无法识别的键绑定）。
  排查用户报障时先看 `journalctl -t fcitx5 | grep 'hux:'`。

## Android（fcitx5-android 插件，待启动）

目标：虎虚以 **fcitx5-android 插件 APK** 形式发布。先 `arm64-v8a` + GitHub Releases，本地 Gradle 构建。

### 已确认决策

| 项 | 决策 |
| --- | --- |
| 仓库 | fork `fcitx5-android`，新增 `plugin/hux`，以 git submodule 引本仓库（`hux-ime`） |
| 模型 | 单独「模型插件」APK（仅 assets 携带 `usr/share/fcitx5/hux/models/sentence-ngram-mobile.bin`） |
| ABI | 仅 `arm64-v8a`（Rust 目标 `aarch64-linux-android`） |
| 分发 | GitHub Releases |
| 构建 | 本地 Gradle 为主；CI 待定 |

### 上游集成事实

- **插件 = 独立 APK**：包名 `org.fcitx.fcitx5.android.plugin.<name>[.debug]`，含 `${appId}.plugin.MANIFEST` intent 与
  `res/xml/plugin.xml`（`apiVersion 0.1`）；主程序自动发现并合并其 `assets/`。
- **数据安装**：插件 `assets/` 由主程序 `DataManager` 复制进应用数据目录（设备加密存储），`descriptor.json` 做差量更新。
- **addon 布局**（与 jyutping 插件同构，CMake `install` 到 assets）：
  - `usr/lib/fcitx5/libhux.so`；
  - `usr/share/fcitx5/addon/hux.conf`、`usr/share/fcitx5/inputmethod/hux.conf`（`COMPONENT config`）；
  - `usr/share/fcitx5/hux/…`（数据；`COMPONENT prebuilt-assets`，`fcitxComponent { installPrebuiltAssets = true }`）。
- **运行时环境**（主程序 `native-lib.cpp` 设置，先于 fcitx5 启动）：
  - `XDG_DATA_HOME=<外部 files>/data` → 可写：选项/学习库/模型；
  - `XDG_DATA_DIRS=<appData>/usr/share` → 插件数据的安装位置；
  - `FCITX_ADDON_DIRS` 等由核心处理。
- **候选点击**走 `CandidateWord::select()`（`androidfrontend.cpp`）→ 现实现直接可用。
- **构建要求**：NDK `28.0.13004108`、CMake `3.31.6`、AGP；插件模块用五个约定插件
  （app / plugin-app / native-app / data-descriptor / fcitx-component）。

### 本仓库（hux-ime）改动

- 平台层数据目录查找支持 **`XDG_DATA_DIRS`**（落在 `fcitx5/src/paths.rs`，内核不读环境变量）：
  顺序 `HUX_DATA_DIRS`（覆盖） > `XDG_DATA_HOME/fcitx5/hux` > `XDG_DATA_DIRS/*/fcitx5/hux` > `/usr/share/fcitx5/hux`；
  桌面行为不变（新路径只是补充）。
- 视验收结果决定是否需要 `__ANDROID__` 差异（配置 schema、状态区子菜单；见「风险与备选」）。
- 文档：[`../docs/usage.md`](../docs/usage.md) 增补 Android 安装/模型；`REUSE` 头。

### fork 侧工作（`plugin/hux`，未开始）

- `settings.gradle.kts` 加 `include(":plugin:hux")`；`.gitmodules` 加 `hux-ime` 子模块。
- `plugin/hux/build.gradle.kts`：五个约定插件；`packaging.jniLibs.excludes`（`libc++_shared`、`libFcitx5*` 等）。
- `AndroidManifest.xml`、`res/xml/plugin.xml`（domain `fcitx5-hux`）、图标与文案、`plugin_resources_keep.xml`。
- `src/main/cpp/CMakeLists.txt`：
  - `find_package(fcitx5 CONFIG)` + `find_package(Fcitx5Core MODULE)`；
  - Rust：`ANDROID_ABI=arm64-v8a → aarch64-linux-android`，`cargo build --target … --release`（**staticlib 无需链接器配置**）；
  - `add_library(hux SHARED <hux-ime>/platform/fcitx5/shell/hux.cpp)` + 链接 `libhux_platform_fcitx5.a`、`Fcitx5::Core`（按需 `log dl m unwind`）；
  - `install(TARGETS hux LIBRARY DESTINATION /usr/lib/fcitx5 COMPONENT config)`、
    `install(FILES conf/* … COMPONENT config)`、`install(DIRECTORY data/ … COMPONENT prebuilt-assets)`（排除 `README.md`）。
- 模型插件模块：仅 `assets/usr/share/fcitx5/hux/models/sentence-ngram-mobile.bin` + `plugin.xml`。
- 本地构建：`./gradlew :plugin:hux:assembleRelease`（需 Android SDK/NDK、`rustup target add aarch64-linux-android`）。

### 验收（真机）

1. 主程序 + 插件装好 → 输入法列表出现「虎虚」。
2. 打字出候选、点击上屏、翻页、数字直选。
3. 音反查/字反查（软键盘触发键可另配；硬件键盘用默认 `` ` `` / `~`）。
4. 配置页「行为/快捷键」可读写并即时生效。
5. 选项/学习库落在 `Android/data/<pkg>/files/data/fcitx5/hux/`。
6. 装模型 APK 后整句质量提升；logcat 可见 `hux: dirs… model…` 状态。

### 风险与备选

- **Rust × AGP**：若 CMake 内调 cargo 不稳 → 改为「先脚本 cargo 构建，CMake 只链接」。
- **配置页渲染**：`List|Key` 与嵌套子配置受支持（对照 Android `ConfigType`）；若异常 → Android 分支扁平 schema。
- **状态区**：`SimpleAction`+`Menu` 子菜单若不被 Android 状态区渲染 → 平铺 5 个开关。
- **模型体积**（~224 MB）：GitHub Releases 直发；F-Droid/Play 暂不做。
- **上游收编**：先 fork 自用；视情况再提 PR（其 CI 是否接受 Rust 构建待议）。

### 里程碑

| 阶段 | 内容 | 预估 |
| --- | --- | --- |
| M0 | 骨架可加载（插件 APK → 虎虚出现、能打字） | 0.5–1 天 |
| M1 | 功能闭环（数据/选项/学习/配置页/状态区） | 1–2 天 |
| M2 | 模型 APK + 文档 | 0.5–1 天 |
| M3 | 发布（GitHub Releases + 使用说明） | 0.5 天 |

## Windows / macOS / iOS（骨架，暂缓）

三端均为**骨架，仅占位**；共同首要约束：`hux-core` 无平台假设，经 `hux-ffi` 边界接入
（见 [`../docs/refactor.md`](../docs/refactor.md)）。

| 平台 | 参考实现 | 状态 |
| --- | --- | --- |
| Windows（`windows/`） | 上游 `虎爪`（tigerclaw，win 原生）与 fcitx5-windows | 仅占位 |
| macOS（`macos/`） | fcitx5-macos | 仅占位 |
| iOS（`ios/`） | fcitx5-ios | 仅占位 |
