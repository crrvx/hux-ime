// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! 学习索引的数据模型：事件、分区/汇总/前缀项、索引本体与容量常量。

use crate::cache::Fifo;
use hashbrown::HashMap;
use std::rc::Rc;

/// 参照 `MAX_LEVEL`：单条 `(code, mode, context)` 纠错的最大累积等级。
pub(super) const MAX_LEVEL: f64 = 10.0;
pub(super) const MATERIALIZED_CODE_LIMIT: usize = 256;
pub(super) const PREFIX_QUERY_LIMIT: usize = 4096;
pub(super) const CODE_WINDOW_LIMIT: usize = 2048;
pub(super) const CODE_WINDOW_SLOTS: usize = 64;

#[derive(Clone, Debug)]
pub struct Event {
    pub time: f64,
    pub mode: String,
    pub code: String,
    pub text: String,
    pub context: String,
}

#[derive(Clone, Copy)]
pub(super) struct Choice {
    /// 累积纠错次数（等级），上限 `MAX_LEVEL`。
    pub(super) weight: f64,
}

#[derive(Clone)]
pub(super) struct Group {
    pub(super) code: String,
    pub(super) mode: String,
    pub(super) context: String,
    pub(super) choices: HashMap<String, Choice>,
}

#[derive(Clone)]
pub(super) struct Summary {
    pub(super) mode: String,
    pub(super) text: String,
    pub(super) exact: HashMap<String, f64>,
    pub(super) weight: f64,
    pub(super) general: f64,
}

#[derive(Clone)]
pub(super) struct PrefixEntry {
    pub(super) general: f64,
    pub(super) exact: HashMap<String, f64>,
}

#[derive(Default)]
pub(super) struct Materialized {
    pub(super) exact: HashMap<String, Summary>,
    pub(super) prefixes: HashMap<String, PrefixEntry>,
}

/// 奖励路径链节点（对应参照 `previous` 链上的一个状态）。
#[derive(Clone, Debug)]
pub struct RewardNode {
    pub text_length: usize,
    pub raw_length: usize,
    pub learning_score: f64,
    /// 参照 `learning_early_commit_bonus`（个性化早提交置信度的学习分量）。
    pub learning_early_commit_bonus: f64,
}

/// diff 路径节点。
#[derive(Clone, Debug)]
pub struct DiffPathNode {
    pub raw_length: usize,
    pub text_length: usize,
}

/// diff 候选：文本 + 路径链（`path[0]` 为最外层节点）。
#[derive(Clone, Debug)]
pub struct DiffItem {
    pub text: String,
    pub path: Vec<DiffPathNode>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct DiffEvent {
    /// 参照 `diff` 内 `time=os.time()`；由调用方传入（金样不比对）。
    pub time: f64,
    pub mode: String,
    pub code: String,
    pub text: String,
    pub context: String,
    pub raw_start: usize,
    pub raw_end: usize,
    pub text_start: usize,
    pub text_end: usize,
}

/// 学习索引：`build` 形态（全量重放）或运行时形态（分区 + 物化缓存）。
#[derive(Clone)]
pub struct LearningIndex {
    pub codes: Vec<String>,
    /// 构建索引的时刻（平台用作 epoch；参照 `M.*` 的 `now` 元数据）。
    pub now: f64,
    /// **参照遗留元数据**：`M.runtime_index` 记录的「最大事件时间」，
    /// `update_index` 原样带过，本仓无消费者（保留以维持与参照 `runtime_index`
    /// 的字段同构，便于后续差分核对；时间语义见模块文档）。
    pub future: f64,
    pub(super) partitions: Option<HashMap<String, HashMap<String, Group>>>,
    pub(super) exact: Option<HashMap<String, Summary>>,
    pub(super) prefixes: Option<HashMap<String, PrefixEntry>>,
    pub(super) cache: Fifo<String, Rc<Materialized>>,
    pub(super) prefix_queries: Fifo<String, f64>,
    pub(super) code_windows: Fifo<String, (usize, isize)>,
}
