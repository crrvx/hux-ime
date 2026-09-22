// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;

/// tracker 键分隔符（参照 `state_separator`）。
pub(crate) const STATE_SEPARATOR: &str = "\u{1f}";

/// 证据追踪器（对应参照 tracker 表）。
#[derive(Clone, Debug)]
pub struct Tracker {
    pub text: String,
    pub text_char_count: usize,
    pub raw_length: usize,
    pub evidence_count: usize,
    pub strong_count: usize,
    pub gap_count: usize,
    pub last_share: f64,
}

/// 空码自动上屏的待定候选（对应参照 `empty_code_pending`）。
#[derive(Clone, Debug)]
pub struct EmptyCodePending {
    pub candidate_text: String,
    pub requires_uniqueness_check: bool,
    pub committed_text: String,
    pub base_raw_length: usize,
    pub last_segment_start: usize,
}

/// 已确认锁定段（对应参照 lock 表 `{raw, text, boundaries}`）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Lock {
    pub raw: String,
    pub text: String,
    pub boundaries: String,
}

/// 会话状态（对应参照 `sentence_state` 的瞬态 + 已确认字段）。
#[derive(Clone, Debug)]
pub struct SentenceState {
    pub committed_text: String,
    pub committed_raw: String,
    pub buffered_text: String,
    pub locks: Vec<Lock>,
    pub trackers: Map<String, Tracker>,
    pub empty_code_pending: Option<EmptyCodePending>,
    pub last_seen_raw: String,
    pub last_auto_commit_raw_length: usize,
    pub suspended: bool,
    pub continuation_after_auto_commit: bool,
    pub tab_pending: bool,
    pub model_generation: u64,
}

impl SentenceState {
    /// 参照 `fresh_transient_state`。
    pub fn fresh(model_generation: u64) -> Self {
        Self {
            committed_text: String::new(),
            committed_raw: String::new(),
            buffered_text: String::new(),
            locks: Vec::new(),
            trackers: Map::new(),
            empty_code_pending: None,
            last_seen_raw: String::new(),
            last_auto_commit_raw_length: 0,
            suspended: false,
            continuation_after_auto_commit: false,
            tab_pending: false,
            model_generation,
        }
    }

    pub fn active_lock(&self) -> Option<&Lock> {
        self.locks.last()
    }

    /// 参照 `synchronize_model_state`：模型代次变化时清空解码相关瞬态。
    pub fn synchronize_model_state(&mut self, generation: u64) -> bool {
        if self.model_generation == generation {
            return false;
        }
        self.model_generation = generation;
        self.trackers.clear();
        self.last_seen_raw.clear();
        self.empty_code_pending = None;
        true
    }

    /// 参照 `save_sentence_state`：把**缓冲前缀**写进上下文属性。
    ///
    /// 会话状态（已确认 `raw`/`text`、锁帧）由调用方持有的 [`SentenceState`] 承载，**不经属性
    /// 往返**（本仓 `TigerScheme` 每会话一份状态，参照的 Lua `env` 才是无状态、每次入口重读）。
    /// 属性层因此只剩「宿主/内核也要读」的一项：缓冲前缀 [`K_BUFFERED`]——
    /// [`buffered_text`] 在 `select` / `early_commit` / `learning_glue` 都读它，
    /// 内核视图（`live_input` / `live_caret`）在同一处 `set_buffered` 同步。
    ///
    /// 只写不读的 `tiger_sentence_committed` / `tiger_sentence_locks` 快照与旧属性清理已随
    /// `load`/`read_locks` 一并删除（无 FFI / 平台 / C++ 侧读取方）。
    pub fn save(&mut self, context: &mut Context) {
        set_property_if_changed(context, K_BUFFERED, &self.buffered_text.clone());
        context.set_buffered(!self.buffered_text.is_empty());
    }

    /// 参照 `reset_sentence_state`。
    pub fn reset(&mut self, context: &mut Context, continuation_after_auto_commit: bool) {
        let fresh = SentenceState {
            continuation_after_auto_commit,
            ..SentenceState::fresh(self.model_generation)
        };
        *self = fresh;
        self.save(context);
    }
}

/// 参照 `set_property_if_changed`（通用属性助手，定义在 core `session`）。
pub use hux_core::session::set_property_if_changed;

/// 参照 `buffered_text`。
pub fn buffered_text(context: &Context) -> String {
    context.get_property(K_BUFFERED).unwrap_or("").to_string()
}

/// 参照 `live_input`：缓冲态下去除私有 `~` 前缀。
pub fn live_input(context: &Context) -> Vec<u8> {
    let value = context.input();
    if !buffered_text(context).is_empty() && value.first() == Some(&b'~') {
        return value[1..].to_vec();
    }
    value.to_vec()
}

/// 参照 `input_caret`（缓冲态下 caret 含标记，需减一）。
pub fn input_caret(context: &Context) -> usize {
    let length = live_input(context).len();
    let caret = if !buffered_text(context).is_empty() {
        context.caret().saturating_sub(1)
    } else {
        context.caret()
    };
    caret.min(length)
}

/// 参照 `restore_composition_input`。
pub fn restore_composition_input(context: &mut Context, value: &[u8]) {
    let buffered = !buffered_text(context).is_empty();
    let mut target = Vec::with_capacity(value.len() + 1);
    if buffered {
        target.push(b'~');
    }
    target.extend_from_slice(value);
    if context.input() == target.as_slice() && buffered {
        // 纯文本删除只改前缀、原始输入仍为 `~`：需要让翻译失效而不清空组合。
        if context.refresh_non_confirmed_composition() {
            return;
        }
    }
    context.set_input(&target);
}

/// 参照 `cycle_candidate_highlight`：环绕循环导航（step 可为负）。
pub fn cycle_candidate_highlight(context: &mut Context, step: i64) -> bool {
    if !context.has_menu() {
        return false;
    }
    let (selected, count) = {
        let Some(segment) = context.composition.back() else {
            return false;
        };
        (segment.selected_index, segment.prepare(CANDIDATE_LIMIT))
    };
    if count == 0 {
        return false;
    }
    let target = ((selected as i64 + step).rem_euclid(count as i64)) as usize;
    if context.highlight(target) {
        return true;
    }
    // 兼容回退：直接写 `selected_index`（librime-lua 暴露为可写字段）。
    let Some(segment) = context.composition.back_mut() else {
        return false;
    };
    segment.selected_index = target;
    segment.selected_index == target
}

/// 参照 `set_allow_duplicate_single`：读取选项（缺省 true，仅显式关闭时为 false）。
pub fn set_allow_duplicate_single(context: &Context) -> bool {
    context.get_option_or(OPTION_ALLOW_DUPLICATE_SINGLE, true)
}

/// 参照 `invalidate_edit_state`：清除编辑相关瞬态并回退受影响的锁。
pub fn invalidate_edit_state(
    context: &mut Context,
    state: &mut SentenceState,
    first_changed: usize,
    full_length: usize,
) {
    state.tab_pending = false;
    state.trackers.clear();
    state.last_seen_raw.clear();
    state.empty_code_pending = None;
    // 编辑已锁定但未提交的范围会让该锁及后续锁失效；删除到其边界同样解锁；
    // 已提交到应用的文本对应的锁绝不丢弃。
    while let Some(lock) = state.locks.last() {
        if lock.raw.len() > state.committed_raw.len()
            && (first_changed < lock.raw.len() || full_length <= lock.raw.len())
        {
            state.locks.pop();
        } else {
            break;
        }
    }
    state.save(context);
}
