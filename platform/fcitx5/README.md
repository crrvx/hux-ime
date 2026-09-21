<!-- SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com> -->
<!-- SPDX-License-Identifier: GPL-3.0-or-later -->

# platform/fcitx5（K3）

hux-ime（虎虚）fcitx5 平台适配：**C++ 薄壳**（`shell/`，只做 fcitx5 接口适配）+ **Rust 组装**（`src/`）。
C ABI 契约在 [`../../crates/hux-ffi/`](../../crates/hux-ffi/)（CI 校验 `hux_abi.h` 声明 ↔ `libhux.so` 导出一致）；
逻辑在 [`../../crates/hux-core/`](../../crates/hux-core/)（内核）、[`../../crates/hux-scheme/tiger/`](../../crates/hux-scheme/tiger/)（虎句方案：`interaction::processor`/`translate`）与 `hux-cfg`。按键 → 方案（处理器/翻译）→ core（会话/宿主链）→ 提交 / preedit / 候选 → fcitx5。
选项键（菜单/面板）：宿主不硬编码方案选项名——`hux_engine_option_key(role)` 按 `HUX_OPTION_*`
角色向引擎取键（角色序 = `hux_cfg::roles::RUNTIME_OPTION_ROLES`，与状态菜单同源）；
键的来源是方案声明 `Scheme::option_declarations()`，装配处解析成角色表（**缺角色即报错**，
该角色无键 → 宿主跳过该项）；面板数字序号取运行时生效值。
装配方式（P4c）：本层是**装配根**——构造 `TigerScheme` 后以 `dyn Scheme` 驱动（按键 / 候选 / 重建 /
学习 / 反查全经契约），不引用方案内部模块；
桌面与 Android **共用本层**（两端同为 fcitx5；Android 接线见 [`../android/README.md`](../android/README.md)）；
数据目录与模型解析见 [`src/paths.rs`](src/paths.rs)。

按键语义与参照（librime）一致：组合中的可打印字符（如大写字母）先提交当前组合，再交应用；
为保证上屏顺序，宿主层会消费该键并以 `forwardKey` 重发——客户端先收到提交、后收到按键。
**例外**：布局转换键（系统布局与方案布局不同时，如系统 colemak + 方案 us）交回核心处理，
由核心提交**转换后**的字符；自行转发会让客户端按系统布局重新解释该键。

鼠标点击候选 = 按该候选选中并上屏（与空格确认同一条确认/学习链）；提交点学习覆盖核心路径与
宿主自发提交（如组合中的大写字母、候选点击），与参照的提交通知器一致。

安装、数据目录与使用见 [`../../docs/usage.md`](../../docs/usage.md)；
配置项见 [`../../docs/config.md`](../../docs/config.md)。

## 反查（行为契约）

音反查与字反查同机制：触发键推入组合；**仅当触发键为单字符键**（无 Ctrl/Alt/Super）时给出
默认可上屏候选（触发字符按标点表取半/全角，空格上屏），带修饰键的触发不给默认候选。

- **音反查**：输入拼音（支持拼写缩写）出虎码候选；预编辑按音节切分（`` `zhongguo `` → `` `zhong guo ``）。
- **字反查**：取应用侧周边文本（应用不可用时查不到内容、两排为空，不做提示）；两排显示光标
  左侧 1 个字——上排（排头「咅」）= 拼音、下排（排头「虍」）= 虎码（多音/多码以 `/` 连接，缺数据 `?`）；
  ←/→/↑/↓ 交应用处理（应用光标随动，本层不消费；查码段不下发预编辑，避免应用端 marked text 锁住光标）；
  Esc / 再次触发 / 其它键退出（打字照常输入）。展示面为输入面板辅助文本条（auxUp/auxDown）。

## 安装（`cmake --install` 与 `install.sh` 等价）

`cmake --install`（前缀 `/usr`）装 **3 个插件文件 + `data/MANIFEST` 列出的全部随包数据**：

- `lib/<libdir>/fcitx5/libhux.so`（`FCITX_INSTALL_ADDONDIR` 优先，Fedora 等为 `lib64`）；
- `share/fcitx5/addon/hux.conf`、`share/fcitx5/inputmethod/hux.conf`；
- `share/fcitx5/hux/`：码表四件套 + 词先验 + 拼音索引 + 标点表（`data/MANIFEST` 单一来源，
  `install.sh` 装后逐条核对、`uninstall.sh` 按同一清单删除）。

**复核整改第 4 批 F5**：此前 CMake 只装 3 个插件文件，数据仅由 `install.sh` 安装 ⇒ 只走
`cmake --install`（发行版打包 / `DESTDIR` 流程）会得到**无词库引擎**（`Lexicon`/`PunctTable`
静默降级）。现由 CMake 按 `data/MANIFEST` 一并安装；`tools/checks/check_data_manifest.sh`
与 CI 的 `DESTDIR` 步骤守护「装 / 卸 / CMake 三处清单一致」。**n-gram 模型仍不随包**（用户自取；
`uninstall.sh --purge` 才删）。

## 已知限制

会话按输入上下文隔离；失焦时由 fcitx5 核心/前端把客户端预编辑以**原文提交**（fcitx5 惯例，
不保留组合）；切换输入法/重置由本层**直接丢弃**（不提交）。上游默认在切换输入法时提交
候选/预编辑，本实现有意取「丢弃」契约；打包待做（发行版打包脚本，不影响上面的安装布局）。

- **配置页热键绑定的名字要求（复核整改第 4 批 F15）**：快捷键分区的四项（音反查 / 字反查 /
  上翻页 / 下翻页）经 ABI 以 `keysym + 状态位` 交给引擎，引擎按 librime 键名表解释。配置页若绑到
  **没有名字的 keysym**（媒体键、厂商扩展键，如 `XF86AudioPlay`），该绑定无法解析，
  **会被丢弃**——不再静默：引擎把 `hotkeys: 忽略无法识别的绑定 <角色>=<键名>` 写进状态串
  （`hux_engine_status`），本层在应用设置后把状态串落到日志（`FCITX_INFO`，`journalctl -t fcitx5` 可见）。
  请绑常用键（字母 / 数字 / `minus` / `bracketleft` / `Page_Up` …）。
- **状态串指针（F8）**：`hux_engine_status` 返回的指针**在下一次状态刷新前有效**
  （选项保存失败 / 配置诊断 / 学习库错误 / 热键诊断都会替换内部串）——每次需要时重新调用，
  不要缓存；本层只在构造与应用设置后立即读取并落日志。
- **诊断前缀**：状态串除构造期的加载说明（`dirs:` / `lexicon:` / `model:` / `punct:` / `learning:`）
  外，运行期还会出现 `config:`（配置袋角色缺失/类型不符）、`options:`（`options.yaml` 保存失败）、
  `learning:`（运行期学习库写入失败，复核整改第 4 批 F6）、`hotkeys:`（无法识别的键绑定，F15）。
  排查用户报障时先看 `journalctl -t fcitx5 | grep 'hux:'`。
