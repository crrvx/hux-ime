<!-- SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com> -->
<!-- SPDX-License-Identifier: GPL-3.0-or-later -->

# platform：平台层

- 分层：`hux-core` → `hux-cfg` / `hux-ffi` → `hux-scheme/tiger` → `platform/*`。
- 平台优先级：linux / android → windows → macos / ios。
- 内核无平台假设，只经 `hux-ffi` 边界接入。
- 构建 / 安装见 [install.md](../docs/install.md)。
- 安装去向与产物清单见 [resources.md](../docs/resources.md)。
- 配置项见 [config.md](../docs/config.md)。
- 结构规则见 [design.md](../docs/design.md)（§1 硬规则）。
- 随包数据见 [data/README.md](../data/README.md)。

## 状态总览

| 平台 | 状态 | 落点 / 入口 | 参考实现 |
| --- | --- | --- | --- |
| Linux 桌面 | 构建 / 安装可用 | `fcitx5/` | fcitx5 |
| Android | **待启动**（M0 起） | `android/` | fcitx5-android |
| Windows | 仅占位（骨架，暂缓） | `windows/` | `虎爪`（tigerclaw） |
| macOS | 仅占位（骨架，暂缓） | `macos/` | fcitx5-macos |
| iOS | 仅占位（骨架，暂缓） | `ios/` | fcitx5-ios |

表内为短表，逐平台细节如下。

- **Linux 桌面**
  - 状态：构建 / 安装可用；打包（PKGBUILD）待做。
  - 落点：`fcitx5/`（C++ 薄壳 + Rust 组装）。
  - 入口：根 `install.sh` / `uninstall.sh`。
  - 参考实现：fcitx5；按键语义与提交通知器参照 librime。
- **Android**
  - 状态：**待启动**（从里程碑 M0 开始）。
  - 落点：fork `fcitx5-android` 新增 `plugin/hux`。
  - 以 git submodule 引本仓库。
  - 与桌面共用 `fcitx5/`。
  - 本仓 `android/` 放插件构建接线与模型分发说明。
  - 说明见 [`android/README.md`](android/README.md)。
  - 参考实现：fcitx5-android（addon 布局与 jyutping 插件同构）。
- **Windows**
  - 状态：仅占位（骨架，暂缓）。
  - 落点：`windows/`。
  - 参考实现：上游 `虎爪`（tigerclaw，win 原生）、fcitx5-windows。
- **macOS**
  - 状态：仅占位（骨架，暂缓）；落点 `macos/`；参考实现 fcitx5-macos。
- **iOS**
  - 状态：仅占位（骨架，暂缓）；落点 `ios/`；参考实现 fcitx5-ios。

## Linux 桌面（fcitx5）

- 构建 / 安装与落点见 [`install.md`](../docs/install.md)「安装」。
- 状态：可用，**打包（PKGBUILD）待做**。
- 数据目录运行时解析见 [`reference.md`](../docs/reference.md) §2。
- 解析实现落在 `fcitx5/src/paths.rs`。
- 安装去向见 [`resources.md`](../docs/resources.md)「落点与查找顺序」。
- CI 的 `addon` 作业见 [`design.md`](../docs/design.md) §3（测试与性能纪律）。

## fcitx5 addon 行为契约

- **实现**：**C++ 薄壳**（`fcitx5/shell/`）+ **Rust 组装**（`fcitx5/src/`）。
  - C ABI 契约在 [`../crates/hux-ffi/`](../crates/hux-ffi/)。
  - 契约内容：`hux_abi.h` 声明 ↔ `libhux.so` 导出。
  - 逻辑在 [`../crates/hux-core/`](../crates/hux-core/)。
  - 逻辑在 [`../crates/hux-scheme/tiger/`](../crates/hux-scheme/tiger/)。
  - 方案侧入口：`interaction::processor` / `translate`。
  - 配置侧逻辑在 `hux-cfg`。
- **装配**：本层是**装配根**。
  - 构造 `TigerScheme` 后以 `dyn Scheme` 驱动。
  - 驱动面：按键 / 候选 / 重建 / 学习 / 反查全经契约。
  - 不引用方案内部模块。
  - 桌面与 Android **共用本层**。
- **选项键（菜单 / 面板）**：宿主不硬编码方案选项名。
  - `hux_engine_option_key(role)` 按 `HUX_OPTION_*` 角色取键。
  - 角色序 = `hux_cfg::roles::RUNTIME_OPTION_ROLES`。
  - 键由方案 `Scheme::option_declarations()` 声明。
  - 装配处解析成角色表：**缺角色即报错**。
  - 该角色无键 → 宿主跳过。
  - 面板数字序号取运行时生效值。
- **按键与提交语义**与参照（librime）一致。
  - 组合中的可打印字符（如大写字母）先提交当前组合再交应用。
  - 宿主会消费该键并以 `forwardKey` 重发。
  - 客户端先收到提交、后收到按键。
  - **例外**：布局转换键（如系统 colemak + 方案 us）交回核心处理。
  - 由核心提交**转换后**的字符。
  - 鼠标点击候选经 `hux_engine_select_candidate` 按全局索引选中并上屏。
  - 上屏路径同空格确认链。
  - 提交点学习覆盖核心路径与宿主自发提交（组合中的大写字母、候选点击）。
  - 学习行为与参照的提交通知器一致。

### 反查（行为契约）

- **共同机制**：两查同机制——触发键推入组合。
  - **仅当触发键为单字符键**（无 Ctrl/Alt/Super）时给出默认可上屏候选。
  - 触发字符按标点表取半 / 全角，空格上屏。
  - 带修饰键不给默认候选。
- **音反查**：输入拼音（支持拼写缩写）出虎码候选。
  - 预编辑按音节切分（`` `zhongguo `` → `` `zhong guo ``）。
  - 段内 `'` 是**音节分隔符**。
  - 匹配拼写键时透明跳过、但**强制断音**。
  - **强制断音**：音节与尾部补全都不得跨过分隔符。
  - 用于消歧：`` `xi'an `` 只按 `xi` + `an` 切分。
  - 分隔符在预编辑里原样保留、输入当场可见。
  - 例：`` `zh'guo `` → `` `zh'guo ``。
  - 段首 / 段尾同样保留。
  - 连续输入只保留第一个（多余的丢弃、不录入）。
  - 计分对齐 librime 词典反查。
  - 缩写罚 `log 0.5`；全拼可达时缩写剪枝。
  - 补全罚 `log 0.05`；排序 = 可信度 + `ln(权重)`。
  - 上限 20。
- **字反查**：取应用侧周边文本。
  - 周边文本不可用时两排为空、不提示。
  - 显示光标左侧 1 个字。
  - 上排（排头「咅」）= 拼音。
  - 下排（排头「虍」）= 虎码。
  - 多音 / 多码以 `/` 连接，缺数据 `?`。
  - ←/→/↑/↓ 交应用处理（本层不消费）。
  - 查码段不下发预编辑，避免 marked text 锁住光标。
  - Esc / 再次触发 / 其它键退出。
  - 展示面为面板辅助文本条（auxUp/auxDown）。

### 安装落点（平台特有部分）

- `cmake --install` 的产物 = **3 个插件文件 + 3 个图标**。
- 另加 `../data/MANIFEST` 列出的全部随包数据。
- 清单与去向见 [`install.md`](../docs/install.md)「产物清单」。
- 去向同时见 [`resources.md`](../docs/resources.md)。
- 平台侧只补插件目录三条。
  - 系统级跟随 `FCITX_INSTALL_ADDONDIR`。
  - 取值：`lib64` 或 multiarch 的 `lib/<triplet>`。
  - 兼容 `lib` / `lib64`。
  - 用户级加 `-DHUX_RELATIVE_ADDON_DIR=ON`。
  - 它把安装目标记为**相对**路径 `lib/fcitx5`。
  - 该变量本身是绝对路径，`--prefix` 无法重定位。
  - 落点结果 `<prefix>/lib/fcitx5/libhux.so`。
  - 插件目录**没有用户级缺省值**，需 `FCITX_ADDON_DIRS`。
  - `install.sh -u` 会写 `environment.d`。
  - 检测是否生效与回退办法见 [`../docs/install.md`](../docs/install.md)。
  - 对应小节：「用户级（`-u`）的环境变量」。

### 已知限制

- 会话按输入上下文隔离。
- 失焦时由 fcitx5 核心 / 前端提交客户端预编辑。
- 提交方式为**原文提交**（fcitx5 惯例，不保留组合）。
- 切换输入法 / 重置由本层**直接丢弃**（不提交）。
- 上游默认切换时提交，本实现有意取「丢弃」契约。
- 打包待做（见「Linux 桌面」节状态）。
- **配置页保存的落盘归 addon**：fcitx5 的 D-Bus 只调 `setConfig`。
  - 通道：D-Bus `Controller1::SetConfig`。
  - 它**不代写配置文件**。
  - 故本层自行 `safeSaveAsIni` 并实现 `reloadConfig()`。
  - `reloadConfig()` 由 `readAsIni` + `applyConfig` 组成。
  - 缺任一步，配置页改动会被下次启动的 `conf/hux.conf` 旧值压回。
  - 压回依据：`adoptStoredRuntimeOptions` 以「文件里显式写过」的键为权威。
- **fcitx5 版本**：CI 在 **ubuntu-26.04**（Resolute）上构建 addon。
  - 版本取 **apt 提供的版本**，现为 **5.1.19**，不钉版本。
  - `ubuntu-latest` 仍为 24.04、apt 只有 5.1.7，待迁移后换回。
  - 下限由 CMake 明示为 **5.1.15**。
  - 下限依据：指定初始化构造 `Option(OptionParameters)`。
  - `setComment` 需 ≥5.1.9。
  - 低于下限时 `find_package` 在 configure 期报错。
  - 更早版本**不受支持、不参与 CI**。
- **引擎 ↔ UI 对象的生命周期**：addon 实例（含本引擎）**先于** IC 析构。
  - 具体对象：`InputContext`。
  - 候选列表 / 状态区条目归 IC ⇒ 引擎释放后 UI 仍可能持有其指针。
  - 加固①：`~HuxEngine` 对每个 IC `clearGroup(StatusGroup::InputMethod)`。
  - 加固① 目的：摘掉状态区里的 `&menuAction_`。
  - 加固②：`HuxCandidateWord` 改持 `TrackableObjectReference<HuxEngine>`。
  - 加固② 效果：引用失效时 `select()` 直接返回。
- **配置页热键绑定的名字要求**：四项热键经 ABI 交给引擎。
  - 四项：音反查 / 字反查 / 上翻页 / 下翻页。
  - 交出形式：`keysym + 状态位`。
  - 解释按 librime 键名表。
  - 绑到**没有名字的 keysym**时无法解析、**会被丢弃**。
  - 没有名字的 keysym 如媒体键、厂商扩展键 `XF86AudioPlay`。
  - 不再静默：引擎把热键诊断写进状态串（`hux_engine_status`）。
  - 诊断文本：`hotkeys: 忽略无法识别的绑定 <角色>=<键名>`。
  - 本层在应用设置后落到日志（`FCITX_INFO`）。
  - 请绑常用键（字母 / 数字 / `minus` / `bracketleft` / `Page_Up`）。
- **状态串指针**：`hux_engine_status` 返回的指针不长期有效。
  - 有效期：**在下一次状态刷新前有效**。
  - 会替换内部串的事件：选项保存失败 / 配置诊断 / 学习库错误 / 热键诊断。
  - 故每次需要时重新调用。
  - 本层只在构造与应用设置后读取并落日志。
- **诊断前缀**：状态串含构造期加载说明与运行期诊断两类。
  - 构造期前缀：`dirs:` / `lexicon:` / `model:` / `punct:` / `learning:`。
  - 运行期 `config:`：角色缺失 / 类型不符。
  - 运行期 `options:`：`options.yaml` 保存失败。
  - 运行期 `learning:`：学习库写入失败。
  - 运行期 `hotkeys:`：无法识别的绑定。
  - 排查报障先看 `journalctl -t fcitx5 | grep 'hux:'`。
- **宿主项与模型摘要**：状态菜单含引擎角色开关与宿主项两类。
  - 引擎角色开关：`HUX_OPTION_*`，经 `hux_engine_set_option` 切换。
  - 宿主项：「候选窗口显示预编辑」「重新部署」「模型」信息行。
  - 宿主项语义见 [`config.md`](../docs/config.md)。
  - 角色的另一半在配置页：`apply_settings` 把设置值写回 `options.yaml`。
  - 本层翻转后镜像进 `conf/hux.conf` 并落盘。
  - 两侧互不压制。
  - 启动时**没写过**的共享键沿用 `options.yaml`（`adoptStoredRuntimeOptions`）。
  - 「重新部署」走 `hux_engine_redeploy`。
  - 重新部署 = 重走构造期读取、重置全部会话并重推设置与面板。
  - 重置时**会话 id 不变**。
  - 图形 schema（`HuxConfig` + `ToolTipAnnotation`）经 ABI 应用。
  - 应用入口：`hux_engine_apply_settings`。
  - 两个三态子菜单由 `applyConfig()` 收口。
  - 装载结果由 `hux_engine_data_info` 写日志。
  - 「模型」行取 `hux_engine_model_info`。
  - 细节面：`Scheme::model_info` / `Scheme::model_detail`。
  - 本层**不解析**。
  - `hux_engine_model_path` 供打开目录。
  - 该指针在**下一次重新部署前**有效。

## Android（fcitx5-android 插件，待启动）

- 计划与决策见 [`android/README.md`](android/README.md)。

## Windows / macOS / iOS（骨架，暂缓）

- 三端均为**骨架，仅占位**。
- 目录、状态与参考实现见「状态总览」。
- 共同约束：`hux-core` 无平台假设，经 `hux-ffi` 边界接入。
- 骨架约定见 [`design.md`](../docs/design.md) §5。
