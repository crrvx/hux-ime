// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! 学习事件编排（参照 `tiger_sentence.lua` 的 `learning_selection` /
//! `learning_stage` / `learning_submit` / `learning_commit` 系列）：
//! 选取学习目标、暂存差异事件，并在提交点无条件消费 `pending`。
//!
//! 子模块分工：
//! - [`selection`]：学习目标与融合竞争者的取值（参照 `learning_selection`）；
//! - [`stage`]：融合事件与差异事件的暂存（参照 `learning_stage`）；
//! - [`commit`]：提交点的筛选、消费与通知器（参照 `learning_submit` / `learning_commit`）。

use super::*;

mod commit;
mod selection;
mod stage;

pub(crate) use commit::*;
pub(crate) use selection::*;
pub(crate) use stage::*;

// ---------------------------------------------------------------- 学习暂存

/// 交互层学习暂存（对应参照 `env._tiger_learning` 的暂存字段；存储/索引归宿主层）。
#[derive(Clone, Debug, Default)]
pub struct LiveLearning {
    pub mode: String,
    pub pending: Vec<DiffEvent>,
    pub baseline: Option<Selected>,
    pub submitted_raw: Option<String>,
    pub hide_owned: bool,
    /// 参照 `learned.store and learned.store.db`（宿主学习库就绪后置位）。
    pub store_ready: bool,
    /// 提交点接受的学习事件（`learning::Event`）：核心提交路径与宿主
    /// [`learning_commit`] 调用均入此队列，等待宿主持久化（宿主排空后落库）。
    pub submitted: Vec<Event>,
}
