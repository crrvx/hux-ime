// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! 交互层（2b）：会话状态、锁、早提交、translator/filters 与学习（暂存 + 提交通知器），
//! 对应参照 `tiger_sentence.lua` 的状态段（`fresh_transient_state`…`ends_with_digit`）、
//! 证据/追踪器、早提交、`translator`、filters 与 `learning_selection`/`learning_commit` 系列。
//!
//! 说明：
//! - 参照的 `env` 瞬态状态在 Rust 由调用方持有 [`SentenceState`]（每会话一份）；
//! - 参照的 decode 增量缓存属性能优化，本移植的解码为无状态冷路径，
//!   `invalidate_edit_state` 因此只处理锁与瞬态标记（语义一致）；
//! - 上下文属性层只保留缓冲前缀 `K_BUFFERED`：写侧只有 `state.rs` 的 `save`（与内核布尔标记
//!   同处写出），读侧是方案自身的 `state.rs::buffered_text`（`select` / `translate` 与
//!   `Scheme::buffered_text` 经它回读）。内核判据是同一处 `Context::set_buffered(…)` 的
//!   `is_buffered()` 标记，**不读本属性**；`early_commit` / `learning_glue` / `processor` 读的是
//!   状态字段 `SentenceState::buffered_text`——不写成属性。会话状态
//!   （已确认 `raw`/`text`、锁帧）**不再**写成私有属性快照：参照每次入口从属性重读是因为
//!   Lua `env` 无状态，本仓的会话状态由方案对象持有；`load`/`read_locks` 解析、旧属性迁移与
//!   `committed`/`locks` 写侧无生产调用者，已删除。

use crate::char_to_sound_shape;
use crate::decode::{
    DecodeLock, Decoder, Evaluated, Evidence, candidate_is_composed_only, candidate_is_direct,
};
use crate::lexicon::Lexicon;
use crate::sound_to_char_shape;
use hashbrown::{HashMap, HashSet};
use hux_core::collections::Map;
use hux_core::key::{K_ALT_MASK, K_CONTROL_MASK, K_SUPER_MASK, KeyEvent};
use hux_core::learning::{self, DiffEvent, DiffItem, DiffPathNode, Event};
use hux_core::punct::PunctTable;
use hux_core::session::{Candidate, Composition, Context, Segment};

mod early_commit;
mod keys;
mod learning_glue;
mod processor;
mod select;
mod state;
mod translate;

/// 选项名（对应参照 `allow_duplicate_single_option`）。
pub(crate) const OPTION_ALLOW_DUPLICATE_SINGLE: &str = "tiger_sentence_allow_duplicate_single";
/// 参照 `max_raw_length`：实时输入上限（超出则不接收普通字符）。
pub(crate) const MAX_RAW_LENGTH: usize = 128;
/// 提前上屏到预编辑的选项（参照同名字符串）。
pub const OPTION_EARLY_COMMIT_TO_PREEDIT: &str = "tiger_sentence_early_commit_to_preedit";
/// 提前上屏总开关。
pub const OPTION_EARLY_COMMIT: &str = "tiger_sentence_early_commit";
/// 数字直选（addon 扩展）：菜单可见时数字直接上屏当前页候选（1–9；0=10）。
pub const OPTION_DIGIT_SELECT: &str = "tiger_sentence_digit_select";
/// 启用全字集（addon 扩展）：关掉只装主表码表，不装追加码表（`tiger_sentence.codes.<name>.txt`）。
pub(crate) const OPTION_FULL_CHARSET: &str = "tiger_sentence_full_charset";
/// 过滤非汉字（addon 扩展）：追加码表里的部首/笔画/注音/假名等不入词库（主表行不受影响）。
pub(crate) const OPTION_FILTER_NON_HAN: &str = "tiger_sentence_filter_non_han";

/// 候选类型：缓冲态整句候选（`translate` 产出、`select` 识别）。
pub(crate) const KIND_SENTENCE_BUFFERED: &str = "sentence_buffered";
/// 候选类型：缓冲候选上屏前并入缓冲前缀后的改写值（`select` 改写 `kind`）。
pub(crate) const KIND_SENTENCE_BUFFERED_COMMIT: &str = "sentence_buffered_commit";

// ---------------------------------------------------------------- crate 外可见面
//
// 只有集成测试 `tests/key_sequence_differential.rs` 用到的项是 crate 公开 API；其余交互层项
// 仅 crate 内可见（`platform/` 只依赖 `scheme::{…}`），故不再用 glob 把整层平铺出去。
pub use keys::K_SOUND_TO_CHAR_SHAPE_KEY;
pub use learning_glue::LiveLearning;
pub use processor::{ProcessorEnv, ProcessorResult, process_key_event};
pub use state::SentenceState;
pub use translate::{CompositionBuilder, update_notifier};

pub(crate) use early_commit::*;
pub(crate) use keys::*;
pub(crate) use learning_glue::*;
pub(crate) use processor::*;
pub(crate) use select::*;
pub(crate) use state::*;
pub(crate) use translate::*;

#[cfg(test)]
mod tests;
