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
    pub trackers: HashMap<String, Tracker>,
    pub empty_code_pending: Option<EmptyCodePending>,
    pub last_seen_raw: String,
    pub last_auto_commit_raw_length: usize,
    pub suspended: bool,
    pub continuation_after_auto_commit: bool,
    pub tab_pending: bool,
    pub model_generation: u64,
    /// 旧属性一次性迁移是否已完成（参照 `env._tiger_sentence_legacy_cleared`）。
    pub legacy_cleared: bool,
}

impl SentenceState {
    /// 参照 `fresh_transient_state`。
    pub fn fresh(model_generation: u64) -> Self {
        Self {
            committed_text: String::new(),
            committed_raw: String::new(),
            buffered_text: String::new(),
            locks: Vec::new(),
            trackers: HashMap::new(),
            empty_code_pending: None,
            last_seen_raw: String::new(),
            last_auto_commit_raw_length: 0,
            suspended: false,
            continuation_after_auto_commit: false,
            tab_pending: false,
            model_generation,
            legacy_cleared: false,
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

    /// 参照 `sentence_state`：从 context 属性同步已确认/缓冲/锁。
    pub fn load(&mut self, context: &mut Context, model_generation: u64) {
        let combined = context.get_property(K_COMMITTED).unwrap_or("").to_string();
        let (mut committed_raw, mut committed_text) = parse_committed_property(&combined);
        if combined.is_empty() {
            // 旧双属性格式的一次性迁移：写回合并属性并清空旧键。
            let old_raw = context
                .get_property(K_COMMITTED_RAW_LEGACY)
                .unwrap_or("")
                .to_string();
            let old_text = context
                .get_property(K_COMMITTED_TEXT_LEGACY)
                .unwrap_or("")
                .to_string();
            if !old_raw.is_empty() || !old_text.is_empty() {
                committed_raw = old_raw.clone();
                committed_text = old_text.clone();
                set_property_if_changed(context, K_COMMITTED, &format!("{old_raw}\t{old_text}"));
                set_property_if_changed(context, K_COMMITTED_RAW_LEGACY, "");
                set_property_if_changed(context, K_COMMITTED_TEXT_LEGACY, "");
            }
        }
        self.synchronize_model_state(model_generation);
        self.committed_text = committed_text;
        self.committed_raw = committed_raw;
        self.buffered_text = context.get_property(K_BUFFERED).unwrap_or("").to_string();
        self.locks = read_locks(context);
    }

    /// 参照 `save_sentence_state`（含旧属性一次性清理）。
    pub fn save(&mut self, context: &mut Context) {
        set_property_if_changed(context, K_BUFFERED, &self.buffered_text.clone());
        save_locks(context, &self.locks.clone());
        let combined = format!("{}\t{}", self.committed_raw, self.committed_text);
        set_property_if_changed(context, K_COMMITTED, &combined);
        if !self.legacy_cleared {
            for key in [
                K_CONFIDENCE_LEGACY,
                K_PROPOSAL_LEGACY,
                K_STABLE_LEGACY,
                K_EVIDENCE_RAW_LEGACY,
            ] {
                set_property_if_changed(context, key, "");
            }
            self.legacy_cleared = true;
        }
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

/// 参照 `set_property_if_changed`。
pub fn set_property_if_changed(context: &mut Context, key: &str, value: &str) {
    if context.get_property(key).unwrap_or("") != value {
        context.set_property(key, value);
    }
}

/// 参照 `parse_committed_property`：无制表符时返回空对。
pub fn parse_committed_property(value: &str) -> (String, String) {
    match value.split_once('\t') {
        Some((raw, text)) => (raw.to_string(), text.to_string()),
        None => (String::new(), String::new()),
    }
}

/// 参照 `read_locks`：长度前缀帧 `#field:field`，每 3 个字段一个锁。
pub fn read_locks(context: &Context) -> Vec<Lock> {
    let data = context.get_property(K_LOCKS).unwrap_or("");
    let bytes = data.as_bytes();
    let mut fields: Vec<String> = Vec::new();
    let mut offset = 0usize;
    while offset < bytes.len() {
        let Some(colon) = bytes[offset..].iter().position(|byte| *byte == b':') else {
            return Vec::new();
        };
        let colon = offset + colon;
        let Ok(length) = std::str::from_utf8(&bytes[offset..colon])
            .unwrap_or("")
            .parse::<usize>()
        else {
            return Vec::new();
        };
        let Some(end) = colon
            .checked_add(1)
            .and_then(|value| value.checked_add(length))
            .filter(|end| *end <= bytes.len())
        else {
            return Vec::new();
        };
        fields.push(String::from_utf8_lossy(&bytes[colon + 1..end]).into_owned());
        offset = end;
    }
    let mut locks = Vec::new();
    for chunk in fields.chunks_exact(3) {
        locks.push(Lock {
            raw: chunk[0].clone(),
            text: chunk[1].clone(),
            boundaries: chunk[2].clone(),
        });
    }
    locks
}

/// 参照 `save_locks`。
pub fn save_locks(context: &mut Context, locks: &[Lock]) {
    let mut framed = String::new();
    for lock in locks {
        for field in [
            lock.raw.as_str(),
            lock.text.as_str(),
            lock.boundaries.as_str(),
        ] {
            framed.push_str(&format!("{}:{}", field.len(), field));
        }
    }
    set_property_if_changed(context, K_LOCKS, &framed);
}

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
