<!-- SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com> -->
<!-- SPDX-License-Identifier: GPL-3.0-or-later -->

# platform/fcitx5（K3）

hux-ime（虎虚）fcitx5 平台适配：**C++ 薄壳**（`shell/`，只做 fcitx5 接口适配）+ **Rust 组装**（`src/`）。
C ABI 契约在 [`../../crates/hux-ffi/`](../../crates/hux-ffi/)（CI 校验 `hux_abi.h` 声明 ↔ `libhux.so` 导出一致）；
逻辑在 [`../../crates/hux-core/`](../../crates/hux-core/) 与 `hux-cfg`。按键 → core（`processor`/`translate`）→ 提交 / preedit / 候选 → fcitx5。
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

## 已知限制

会话按输入上下文隔离；失焦时由 fcitx5 核心/前端把客户端预编辑以**原文提交**（fcitx5 惯例，
不保留组合）；切换输入法/重置由本层**直接丢弃**（不提交）。上游默认在切换输入法时提交
候选/预编辑，本实现有意取「丢弃」契约；打包待做。
