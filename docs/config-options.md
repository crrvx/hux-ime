<!-- SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com> -->
<!-- SPDX-License-Identifier: GPL-3.0-or-later -->

# 可配置项扩展（开发文档：A 组）

范围：**低成本、常用**的四项（下称 A 组），现行为即默认值，改后即时生效。其余项见 §5 待定。

## 1. 本批项目

| # | 配置项 | 取值 / 默认 | UI 区 | 说明 |
| --- | --- | --- | --- | --- |
| 1 | 候选布局 `CandidateLayout` | `Horizontal`（默认）/ `Vertical` | 行为 | 候选窗排列与选择键语义（←→ 或 ↑↓） |
| 2 | 翻页循环 `PageDownCycle` | 关（默认）/ 开 | 行为 | 末页再翻回首页、首页向上翻到末页（参照 `menu/page_down_cycle`，默认关） |
| 3 | 预编辑内容 `PreeditMode` | `CandidateCode`（默认，现状）/ `RawInput` / `Hidden` | 行为 | 候选分码（高亮候选分码+原文尾部）/ 原始输入 / 不显示 |
| 4 | 提前上屏最短保留码数 `MinRetainedRawLength` | `0`（默认，不额外限制）/ `0..=20` | 行为 | 对照参照 `tiger_sentence/min_retained_raw_length`；概率型早提交仍不少于 3 |

## 2. 逐项实现点

### 1. 候选布局
- C++：`HuxConfig` 新增枚举选项（`FCITX_CONFIG_ENUM` + 中英文名）；`applyUpdate` 里
  `candidateList->setLayoutHint(fcitx::CandidateLayoutHint::Vertical|Horizontal)`。
- Rust：`Settings.candidate_layout` → 写入各会话 context 的 `_vertical` / `_horizontal` / `_linear`
  （host `selector` 已读取这些选项决定 ↑↓/←→ 选择语义；默认 = 现状「Horizontal | Stacked」）。
- 生效时机：下一次 UI 更新（无需重启）。

### 2. 翻页循环
- `HostOptions` 增 `page_cycle: bool`（由 `Settings` 派生）；`host.rs::selector_action` 的
  `NextPage`/`PreviousPage` 加循环分支（`NextPage` 到末页 → 回 0；`PreviousPage` 到首页 → 末页）。
- 仅影响键盘翻页（Page_Up/Page_Down 与配置页键）；面板箭头翻页仍交给 fcitx5。

### 3. 预编辑内容
- `lib.rs::push_update` 按模式分支：
  - `CandidateCode`＝现状（高亮候选 `preedit` 优先 + 组合后原文尾部）；
  - `RawInput`＝缓冲 + 实况输入（不按候选分码）；
  - `Hidden`＝空。
- 反查段（字反查）仍不下发预编辑，维持既有约束。

### 4. 最短保留码数
- `Settings.min_retained_raw_length`（钳制 `0..=20`）→ `HuxOptions` → 写入会话；
- core 管线已具备（`ProcessorEnv.min_retained` → `min_retained_raw_length()`，
  参照语义：`0` = 不额外限制；概率型早提交下限 3 不变）。

## 3. 统一改动套路（四项共用）

1. `shell/hux.cpp`：schema（「行为」区）+ 注解 → `applyConfig()` 填 ABI；
2. `shell/hux_abi.h` + `src/lib.rs::HuxOptions` + `src/settings.rs`：追加字段（C 布局只能追加）；
3. 需要时改 core（本批仅 #2 `host.rs`、#4 会话字段）；
4. 测试：Rust 单测（默认值 / 边界 / 行为）；**默认值必须等于现行为**（键序列金样与既有测试不变）；
5. 文档：`docs/config.md` 表格补行；本文件勾选完成项。

## 4. 顺序与验收

顺序：**#1 → #3 → #2 → #4**（每项独立提交，便于回退）。

验收：
- 配置页四项可改、即改即生效（下一次按键/更新），重启后保持（写入 `~/.config/fcitx5/conf/hux.conf`）；
- #1 竖排时 ↑↓ 选择、←→ 移动语义一致；横排保持现状；
- #2 末页/首页循环正确，默认关时行为与现状一致；
- #3 三态在普通组合、缓冲态、音反查下表现正确；
- #4 边界 0/20 钳制正确，调大后早提交更保守（单测覆盖）。

## 5. 待定（不在本批）

- **B 组（中成本）**：模型路径设置、候选选择键可配置、普通候选显示虎码注释、码表/标点表自定义路径
  （注：用户目录覆盖码表/`symbols.yaml` 已可用，先补文档）。
- **C 组（高成本）**：简繁转换（需 OpenCC 类数据）、用户词/自造词（导入导出与编辑）。
- **明确不做**：早提交概率阈值、`memory_profile`、`ascii_composer` 系列（无内置英文模式）。

> Android 适配（见 [`android.md`](android.md)）复用本批选项：Android 配置页支持 `Enum/Int/Bool/String/List|Key`，
> 本批四项无需额外适配。
