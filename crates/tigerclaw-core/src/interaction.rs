//! 交互层（2b-1）：会话状态、锁与键辅助函数，对应参照
//! `tiger_sentence.lua` 的状态段（`fresh_transient_state`…`ends_with_digit`）。
//!
//! 说明：
//! - 参照的 `env` 瞬态状态在 Rust 由调用方持有 [`SentenceState`]（每会话一份）；
//! - 参照的 decode 增量缓存属性能优化，本移植的解码为无状态冷路径，
//!   `invalidate_edit_state` 因此只处理锁与瞬态标记（语义一致）。

use crate::key::KeyEvent;
use crate::session::Context;

/// 属性键（对应参照 `state_keys`）。
pub const K_BUFFERED: &str = "tiger_sentence_buffered_text";
pub const K_LOCKS: &str = "tiger_sentence_locks";
pub const K_COMMITTED: &str = "tiger_sentence_committed";
pub const K_COMMITTED_TEXT_LEGACY: &str = "tiger_sentence_committed_text";
pub const K_COMMITTED_RAW_LEGACY: &str = "tiger_sentence_committed_raw";
pub const K_CONFIDENCE_LEGACY: &str = "tiger_sentence_confidence";
pub const K_PROPOSAL_LEGACY: &str = "tiger_sentence_proposal";
pub const K_STABLE_LEGACY: &str = "tiger_sentence_stable";
pub const K_EVIDENCE_RAW_LEGACY: &str = "tiger_sentence_evidence_raw";
pub const K_OPTIONS_ERROR: &str = "tiger_sentence_options_error";

/// 选项名（对应参照 `allow_duplicate_single_option`）。
pub const OPTION_ALLOW_DUPLICATE_SINGLE: &str = "tiger_sentence_allow_duplicate_single";
/// 候选上限（参照 `candidate_limit`）。
pub const CANDIDATE_LIMIT: usize = 20;

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
        self.last_seen_raw.clear();
        true
    }

    /// 参照 `sentence_state`：从 context 属性同步已确认/缓冲/锁。
    pub fn load(&mut self, context: &Context, model_generation: u64) {
        let combined = context.get_property(K_COMMITTED).unwrap_or("").to_string();
        let (mut committed_raw, mut committed_text) = parse_committed_property(&combined);
        if combined.is_empty() {
            // 旧双属性格式的一次性迁移。
            let old_raw = context
                .get_property(K_COMMITTED_RAW_LEGACY)
                .unwrap_or("")
                .to_string();
            let old_text = context
                .get_property(K_COMMITTED_TEXT_LEGACY)
                .unwrap_or("")
                .to_string();
            if !old_raw.is_empty() || !old_text.is_empty() {
                committed_raw = old_raw;
                committed_text = old_text;
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
        if colon + 1 + length > bytes.len() {
            return Vec::new();
        }
        fields.push(String::from_utf8_lossy(&bytes[colon + 1..colon + 1 + length]).into_owned());
        offset = colon + 1 + length;
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

/// 参照 `set_allow_duplicate_single`：读取选项（缺省 true）。
pub fn set_allow_duplicate_single(context: &Context) -> bool {
    context.get_option(OPTION_ALLOW_DUPLICATE_SINGLE)
}

/// 参照 `is_modifier_repr`：独立的修饰键事件（不消耗小数点待发状态）。
pub fn is_modifier_repr(repr: &str) -> bool {
    repr.starts_with("Shift")
        || repr.starts_with("Control")
        || repr.starts_with("Alt")
        || repr.starts_with("Super")
        || repr.starts_with("Meta")
        || repr == "Caps_Lock"
        || repr == "Num_Lock"
        || repr.starts_with("ISO_Level")
        || repr == "Mode_switch"
}

/// 参照 `is_plain_char_key`：只接受无 Ctrl/Alt/Super 的字符输入。
pub fn is_plain_char_key(key_event: &KeyEvent, repr: &str) -> Option<char> {
    if key_event.ctrl() || key_event.alt() || key_event.super_modifier() {
        return None;
    }
    if repr.len() == 1
        && let Some(ch) = repr.chars().next()
        && ch.is_ascii_lowercase()
    {
        return Some(ch);
    }
    match repr {
        "semicolon" => return Some(';'),
        "apostrophe" => return Some('\''),
        _ => {}
    }
    if repr.len() == 1
        && let Some(ch) = repr.chars().next()
        && ch.is_ascii_digit()
    {
        return Some(ch);
    }
    if let Some(digit) = repr.strip_prefix("KP_")
        && digit.len() == 1
        && let Some(ch) = digit.chars().next()
        && ch.is_ascii_digit()
    {
        return Some(ch);
    }
    None
}

/// 参照 `ends_with_digit`：半角与全角数字都会触发小数点跟进。
pub fn ends_with_digit(text: &str) -> bool {
    let Some(last) = text.chars().last() else {
        return false;
    };
    last.is_ascii_digit() || ('\u{ff10}'..='\u{ff19}').contains(&last)
}

/// 参照 `get_min_retained_raw_length`：由配置提供的下限（缺失/非法为 0）。
pub fn min_retained_raw_length(value: Option<i64>) -> usize {
    match value {
        Some(number) if number >= 0 => number as usize,
        _ => 0,
    }
}

/// 参照 `invalidate_edit_state`：清除编辑相关瞬态并回退受影响的锁。
pub fn invalidate_edit_state(
    context: &mut Context,
    state: &mut SentenceState,
    first_changed: usize,
    full_length: usize,
) {
    state.tab_pending = false;
    state.last_seen_raw.clear();
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::Segment;

    fn state_with_lock(raw: &str, text: &str) -> (Context, SentenceState) {
        let mut context = Context::new();
        let mut state = SentenceState::fresh(1);
        state.locks.push(Lock {
            raw: raw.to_string(),
            text: text.to_string(),
            boundaries: "2,3;".to_string(),
        });
        state.committed_raw = raw.to_string();
        state.committed_text = text.to_string();
        state.save(&mut context);
        (context, state)
    }

    #[test]
    fn locks_round_trip_with_framing() {
        let (context, state) = state_with_lock("ab", "甲");
        let loaded = read_locks(&context);
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].raw, "ab");
        assert_eq!(loaded[0].text, "甲");
        assert_eq!(loaded[0].boundaries, "2,3;");
        assert_eq!(state.locks[0].text, "甲");
        // 空字段同样可往返（0: 帧）。
        let (mut context2, mut state2) = (Context::new(), SentenceState::fresh(1));
        state2.locks.push(Lock {
            raw: String::new(),
            text: "甲".to_string(),
            boundaries: "0,3;".to_string(),
        });
        state2.save(&mut context2);
        assert_eq!(read_locks(&context2), state2.locks);
        // 畸形数据返回空列表。
        context2.set_property(K_LOCKS, "9:ab");
        assert!(read_locks(&context2).is_empty());
    }

    #[test]
    fn committed_property_and_buffered_plumbing() {
        assert_eq!(
            parse_committed_property("ab\t甲"),
            ("ab".to_string(), "甲".to_string())
        );
        assert_eq!(
            parse_committed_property("no-tab"),
            (String::new(), String::new())
        );
        let mut context = Context::new();
        set_property_if_changed(&mut context, K_BUFFERED, "甲");
        context.set_input(b"~ab");
        context.set_caret(2);
        assert_eq!(buffered_text(&context), "甲");
        assert_eq!(live_input(&context), b"ab");
        assert_eq!(input_caret(&context), 1);
        restore_composition_input(&mut context, b"ab");
        assert_eq!(context.input(), b"~ab");
    }

    #[test]
    fn cycle_highlight_wraps_both_directions() {
        let mut context = Context::new();
        context.set_input(b"ab");
        let mut segment = Segment {
            start: 0,
            end: 2,
            ..Segment::default()
        };
        for text in ["甲", "乙", "丙"] {
            segment
                .candidates
                .push(crate::session::Candidate::new("sentence", 0, 2, text, ""));
        }
        context.composition.segments.push(segment);
        assert!(cycle_candidate_highlight(&mut context, 1));
        assert_eq!(context.composition.back().unwrap().selected_index, 1);
        assert!(cycle_candidate_highlight(&mut context, -1));
        assert_eq!(context.composition.back().unwrap().selected_index, 0);
        assert!(cycle_candidate_highlight(&mut context, -1));
        assert_eq!(context.composition.back().unwrap().selected_index, 2);
    }

    #[test]
    fn plain_char_and_modifier_detection() {
        let key = KeyEvent::new(0x61, 0);
        assert_eq!(is_plain_char_key(&key, "a"), Some('a'));
        assert_eq!(is_plain_char_key(&key, "semicolon"), Some(';'));
        assert_eq!(is_plain_char_key(&key, "apostrophe"), Some('\''));
        assert_eq!(is_plain_char_key(&key, "7"), Some('7'));
        assert_eq!(is_plain_char_key(&key, "KP_3"), Some('3'));
        assert_eq!(is_plain_char_key(&key, "A"), None);
        assert_eq!(is_plain_char_key(&key, "space"), None);
        let ctrl = KeyEvent::new(0x61, crate::key::K_CONTROL_MASK);
        assert_eq!(is_plain_char_key(&ctrl, "a"), None);
        assert!(is_modifier_repr("Shift_L"));
        assert!(is_modifier_repr("ISO_Level3_Shift"));
        assert!(is_modifier_repr("Mode_switch"));
        // 参照按前缀匹配：带修饰的组合键同样命中（用于“小数点待发”判定）。
        assert!(is_modifier_repr("Shift+a"));
        assert!(!is_modifier_repr("a"));
    }

    #[test]
    fn digit_detection_and_retention() {
        assert!(ends_with_digit("甲1"));
        assert!(ends_with_digit("甲１"));
        assert!(!ends_with_digit("甲"));
        assert!(!ends_with_digit(""));
        assert_eq!(min_retained_raw_length(Some(3)), 3);
        assert_eq!(min_retained_raw_length(Some(-1)), 0);
        assert_eq!(min_retained_raw_length(None), 0);
    }

    #[test]
    fn invalidate_removes_affected_locks_only() {
        let (mut context, mut state) = state_with_lock("ab", "甲");
        // 第二个锁延伸到已提交范围之外（可被编辑失效）。
        state.locks.push(Lock {
            raw: "abcd".to_string(),
            text: "甲乙".to_string(),
            boundaries: "2,3;4,6;".to_string(),
        });
        // 编辑发生在第二个锁内部 → 该锁被移除，第一个（已提交）保留。
        invalidate_edit_state(&mut context, &mut state, 3, 5);
        assert_eq!(state.locks.len(), 1);
        assert_eq!(state.locks[0].raw, "ab");
        // 编辑完全越过锁边界（first_changed >= raw 且 full_length > raw）→ 保留锁。
        state.locks.push(Lock {
            raw: "abcd".to_string(),
            text: "甲乙".to_string(),
            boundaries: "2,3;4,6;".to_string(),
        });
        invalidate_edit_state(&mut context, &mut state, 4, 6);
        assert_eq!(state.locks.len(), 2);
        // 删除到锁边界（full_length <= raw）→ 解锁。
        invalidate_edit_state(&mut context, &mut state, 0, 2);
        assert_eq!(state.locks.len(), 1);
        assert_eq!(state.locks[0].raw, "ab");
    }
}
