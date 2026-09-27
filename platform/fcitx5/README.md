<!-- SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com> -->
<!-- SPDX-License-Identifier: GPL-3.0-or-later -->

# fcitx5 宿主层：桌面与 Android 共用的 addon

- C++ 薄壳（`shell/`）只做 fcitx5 接口适配：按键 → 本层、提交 / preedit / 候选 ← 本层回调
- Rust 组装（`src/`）是**装配根**：构造方案并驱动它
- 桌面与 Android **共用本层**，两端只在数据目录来源与 `__ANDROID__` 分支上不同
- 两个落点各有一册：[`../linux/README.md`](../linux/README.md) 与 \
  [`../android/README.md`](../android/README.md)

## 边界与实现

- 两侧之间**只有 C ABI**：`crates/hux-ffi/include/hux_abi.h` 的声明 ↔ `libhux.so` 的导出， \
  契约目录在 [`../../crates/hux-ffi/`](../../crates/hux-ffi/)
- 逻辑在 [`../../crates/hux-core/`](../../crates/hux-core/) 与 \
  [`../../crates/hux-scheme/tiger/`](../../crates/hux-scheme/tiger/)，配置侧逻辑在 `hux-cfg`
- 本层构造 `TigerScheme`，之后只以 `dyn Scheme` 驱动；驱动面即按键、候选、重建、学习、反查 \
  五项
- 本层**不引用方案内部模块**；方案侧入口只有 `interaction::processor` 与 `translate`

## 选项键：按角色取，不认名字

- 宿主**不硬编码**方案选项名，而是用 `hux_engine_option_key(role)` 按 `HUX_OPTION_*` 角色取键
  - 键由方案 `Scheme::option_declarations()` 声明，角色序即 `hux_cfg::roles::RUNTIME_OPTION_ROLES`
  - 装配处把「角色 → 键」解析成表，**缺角色即报错**；某角色无键时宿主跳过该项， \
    不静默落到别的键上
- 面板上的数字序号取运行时**生效值**（配置覆盖之后的值）

## 按键与提交语义（对齐 librime）

- 组合中的可打印字符（如大写字母）**先提交当前组合、再交应用**：宿主消费该键并以 `forwardKey` \
  重发，于是客户端先收到提交、后收到按键
- **例外**是布局转换键（如系统 colemak + 方案 us）：交回核心处理，由核心提交**转换后**的字符
- 鼠标点击候选经 `hux_engine_select_candidate` 按**全局索引**选中并上屏，路径与空格确认链相同
- 提交点学习覆盖核心路径与**宿主自发提交**（组合中的大写字母、候选点击），行为与 librime 一致

## 反查

- **共同机制**：两查都是「触发键推入组合」；仅当触发键为**单字符键**（无 Ctrl/Alt/Super）时 \
  才给出默认可上屏候选，触发字符按标点表取半 / 全角、空格上屏，带修饰键不给默认候选
- **音反查**：输入拼音（支持拼写缩写）出虎码候选，预编辑按音节切分： \
  `` `zhongguo `` → `` `zhong guo ``
  - 段内 `'` 是**音节分隔符**：匹配拼写键时透明跳过、但**强制断音**——音节与尾部补全都不许 \
    跨过分隔符，用来消歧（`` `xi'an `` 只按 `xi` + `an` 切分）
  - 分隔符在预编辑里原样保留、输入当场可见（`` `zh'guo `` → `` `zh'guo ``，段首 / 段尾同样 \
    保留）；连续输入只保留第一个，多余的丢弃、不录入
  - 计分对齐 librime 的词典反查：缩写罚 `log 0.5`、全拼可达时缩写剪枝、补全罚 `log 0.05`、 \
    排序 = 可信度 + `ln(权重)`、上限 20
- **字反查**：取应用侧周边文本，显示光标左侧 1 个字；周边文本不可用时两排为空、不提示
  - 上排（排头「咅」）是拼音，下排（排头「虍」）是虎码；多音 / 多码以 `/` 连接，缺数据写 `?`
  - ←/→/↑/↓ 交应用处理，本层不消费；查码段**不下发预编辑**，避免 marked text 锁住光标
  - Esc、再次触发或其它键退出；展示面是面板辅助文本条（auxUp / auxDown）

## 会话与提交

- 会话按输入上下文隔离：每个 `InputContext` 一份，暂存隔离、选项为引擎级
- 失焦时由 fcitx5 核心 / 前端提交客户端预编辑
- 提交是**原文提交**（fcitx5 惯例，不保留组合）
- 切换输入法 / 重置由本层**直接丢弃、不提交**；上游默认为「切换时提交」，本实现有意取丢弃契约

## 配置写入、热键与诊断

- **配置页保存的落盘归 addon**：D-Bus 的 `Controller1::SetConfig` 只调 `setConfig`， \
  **不代写配置文件**；本层因此自行 `safeSaveAsIni` 并实现 `reloadConfig()`
- `reloadConfig()` = `readAsIni` + `applyConfig`；缺任一步，改动会被下次启动的旧值压回
- 压回依据是 `adoptStoredRuntimeOptions`：以「文件里显式写过」的键为权威
- **热键必须绑到有名字的键**：音反查 / 字反查 / 上翻页 / 下翻页四项经 ABI 以 `keysym + 状态位` \
  交给引擎，按 librime 键名表解释
- 绑到没有名字的 keysym（媒体键、厂商扩展键如 `XF86AudioPlay`）**会被丢弃**；引擎不再静默， \
  而是把 `hotkeys: 忽略无法识别的绑定 <角色>=<键名>` 写进状态串，本层在应用设置后落日志 \
  （`FCITX_INFO`）
- 请绑常用键：字母、数字、`minus`、`bracketleft`、`Page_Up`
- **诊断前缀**：构造期 `dirs:` / `lexicon:` / `model:` / `punct:` / `learning:`，运行期 `config:` \
  （角色缺失 / 类型不符）、`options:`（`options.yaml` 保存失败）、`learning:`（学习库写入失败） \
  与 `hotkeys:`（无法识别的绑定）
- **状态串（`hux_engine_status`）指针不长期有效**：只在下一次状态刷新前有效——选项保存失败、 \
  配置诊断、学习库错误与热键诊断都会替换内部串；需要时重新调用，本层只在构造与应用设置后读取
  > 排查报障先看 `journalctl -t fcitx5 | grep 'hux:'`

## 状态菜单与模型信息

- 菜单里有**引擎角色开关**（`HUX_OPTION_*`，经 `hux_engine_set_option` 切换）与**宿主项** \
  （「候选窗口显示预编辑」「重新部署」，语义见 [`../../docs/config.md`](../../docs/config.md)）， \
  以及两个三态子菜单「提前上屏」「标点映射」——**模型信息并入首项「虎虚」**，点击打开模型所在目录
- 选项的另一半在配置页：`apply_settings` 把设置值写回 `options.yaml`，本层翻转后镜像进 \
  `conf/hux.conf` 并落盘，两侧互不压制
- 启动时**没写过**的共享键沿用 `options.yaml`（`adoptStoredRuntimeOptions`）
- **重新部署**走 `hux_engine_redeploy`：重走构造期读取、重置全部会话、重推设置与面板；重置时 \
  **会话 id 不变**
- 图形 schema（`HuxConfig` + `ToolTipAnnotation`）经 ABI 应用，入口 `hux_engine_apply_settings`； \
  两个三态子菜单由 `applyConfig()` 收口，装载结果由 `hux_engine_data_info` 写日志
- 模型信息取 `hux_engine_model_info`，细节面由 `Scheme::model_info` / \
  `Scheme::model_detail` 给出，本层**不解析**
- `hux_engine_model_path` 供打开模型目录，该指针在**下一次重新部署前**有效

## 生命周期加固

- addon 实例（含本引擎）**先于** `InputContext` 析构，而候选列表与状态区条目都归 IC，引擎释放后 \
  UI 仍可能持有其指针，两处加固：
  - `~HuxEngine` 对每个 IC 调 `clearGroup(StatusGroup::InputMethod)`，摘掉状态区里的 \
    `&menuAction_`
  - `HuxCandidateWord` 改持 `TrackableObjectReference<HuxEngine>`，引用失效时 `select()` \
    直接返回

## 版本下限

- CMake 明示 fcitx5 **≥ 5.1.15**：配置项要用指定初始化构造 `Option(OptionParameters)`，该构造 \
  5.1.15 才有；候选注释 `CandidateWord::setComment` 需 ≥ 5.1.9
- 低于下限时 `find_package` 在 configure 期报错；更早版本**不受支持、不参与 CI**
- CI 的发行版与 apt 版本见 [`platform/linux/README.md`](../linux/README.md)
