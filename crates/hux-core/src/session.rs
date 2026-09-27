// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! 会话运行时：实现参照交互层依赖的 librime Context/Composition/Menu 子集。
//!
//! 对齐点（与参照用法一一对应）：
//! - 输入为**字节串**；`caret` 为字节偏移（0 = 最前）。
//! - `input` 可带私有缓冲标记 `~`（`buffered` 属性非空时），`live_input` 去除。
//! - 菜单：段内候选列表 + `selected_index`；`highlight` 为**绝对**索引：
//!   截断到 `count-1`，且索引未变化时返回 false（librime `Context::Highlight`）。
//! - `confirm_current_selection`：标记末段为选中（librime 语义），随后的 `commit`
//!   触发提交事件（事件含提交文本）并清空组合。
//! - 事件（update/commit/option）入队；调用方在每个操作后 `drain_events()`，
//!   与参照的同步 notifier 在可观测行为上等价。
//! - `last_commit` 保留最近一次组合提交文本，供诊断；参照的 `get_commit_text()`
//!   为即时计算（任何时刻可读），跨实现一律以 [`Event::Commit`] 携带的文本为准。
//! - 属性写入不产生事件（参照未使用 `property_update_notifier`）。
//! - 组合重建（分段/翻译/过滤）由**方案侧**交互层负责（`hux-scheme/*`）；
//!   本模块只提供 Context/Composition/Menu 子集，不感知任何方案。
//!
//! 结构：本模块保留模块文档与公开面（`pub use` 重导出）；`composition` 子模块承载
//! 候选/段/组合，`context` 子模块承载事件与输入上下文，单测就近放在各自子模块下。

mod composition;
mod context;

pub use composition::{Candidate, Composition, Segment};
pub use context::{Context, Event, set_property_if_changed};
