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
//! - 上下文属性层只保留**宿主与内核共享**的缓冲前缀（`K_BUFFERED`：`select` /
//!   `early_commit` / `learning_glue` 与 `Context::is_buffered` 都读它）。会话状态
//!   （已确认 `raw`/`text`、锁帧）**不再**写成私有属性快照：参照每次入口从属性重读是因为
//!   Lua `env` 无状态，本仓的会话状态由方案对象持有；原先的 `load`/`read_locks` 解析、
//!   旧属性迁移与 `committed`/`locks` 写侧因此无生产调用者（也无 FFI / 平台 / C++ 读取方），
//!   已在复核整改 3b（A3）删除，见 `docs/refactor.md` §8。

use crate::char_to_sound_shape;
use crate::decode::{
    DecodeLock, Decoder, Evaluated, Evidence, candidate_is_composed_only, candidate_is_direct,
};
use crate::lexicon::Lexicon;
use crate::sound_to_char_shape;
use hashbrown::{HashMap, HashSet};
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
pub const OPTION_ALLOW_DUPLICATE_SINGLE: &str = "tiger_sentence_allow_duplicate_single";
/// 候选上限（参照 `candidate_limit`）：单一来源在 [`crate::decode::CANDIDATE_LIMIT`]。
pub use crate::decode::CANDIDATE_LIMIT;

/// 参照 `max_raw_length`：实时输入上限（超出则不接收普通字符）。
pub const MAX_RAW_LENGTH: usize = 128;
/// 提前上屏到预编辑的选项（参照同名字符串）。
pub const OPTION_EARLY_COMMIT_TO_PREEDIT: &str = "tiger_sentence_early_commit_to_preedit";
/// 提前上屏总开关。
pub const OPTION_EARLY_COMMIT: &str = "tiger_sentence_early_commit";
/// 数字直选（addon 扩展）：菜单可见时数字直接上屏当前页候选（1–9；0=10）。
pub const OPTION_DIGIT_SELECT: &str = "tiger_sentence_digit_select";

pub use early_commit::*;
pub use keys::*;
pub use learning_glue::*;
pub use processor::*;
pub use select::*;
pub use state::*;
pub use translate::*;

#[cfg(test)]
mod tests;
