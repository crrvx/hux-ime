//! 交互层（2b）：会话状态、锁、早提交、translator/filters 与学习暂存，对应参照
//! `tiger_sentence.lua` 的状态段（`fresh_transient_state`…`ends_with_digit`）、
//! 证据/追踪器、早提交、`translator`、filters 与 `learning_selection` 系列。
//!
//! 说明：
//! - 参照的 `env` 瞬态状态在 Rust 由调用方持有 [`SentenceState`]（每会话一份）；
//! - 参照的 decode 增量缓存属性能优化，本移植的解码为无状态冷路径，
//!   `invalidate_edit_state` 因此只处理锁与瞬态标记（语义一致）。
//! - `read_locks` 采用严格整数解析：非法帧一律返回空表（参照的 `tonumber`
//!   对空白/浮点更宽容，但属性数据只由本实现写出，实际不会出现该差异）。

use crate::decode::{DecodeLock, Decoder, Evaluated, Evidence};
use crate::key::KeyEvent;
use crate::learning::{self, DiffEvent, DiffItem, DiffPathNode, Event};
use crate::lexicon::Lexicon;
use crate::session::{Candidate, Context};
use hashbrown::HashMap;

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

/// 参照 `max_raw_length`：实时输入上限（超出则不接收普通字符）。
pub const MAX_RAW_LENGTH: usize = 128;
/// tracker 键分隔符（参照 `state_separator`）。
const STATE_SEPARATOR: &str = "\u{1f}";
const EARLY_COMMIT_MINIMUM_SHARE: f64 = 0.995;
const EARLY_COMMIT_STRONG_SHARE: f64 = 0.99999;
const EARLY_COMMIT_REQUIRED_EVIDENCE: usize = 3;
const EARLY_COMMIT_REQUIRED_STRONG: usize = 2;
const EARLY_COMMIT_MAXIMUM_NEUTRAL_GAP: usize = 3;
const EARLY_COMMIT_RETAINED_RAW_LENGTH: usize = 3;
/// 提前上屏到预编辑的选项（参照同名字符串）。
pub const OPTION_EARLY_COMMIT_TO_PREEDIT: &str = "tiger_sentence_early_commit_to_preedit";
/// 提前上屏总开关。
pub const OPTION_EARLY_COMMIT: &str = "tiger_sentence_early_commit";

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

/// 参照 `set_allow_duplicate_single`：读取选项（缺省 true，仅显式关闭时为 false）。
pub fn set_allow_duplicate_single(context: &Context) -> bool {
    context.get_option_or(OPTION_ALLOW_DUPLICATE_SINGLE, true)
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

/// 参照 `reset_early_evidence`。
pub fn reset_early_evidence(state: &mut SentenceState) {
    state.trackers.clear();
    state.last_seen_raw.clear();
}

/// 参照 `has_selection_suffix`：显式选重后缀（分号/引号/数字）。
pub fn has_selection_suffix(raw: &[u8]) -> bool {
    raw.iter()
        .any(|byte| *byte == b';' || *byte == b'\'' || byte.is_ascii_digit())
}

/// 参照 `common_text_prefix`：逐字符公共前缀。
pub fn common_text_prefix(left: &str, right: &str) -> String {
    let mut out = String::new();
    for (a, b) in left.chars().zip(right.chars()) {
        if a != b {
            break;
        }
        out.push(a);
    }
    out
}

/// 参照 `prefix_extends`：互为字节前缀。
fn prefix_extends(left: &str, right: &str) -> bool {
    left.starts_with(right) || right.starts_with(left)
}

/// 参照 `prefix_contradicted`。
fn prefix_contradicted(tracker: &Tracker, evidence: &Evidence) -> bool {
    if evidence.prefixes.is_empty() {
        return false;
    }
    let own = evidence.find(&tracker.text, tracker.raw_length);
    let self_share = own.map(|prefix| prefix.share).unwrap_or(0.0);
    for prefix in &evidence.prefixes {
        if !prefix.text.is_empty()
            && prefix.text != tracker.text
            && !prefix_extends(&prefix.text, &tracker.text)
        {
            let shared = common_text_prefix(&prefix.text, &tracker.text);
            if !shared.is_empty()
                && shared.len() < tracker.text.len()
                && (own.is_none() || prefix.share > self_share)
            {
                return true;
            }
        }
    }
    false
}

/// 参照 `retain_trackers_without_counting`。
fn retain_trackers_without_counting(
    trackers: &HashMap<String, Tracker>,
    evidence: &Evidence,
) -> HashMap<String, Tracker> {
    let mut next = HashMap::new();
    for (key, tracker) in trackers {
        let Some(current) = evidence.find(&tracker.text, tracker.raw_length) else {
            continue;
        };
        if prefix_contradicted(tracker, evidence) {
            continue;
        }
        let mut tracker = tracker.clone();
        tracker.gap_count += 1;
        if tracker.gap_count <= EARLY_COMMIT_MAXIMUM_NEUTRAL_GAP {
            tracker.last_share = current.share;
            next.insert(key.clone(), tracker);
        }
    }
    next
}

/// 参照 `tracker_better`。
fn tracker_better(left: &Tracker, right: &Tracker) -> bool {
    if left.text_char_count != right.text_char_count {
        return left.text_char_count > right.text_char_count;
    }
    if left.last_share != right.last_share {
        return left.last_share > right.last_share;
    }
    left.raw_length < right.raw_length
}

/// 参照 `implicit_rank_allowed`：空码提交后的续接只放宽到合法隐式路径。
pub fn implicit_rank_allowed(
    candidate: &Evaluated,
    raw: &[u8],
    continuation_after_auto_commit: bool,
    allow_duplicate_single: bool,
) -> bool {
    if !continuation_after_auto_commit {
        return true;
    }
    let previous_nonempty = candidate
        .previous_text
        .as_deref()
        .map(|text| !text.is_empty())
        .unwrap_or(false);
    has_selection_suffix(raw)
        || candidate.max_rank <= 1
        || (allow_duplicate_single && previous_nonempty)
}

/// 参照 `strong_empty_code_candidate`：未截断池中的强置信候选。
fn strong_empty_code_candidate(
    eligible: &[&Evaluated],
    candidate_index: usize,
    visible_top: Option<&str>,
    pool_truncated: bool,
) -> bool {
    if pool_truncated {
        return false;
    }
    let Some(top) = visible_top else {
        return false;
    };
    if eligible[candidate_index].text != top {
        return false;
    }
    let max_score = eligible
        .iter()
        .map(|candidate| candidate.confidence_score)
        .fold(f64::NEG_INFINITY, f64::max);
    let mut total = 0.0;
    let mut candidate_mass = 0.0;
    for (index, candidate) in eligible.iter().enumerate() {
        let mass = (candidate.confidence_score - max_score).exp();
        total += mass;
        if index == candidate_index {
            candidate_mass += mass;
        }
    }
    total > 0.0 && candidate_mass / total >= EARLY_COMMIT_STRONG_SHARE
}

/// 参照 `capture_empty_code_candidate`。
pub fn capture_empty_code_candidate(
    decoder: &mut Decoder,
    full_before: &[u8],
    committed_text: &str,
    allow_duplicate_single: bool,
    lock: Option<&Lock>,
) -> anyhow::Result<Option<EmptyCodePending>> {
    let raw = String::from_utf8_lossy(full_before).into_owned();
    decoder.set_allow_duplicate_single(allow_duplicate_single);
    let lock = lock.map(|lock| DecodeLock {
        raw: &lock.raw,
        text: &lock.text,
        boundaries: &lock.boundaries,
    });
    let decoded = decoder.decode_with_lock(&raw, false, committed_text, lock)?;
    if decoded.items.is_empty() || decoded.learning_affected {
        return Ok(None);
    }
    let visible_top = decoded
        .items
        .first()
        .map(|candidate| candidate.text.clone());
    let restrict = !has_selection_suffix(full_before);
    let is_eligible = |candidate: &Evaluated| {
        let previous_nonempty = candidate
            .previous_text
            .as_deref()
            .map(|text| !text.is_empty())
            .unwrap_or(false);
        !restrict
            || candidate.max_rank <= 1
            || (allow_duplicate_single
                && (previous_nonempty || candidate.text.chars().count() == 1))
    };
    let Some(first_index) = decoded.items.iter().position(is_eligible) else {
        return Ok(None);
    };
    let eligible: Vec<&Evaluated> = decoded
        .confidence_candidates
        .iter()
        .filter(|candidate| is_eligible(candidate))
        .collect();
    let candidate_index = eligible
        .iter()
        .position(|candidate| {
            candidate.path == decoded.items[first_index].path
                && candidate.text == decoded.items[first_index].text
        })
        .unwrap_or(0);
    let first = &decoded.items[first_index];
    if first.text.is_empty()
        || !first.text.starts_with(committed_text)
        || first.text.len() <= committed_text.len()
    {
        return Ok(None);
    }
    let pool_truncated = decoded.evidence.confidence_truncated;
    if eligible.len() > 1
        && !strong_empty_code_candidate(
            &eligible,
            candidate_index,
            visible_top.as_deref(),
            pool_truncated,
        )
    {
        return Ok(None);
    }
    Ok(Some(EmptyCodePending {
        candidate_text: first.text.clone(),
        requires_uniqueness_check: eligible.len() == 1,
        committed_text: committed_text.to_string(),
        base_raw_length: full_before.len(),
        last_segment_start: first.previous_raw_length,
    }))
}

/// 参照 `submit_early`：缓冲分支写回缓冲与单锁；否则返回待上屏文本。
pub fn submit_early(
    context: &mut Context,
    state: &mut SentenceState,
    commit: &str,
) -> Option<String> {
    if context.get_option(OPTION_EARLY_COMMIT_TO_PREEDIT) || !state.buffered_text.is_empty() {
        state.buffered_text.push_str(commit);
        state.locks = vec![Lock {
            raw: state.committed_raw.clone(),
            text: state.committed_text.clone(),
            boundaries: format!(
                "{},{};",
                state.committed_raw.len(),
                state.committed_text.len()
            ),
        }];
        state.save(context);
        None
    } else {
        Some(commit.to_string())
    }
}

/// 参照 `try_commit_mature_prefix`：证据成熟则提交选中前缀。
pub fn try_commit_mature_prefix(
    context: &mut Context,
    state: &mut SentenceState,
    evidence_raw: &[u8],
    min_retained: usize,
) -> bool {
    let retain = if min_retained > 0 {
        EARLY_COMMIT_RETAINED_RAW_LENGTH.max(min_retained)
    } else {
        EARLY_COMMIT_RETAINED_RAW_LENGTH
    };
    let mut selected: Option<&Tracker> = None;
    for tracker in state.trackers.values() {
        if (tracker.evidence_count >= EARLY_COMMIT_REQUIRED_EVIDENCE
            || tracker.strong_count >= EARLY_COMMIT_REQUIRED_STRONG)
            && tracker.raw_length > state.committed_raw.len()
            && tracker.raw_length <= evidence_raw.len()
            && evidence_raw.len() - tracker.raw_length >= retain
            && tracker.text.len() > state.committed_text.len()
            && tracker.text.starts_with(&state.committed_text)
            && selected
                .map(|current| tracker_better(tracker, current))
                .unwrap_or(true)
        {
            selected = Some(tracker);
        }
    }
    let Some(selected) = selected else {
        return false;
    };
    if evidence_raw.len() - state.last_auto_commit_raw_length < EARLY_COMMIT_RETAINED_RAW_LENGTH {
        return false;
    }
    let commit = selected.text[state.committed_text.len()..].to_string();
    if commit.is_empty() {
        return false;
    }
    let selected_text = selected.text.clone();
    let selected_raw_length = selected.raw_length;
    state.committed_text = selected_text;
    state.committed_raw =
        String::from_utf8_lossy(&evidence_raw[..selected_raw_length]).into_owned();
    state.last_auto_commit_raw_length = selected_raw_length;
    state.continuation_after_auto_commit = false;
    reset_early_evidence(state);
    state.save(context);
    if let Some(commit_text) = submit_early(context, state, &commit) {
        context_commit(context, &commit_text);
    }
    restore_composition_input(context, &evidence_raw[selected_raw_length..]);
    true
}

/// 提交文本到上下文（组合外直接提交；供 `submit_early` 的非缓冲分支使用）。
fn context_commit(context: &mut Context, text: &str) {
    context.direct_commit(text);
}

/// 提前上屏的共用参数（避免 `too_many_arguments`）。
#[derive(Clone, Copy, Debug)]
pub struct EarlyCommitParams {
    pub allow_duplicate_single: bool,
    pub generation: u64,
    pub min_retained: usize,
}

/// 参照 `try_early_commit`：证据驱动的前缀提前上屏。
pub fn try_early_commit(
    decoder: &mut Decoder,
    context: &mut Context,
    state: &mut SentenceState,
    params: EarlyCommitParams,
) -> anyhow::Result<bool> {
    let live_raw = live_input(context);
    if input_caret(context) != live_raw.len()
        || !context.get_option(OPTION_EARLY_COMMIT)
        || state.suspended
    {
        reset_early_evidence(state);
        return Ok(false);
    }
    let mut full_raw = state.committed_raw.as_bytes().to_vec();
    full_raw.extend_from_slice(&live_raw);
    if full_raw.len() <= 4 {
        reset_early_evidence(state);
        return Ok(false);
    }
    let raw = String::from_utf8_lossy(&full_raw).into_owned();
    decoder.set_allow_duplicate_single(params.allow_duplicate_single);
    let lock = state.active_lock().map(|lock| DecodeLock {
        raw: &lock.raw,
        text: &lock.text,
        boundaries: &lock.boundaries,
    });
    let decoded = decoder.decode_with_lock(&raw, true, &state.committed_text, lock)?;
    if params.generation != state.model_generation {
        state.synchronize_model_state(params.generation);
        return Ok(false);
    }
    if decoded.learning_affected || decoded.evidence.confidence_truncated {
        reset_early_evidence(state);
        return Ok(false);
    }
    let evidence_raw = full_raw;

    if state.last_seen_raw == raw {
        return Ok(try_commit_mature_prefix(
            context,
            state,
            &evidence_raw,
            params.min_retained,
        ));
    }

    let extends_previous_generation = state.last_seen_raw.is_empty()
        || (evidence_raw.len() == state.last_seen_raw.len() + 1
            && raw.starts_with(&state.last_seen_raw));
    if !extends_previous_generation {
        state.trackers.clear();
    }
    state.last_seen_raw = raw;

    let accepted_top = decoded
        .items
        .first()
        .filter(|candidate| candidate.supplement_score > 0.0)
        .map(|candidate| candidate.text.clone());
    let merged_incomplete_tail = decoded.evidence.merged_incomplete_tail;
    let mut qualifying: HashMap<String, &crate::decode::PrefixEvidence> = HashMap::new();
    for prefix in &decoded.evidence.prefixes {
        if !prefix.text.is_empty()
            && prefix.boundary_closed
            && prefix.share >= EARLY_COMMIT_MINIMUM_SHARE
            && prefix.raw_length > state.committed_raw.len()
            && prefix.text.len() > state.committed_text.len()
            && prefix.text.starts_with(&state.committed_text)
            && accepted_top
                .as_deref()
                .map(|top| top.starts_with(&prefix.text))
                .unwrap_or(true)
            && (merged_incomplete_tail
                || decoded
                    .visible_prefixes
                    .contains(&(prefix.raw_length, prefix.text.clone())))
        {
            qualifying.insert(
                format!("{}{}{}", prefix.text, STATE_SEPARATOR, prefix.raw_length),
                prefix,
            );
        }
    }

    let retain_without_counting = qualifying.is_empty()
        && (decoded.evidence.neutral_low_confidence || merged_incomplete_tail);
    if retain_without_counting {
        state.trackers = retain_trackers_without_counting(&state.trackers, &decoded.evidence);
        return Ok(try_commit_mature_prefix(
            context,
            state,
            &evidence_raw,
            params.min_retained,
        ));
    }

    let mut next_trackers: HashMap<String, Tracker> = HashMap::new();
    for (key, prefix) in qualifying {
        let mut tracker = state.trackers.get(&key).cloned().unwrap_or(Tracker {
            text: prefix.text.clone(),
            text_char_count: prefix.text_char_count,
            raw_length: prefix.raw_length,
            evidence_count: 0,
            strong_count: 0,
            gap_count: 0,
            last_share: 0.0,
        });
        tracker.evidence_count = EARLY_COMMIT_REQUIRED_EVIDENCE.min(tracker.evidence_count + 1);
        tracker.strong_count = if prefix.share >= EARLY_COMMIT_STRONG_SHARE {
            EARLY_COMMIT_REQUIRED_STRONG.min(tracker.strong_count + 1)
        } else {
            0
        };
        tracker.gap_count = 0;
        tracker.last_share = prefix.share;
        next_trackers.insert(key, tracker);
    }
    state.trackers = next_trackers;
    Ok(try_commit_mature_prefix(
        context,
        state,
        &evidence_raw,
        params.min_retained,
    ))
}

/// 参照 `try_empty_code_commit`：空码（整句唯一候选）自动上屏。
pub fn try_empty_code_commit(
    decoder: &mut Decoder,
    context: &mut Context,
    state: &mut SentenceState,
    full_before: &[u8],
    appended_letter: &[u8],
    params: EarlyCommitParams,
) -> anyhow::Result<bool> {
    if !context.get_option(OPTION_EARLY_COMMIT) || state.suspended {
        state.empty_code_pending = None;
        return Ok(false);
    }
    if state.synchronize_model_state(params.generation) {
        return Ok(false);
    }
    let pending = match state.empty_code_pending.clone() {
        Some(pending) => Some(pending),
        None => capture_empty_code_candidate(
            decoder,
            full_before,
            &state.committed_text,
            params.allow_duplicate_single,
            state.active_lock(),
        )?,
    };
    let mut full_raw = state.committed_raw.as_bytes().to_vec();
    full_raw.extend_from_slice(&live_input(context));
    let mut expected = full_before.to_vec();
    expected.extend_from_slice(appended_letter);
    if full_raw != expected || input_caret(context) != live_input(context).len() {
        state.empty_code_pending = None;
        return Ok(false);
    }
    state.empty_code_pending = pending.clone();

    let Some(pending) = pending else {
        return Ok(false);
    };
    let raw = String::from_utf8_lossy(&full_raw).into_owned();
    if crate::decode::has_complete_candidate(
        decoder.lexicon(),
        &raw,
        &state.committed_text,
        None,
        false,
        params.allow_duplicate_single,
    ) {
        state.empty_code_pending = None;
        return Ok(false);
    }
    if pending.committed_text != state.committed_text
        || pending.base_raw_length >= full_raw.len()
        || pending.last_segment_start >= full_raw.len()
    {
        state.empty_code_pending = None;
        return Ok(false);
    }
    let extended_last_segment =
        String::from_utf8_lossy(&full_raw[pending.last_segment_start..]).into_owned();
    if decoder
        .lexicon()
        .proper_code_prefixes
        .contains(&extended_last_segment)
    {
        return Ok(false);
    }
    if params.min_retained > 0 && full_raw.len() - pending.base_raw_length < params.min_retained {
        return Ok(false);
    }
    if pending.requires_uniqueness_check
        && crate::decode::has_complete_candidate(
            decoder.lexicon(),
            &String::from_utf8_lossy(&full_raw[..pending.base_raw_length]),
            &pending.committed_text,
            Some(&pending.candidate_text),
            true,
            params.allow_duplicate_single,
        )
    {
        state.empty_code_pending = None;
        return Ok(false);
    }
    let commit = pending.candidate_text[pending.committed_text.len()..].to_string();
    let retained_raw = full_raw[pending.base_raw_length..].to_vec();
    state.committed_text = pending.candidate_text.clone();
    state.committed_raw =
        String::from_utf8_lossy(&full_raw[..pending.base_raw_length]).into_owned();
    state.last_auto_commit_raw_length = pending.base_raw_length;
    state.trackers.clear();
    state.last_seen_raw.clear();
    state.suspended = false;
    state.empty_code_pending = None;
    state.continuation_after_auto_commit = true;
    // 参照顺序：submit_early → save_sentence_state → restore（缓冲分支在
    // submit_early 内部已保存一次，幂等）。
    if let Some(commit_text) = submit_early(context, state, &commit) {
        context_commit(context, &commit_text);
    }
    state.save(context);
    restore_composition_input(context, &retained_raw);
    Ok(true)
}

/// 参照 `trim_segmented_after_raw_prefix`：去掉前 `raw_prefix_length` 个原始字符
/// 对应的片段（片段为 ASCII，按字节计数即可）。
pub fn trim_segmented_after_raw_prefix(segmented: &str, raw_prefix_length: usize) -> String {
    if raw_prefix_length == 0 || segmented.is_empty() {
        return segmented.to_string();
    }
    let bytes = segmented.as_bytes();
    let mut raw_count = 0usize;
    let mut index = 0usize;
    while index < bytes.len() && raw_count < raw_prefix_length {
        if bytes[index] != b' ' {
            raw_count += 1;
        }
        index += 1;
    }
    while index < bytes.len() && bytes[index] == b' ' {
        index += 1;
    }
    if index < bytes.len() {
        segmented[index..].to_string()
    } else {
        String::new()
    }
}

/// 参照 `reverse_comment`：单字显示全部编码（源序），词组逐字 `字:码组`。
pub fn reverse_comment(lexicon: &Lexicon, text: &str) -> Option<String> {
    if !lexicon.built {
        return None;
    }
    let chars: Vec<char> = text.chars().collect();
    if chars.is_empty() {
        return None;
    }
    if chars.len() == 1 {
        let codes = lexicon.character_codes.get(&chars[0].to_string())?;
        if codes.is_empty() {
            return None;
        }
        return Some(format!(" {}", codes.join(" / ")));
    }
    let mut parts = Vec::with_capacity(chars.len());
    for ch in &chars {
        match lexicon.character_codes.get(&ch.to_string()) {
            Some(codes) if !codes.is_empty() => {
                parts.push(format!("{}:{}", ch, codes.join("/")));
            }
            _ => parts.push(format!("{}:?", ch)),
        }
    }
    Some(format!(" {}", parts.join(" ")))
}

/// 参照 `translator(input, seg, env)`：解码产出候选（无锁路径）。
pub fn translate(
    decoder: &mut Decoder,
    context: &Context,
    state: &SentenceState,
    input: &[u8],
    seg_start: usize,
    seg_end: usize,
    out: &mut Vec<Candidate>,
) -> anyhow::Result<()> {
    if input.first() == Some(&b'`') {
        return Ok(()); // 反查段由 reverse lookup 处理
    }
    let allow_duplicate_single = set_allow_duplicate_single(context);
    decoder.set_allow_duplicate_single(allow_duplicate_single);
    let committed_text = state.committed_text.clone();
    let committed_raw = state.committed_raw.clone();
    let buffered = state.buffered_text.clone();
    let mut input = input;
    if !buffered.is_empty() {
        if seg_start != 0 || input.first() != Some(&b'~') {
            return Ok(());
        }
        input = &input[1..];
    }
    if !buffered.is_empty()
        && input.is_empty()
        && let Some(lock) = state.active_lock()
        && lock.raw == committed_raw
        && lock.text == committed_text
    {
        let mut candidate = Candidate::new("sentence_buffered", seg_start, seg_end, "", "");
        candidate.quality = 1000.0;
        candidate.preedit = buffered;
        out.push(candidate);
        return Ok(());
    }
    let mut raw = committed_raw.as_bytes().to_vec();
    raw.extend_from_slice(input);
    let raw_text = String::from_utf8_lossy(&raw).into_owned();
    let lock = state.active_lock().map(|lock| DecodeLock {
        raw: &lock.raw,
        text: &lock.text,
        boundaries: &lock.boundaries,
    });
    let decoded = decoder.decode_with_lock(&raw_text, false, &committed_text, lock)?;
    let mut yielded = 0usize;
    for item in &decoded.items {
        if !implicit_rank_allowed(
            item,
            &raw,
            state.continuation_after_auto_commit,
            allow_duplicate_single,
        ) {
            continue;
        }
        if !committed_text.is_empty() && !item.text.starts_with(&committed_text) {
            continue;
        }
        let text = if committed_text.is_empty() {
            item.text.clone()
        } else {
            item.text[committed_text.len()..].to_string()
        };
        let mut preedit = item.segmented.clone();
        if !committed_raw.is_empty() {
            preedit = trim_segmented_after_raw_prefix(&item.segmented, committed_raw.len());
        }
        if text.is_empty() && buffered.is_empty() {
            continue;
        }
        let kind = if buffered.is_empty() {
            "sentence"
        } else {
            "sentence_buffered"
        };
        let mut candidate = Candidate::new(kind, seg_start, seg_end, &text, "");
        if !buffered.is_empty() {
            candidate.quality = 1000.0;
        }
        let separator = if !buffered.is_empty() && !preedit.is_empty() {
            " "
        } else {
            ""
        };
        candidate.preedit = format!("{buffered}{separator}{preedit}");
        out.push(candidate);
        yielded += 1;
        if yielded >= CANDIDATE_LIMIT {
            return Ok(());
        }
    }
    if yielded == 0 && !buffered.is_empty() {
        let mut candidate = Candidate::new(
            "sentence_buffered",
            seg_start,
            seg_end,
            &String::from_utf8_lossy(input),
            "",
        );
        candidate.quality = 1000.0;
        let separator = if input.is_empty() { "" } else { " " };
        candidate.preedit = format!("{buffered}{separator}{}", String::from_utf8_lossy(input));
        out.push(candidate);
    }
    Ok(())
}

/// 参照 `buffer_filter`：缓冲态只保留 `sentence_buffered` 候选。
pub fn buffer_filter(candidates: &[Candidate], buffered: bool) -> Vec<Candidate> {
    candidates
        .iter()
        .filter(|candidate| !buffered || candidate.kind == "sentence_buffered")
        .cloned()
        .collect()
}

/// 参照 `reverse_comment_filter`：反查段候选写入虎码注释。
pub fn reverse_comment_filter(candidates: &mut [Candidate], active: bool, lexicon: &Lexicon) {
    if !active {
        return;
    }
    for candidate in candidates {
        if let Some(comment) = reverse_comment(lexicon, &candidate.text) {
            candidate.comment = comment;
        }
    }
}

/// 参照 `ends_with_digit`。
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

// ---------------------------------------------------------------- 学习暂存

/// 参照 `learning_selection` 的选中项：文本 + 路径末节点 raw 长度 + `learning.diff` 路径。
#[derive(Clone, Debug)]
pub struct Selected {
    pub text: String,
    pub raw_length: usize,
    pub diff: DiffItem,
}

impl Selected {
    /// 参照缓冲兜底 `{text=committed_text, path={raw_length=#committed_raw}}`。
    /// 参照兜底节点缺 `text_length`（diff 会因比较 nil 报错并被 pcall 吞掉）；
    /// 此处补成良构节点，行为契约为「缓冲空闲时以已确认前缀为选中项」。
    pub fn buffered(committed_raw: &str, committed_text: &str) -> Self {
        let raw_length = committed_raw.len();
        let text_length = committed_text.len();
        Self {
            text: committed_text.to_string(),
            raw_length,
            diff: DiffItem {
                text: committed_text.to_string(),
                path: vec![DiffPathNode {
                    raw_length,
                    text_length,
                }],
            },
        }
    }
}

/// 参照 `learning_selection` 的三元返回（`selected`、`first`、`raw`）。
#[derive(Debug, Default)]
pub struct LearningSelection {
    pub selected: Option<Selected>,
    pub first: Option<Selected>,
    pub raw: Vec<u8>,
}

/// 交互层学习暂存（对应参照 `env._tiger_learning` 的暂存字段；存储/索引归 K3）。
#[derive(Clone, Debug, Default)]
pub struct LiveLearning {
    pub mode: String,
    pub pending: Vec<DiffEvent>,
    pub baseline: Option<Selected>,
    pub submitted_raw: Option<String>,
    pub hide_owned: bool,
    /// 参照 `learned.store and learned.store.db`（K3 学习库就绪后置位）。
    pub store_ready: bool,
}

/// 参照 `learning_selection`：按当前段选中项从可见候选中取学习目标。
pub fn learning_selection(
    decoder: &mut Decoder,
    context: &Context,
    state: &SentenceState,
) -> anyhow::Result<LearningSelection> {
    let live = live_input(context);
    let mut raw = state.committed_raw.as_bytes().to_vec();
    raw.extend_from_slice(&live);
    let allow_duplicate_single = set_allow_duplicate_single(context);
    decoder.set_allow_duplicate_single(allow_duplicate_single);
    let target = context
        .composition
        .back()
        .map(|segment| segment.selected_index)
        .unwrap_or(0);
    let lock = state.active_lock().map(|lock| DecodeLock {
        raw: &lock.raw,
        text: &lock.text,
        boundaries: &lock.boundaries,
    });
    let raw_text = String::from_utf8_lossy(&raw).into_owned();
    let decoded = decoder.decode_with_lock(&raw_text, false, &state.committed_text, lock)?;
    let mut first: Option<Selected> = None;
    let mut selected: Option<Selected> = None;
    let mut visible = 0usize;
    for item in &decoded.items {
        if implicit_rank_allowed(
            item,
            &raw,
            state.continuation_after_auto_commit,
            allow_duplicate_single,
        ) && item.text.starts_with(&state.committed_text)
            && item.text.len() > state.committed_text.len()
        {
            let (raw_length, diff) = decoder.path_summary(item);
            let candidate = Selected {
                text: item.text.clone(),
                raw_length,
                diff,
            };
            if first.is_none() {
                first = Some(candidate.clone());
            }
            if visible == target {
                selected = Some(candidate);
            }
            visible += 1;
        }
    }
    if selected.is_none() && live.is_empty() && !state.buffered_text.is_empty() {
        selected = Some(Selected::buffered(
            &state.committed_raw,
            &state.committed_text,
        ));
    }
    Ok(LearningSelection {
        selected,
        first,
        raw,
    })
}

/// 参照 `learning_stage`：把 `before -> selected` 的差异事件并入 `pending`。
pub fn learning_stage(
    live: &mut LiveLearning,
    state: &SentenceState,
    selected: Option<&Selected>,
    raw: &[u8],
    submitted_first: Option<&Selected>,
    now: f64,
) {
    if live.mode.is_empty() {
        return;
    }
    let Some(selected) = selected else {
        return;
    };
    let baseline = if state.tab_pending {
        live.baseline.as_ref()
    } else {
        submitted_first
    };
    if let Some(baseline) = baseline {
        let lock_floor = state.active_lock().map(|lock| lock.raw.len()).unwrap_or(0);
        let floor = state.committed_raw.len().max(lock_floor);
        let events = learning::diff(
            raw,
            Some(&baseline.diff),
            Some(&selected.diff),
            floor,
            &live.mode,
            now,
        );
        for event in events {
            if live.pending.len() < 256 {
                live.pending.push(event);
            }
        }
    }
    live.baseline = None;
}

/// 参照 `learning_submit`：筛选 `pending`、**无条件消费**，返回待持久化事件。
pub fn learning_submit(
    live: &mut LiveLearning,
    selected: Option<&Selected>,
    actual: &str,
    expected: &str,
) -> Vec<Event> {
    let mut accepted = Vec::new();
    let mut remaining = Vec::new();
    if let Some(selected) = selected {
        if !actual.is_empty() && actual == expected && !live.mode.is_empty() {
            for event in &live.pending {
                if event.raw_end > selected.raw_length {
                    remaining.push(event.clone());
                } else if event.mode == live.mode
                    && event.text_start >= selected.text.len().saturating_sub(expected.len())
                    && selected
                        .text
                        .get(event.text_start..event.text_end)
                        .is_some_and(|text| text == event.text.as_str())
                {
                    accepted.push(event.clone());
                }
            }
        }
    }
    live.pending = remaining;
    live.baseline = None;
    accepted
        .into_iter()
        .map(|event| Event {
            time: event.time,
            mode: event.mode,
            code: event.code,
            text: event.text,
            context: event.context,
        })
        .collect()
}

// ---------------------------------------------------------------- 选项同步

/// 参照 `M.options` 的内建缺省表。
pub fn option_defaults() -> HashMap<String, bool> {
    HashMap::from([
        ("tiger_sentence_early_commit".to_string(), true),
        ("tiger_sentence_allow_duplicate_single".to_string(), true),
        ("tiger_sentence_early_commit_to_preedit".to_string(), false),
    ])
}

/// 参照 `M.options` 的配置存储（文件读写、错误属性由 K3 承担）。
#[derive(Clone, Debug, Default)]
pub struct Options {
    /// schema 缺省（`tiger_sentence/option_defaults/<name>`，回退内建缺省）。
    pub defaults: HashMap<String, bool>,
    /// 持久化值（`options/<name>`，缺省回退 `user.yaml` 的 `var/option/<name>`）。
    pub values: HashMap<String, bool>,
    pub revision: u64,
}

impl Options {
    pub fn new(defaults: HashMap<String, bool>) -> Self {
        Self {
            defaults,
            values: HashMap::new(),
            revision: 0,
        }
    }

    /// 参照 `M.options.sync`：把持久化值（缺省回退 schema 缺省）同步进上下文选项。
    pub fn sync(&self, context: &mut Context) {
        for (name, fallback) in &self.defaults {
            let value = self.values.get(name).copied().unwrap_or(*fallback);
            if context.get_option(name) != value {
                context.set_option(name, value);
            }
        }
    }

    /// 参照 `option_update_notifier` 回调：记录变更并递增 revision；
    /// 返回是否需要持久化（写文件与失败属性由 K3 处理）。
    pub fn observe(&mut self, context: &Context, name: &str) -> bool {
        if !self.defaults.contains_key(name) {
            return false;
        }
        let value = context.get_option(name);
        if self.values.get(name) == Some(&value) {
            return false;
        }
        self.values.insert(name.to_string(), value);
        self.revision += 1;
        true
    }
}

// ---------------------------------------------------------------- ascii 策略

/// 参照 `ascii_component` 私有 schema 覆盖的按键名。
pub const ASCII_SWITCH_KEYS: [&str; 10] = [
    "Shift_L",
    "Shift_R",
    "Control_L",
    "Control_R",
    "Alt_L",
    "Alt_R",
    "Super_L",
    "Super_R",
    "Caps_Lock",
    "Eisu_toggle",
];

/// 参照 `ascii_component`：缓冲态下把 `commit_code`/`inline_ascii` 归一为
/// `commit_text`，未配置样式按 `noop`（原生 ascii_composer 由 K3 宿主提供）。
pub fn ascii_switch_styles(source: &HashMap<String, String>) -> HashMap<String, String> {
    ASCII_SWITCH_KEYS
        .iter()
        .map(|name| {
            let style = source
                .get(*name)
                .cloned()
                .unwrap_or_else(|| "noop".to_string());
            let style = if style == "commit_code" || style == "inline_ascii" {
                "commit_text".to_string()
            } else {
                style
            };
            (name.to_string(), style)
        })
        .collect()
}

// ---------------------------------------------------------------- 处理器

/// 处理器宿主环境（对应参照 `env` 的非会话部分；K3 补内存/词库/选项职责）。
pub struct ProcessorEnv<'a> {
    /// 参照 `os.time()`（学习事件时间戳）。
    pub now: f64,
    /// 参照 `env._tiger_sentence_dot_armed`（数字后小数点待发）。
    pub dot_armed: &'a mut bool,
    /// 参照 `get_min_retained_raw_length(env)` 的配置值。
    pub min_retained: Option<i64>,
}

/// 处理器结果：`Consume` 对应参照返回 1（拦截），`Forward` 对应 2（交后续处理器）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ProcessorResult {
    Consume,
    Forward,
}

/// 参照 `processor(key_event, env)`。宿主职责（内存配置、词库懒加载、选项同步、
/// 学习库存储）由调用方在进入前完成。
pub fn processor(
    key_event: &KeyEvent,
    context: &mut Context,
    state: &mut SentenceState,
    decoder: &mut Decoder,
    live: &mut LiveLearning,
    env: &mut ProcessorEnv<'_>,
) -> anyhow::Result<ProcessorResult> {
    if key_event.release() {
        return Ok(ProcessorResult::Forward);
    }
    let repr = key_event.repr();
    let repr = repr.as_str();
    let allow_duplicate_single = set_allow_duplicate_single(context);
    decoder.set_allow_duplicate_single(allow_duplicate_single);
    live.submitted_raw = None;
    // 缓冲空闲时把菜单导航键留给宿主。
    if !state.buffered_text.is_empty()
        && live_input(context).is_empty()
        && matches!(
            repr,
            "Tab" | "ISO_Left_Tab" | "Shift+Tab" | "Up" | "Down" | "Page_Up" | "Page_Down"
        )
    {
        return Ok(ProcessorResult::Consume);
    }
    if !state.buffered_text.is_empty() && context.caret() < 1 {
        context.set_caret(1);
    }
    if !context.is_composing() {
        live.pending.clear();
        live.baseline = None;
    }
    // 参照 `_dotAfterDigitArmed`：先取待发值；非修饰键随后消耗它。
    let dot_armed = *env.dot_armed;
    if !is_modifier_repr(repr) {
        *env.dot_armed = false;
    }
    let params = EarlyCommitParams {
        allow_duplicate_single,
        generation: state.model_generation,
        min_retained: min_retained_raw_length(env.min_retained),
    };
    if let Some(ch) = is_plain_char_key(key_event, repr) {
        if !context.is_composing()
            && (!state.committed_raw.is_empty()
                || !state.last_seen_raw.is_empty()
                || !state.trackers.is_empty()
                || state.suspended
                || state.continuation_after_auto_commit
                || state.active_lock().is_some()
                || state.tab_pending)
        {
            state.reset(context, false);
        }
        // 分号/引号只在组合中作 rank 选择器；空闲时交标点处理器。
        if !context.is_composing() && (ch == ';' || ch == '\'') {
            return Ok(ProcessorResult::Forward);
        }
        if live_input(context).len() >= MAX_RAW_LENGTH {
            return Ok(ProcessorResult::Consume);
        }
        // 空闲数字直接上屏（全角选项下为全角）。
        if ch.is_ascii_digit() && !context.is_composing() {
            if context.get_option("full_shape") {
                const FULL_SHAPE_DIGITS: [char; 10] =
                    ['０', '１', '２', '３', '４', '５', '６', '７', '８', '９'];
                let index = (ch as u8 - b'0') as usize;
                context.direct_commit(&FULL_SHAPE_DIGITS[index].to_string());
            } else {
                context.direct_commit(&ch.to_string());
            }
            *env.dot_armed = true;
            return Ok(ProcessorResult::Consume);
        }
        let is_letter = ch.is_ascii_lowercase();
        let live_before = live_input(context);
        let caret = input_caret(context);
        let mut full_before = state.committed_raw.as_bytes().to_vec();
        full_before.extend_from_slice(&live_before);
        if caret != live_before.len() {
            live.pending.clear();
            live.baseline = None;
            invalidate_edit_state(
                context,
                state,
                state.committed_raw.len() + caret,
                full_before.len() + ch.len_utf8(),
            );
            context.push_input(ch.to_string().as_bytes());
            return Ok(ProcessorResult::Consume);
        }
        if state.tab_pending && is_letter {
            let target = context
                .composition
                .back()
                .map(|segment| segment.selected_index)
                .unwrap_or(0);
            let lock = state.active_lock().map(|lock| DecodeLock {
                raw: &lock.raw,
                text: &lock.text,
                boundaries: &lock.boundaries,
            });
            let raw_text = String::from_utf8_lossy(&full_before).into_owned();
            let decoded =
                decoder.decode_with_lock(&raw_text, false, &state.committed_text, lock)?;
            let mut candidate: Option<Selected> = None;
            let mut visible = 0usize;
            for item in &decoded.items {
                if implicit_rank_allowed(
                    item,
                    &full_before,
                    state.continuation_after_auto_commit,
                    allow_duplicate_single,
                ) && item.text.starts_with(&state.committed_text)
                    && item.text.len() > state.committed_text.len()
                {
                    if visible == target {
                        let (raw_length, diff) = decoder.path_summary(item);
                        candidate = Some(Selected {
                            text: item.text.clone(),
                            raw_length,
                            diff,
                        });
                        break;
                    }
                    visible += 1;
                }
            }
            if let Some(candidate) = candidate {
                if candidate.raw_length > state.committed_raw.len() {
                    learning_stage(live, state, Some(&candidate), &full_before, None, env.now);
                    state.tab_pending = false;
                    let boundaries: String = candidate
                        .diff
                        .path
                        .iter()
                        .map(|node| format!("{},{};", node.raw_length, node.text_length))
                        .collect();
                    let locked_raw =
                        String::from_utf8_lossy(&full_before[..candidate.raw_length]).into_owned();
                    state.locks.push(Lock {
                        raw: locked_raw.clone(),
                        text: candidate.text.clone(),
                        boundaries,
                    });
                    let commit = if context.get_option(OPTION_EARLY_COMMIT) {
                        let commit = candidate.text[state.committed_text.len()..].to_string();
                        state.committed_text = candidate.text.clone();
                        state.committed_raw = locked_raw;
                        Some(commit)
                    } else {
                        None
                    };
                    reset_early_evidence(state);
                    state.empty_code_pending = None;
                    state.suspended = false;
                    state.continuation_after_auto_commit = false;
                    state.save(context);
                    if let Some(commit) = commit {
                        if let Some(text) = submit_early(context, state, &commit) {
                            context_commit(context, &text);
                        }
                    }
                    let mut restored = full_before[state.committed_raw.len()..].to_vec();
                    restored.extend_from_slice(ch.to_string().as_bytes());
                    restore_composition_input(context, &restored);
                    return Ok(ProcessorResult::Consume);
                }
            }
        }
        state.tab_pending = false;
        live.baseline = None;
        if !is_letter {
            state.empty_code_pending = None;
            state.save(context);
        }
        context.push_input(ch.to_string().as_bytes());
        if is_letter
            && try_empty_code_commit(
                decoder,
                context,
                state,
                &full_before,
                ch.to_string().as_bytes(),
                params,
            )?
        {
            return Ok(ProcessorResult::Consume);
        }
        try_early_commit(decoder, context, state, params)?;
        return Ok(ProcessorResult::Consume);
    }
    if !context.is_composing() {
        if dot_armed
            && (repr == "period" || repr == "KP_Decimal")
            && !key_event.shift()
            && !key_event.ctrl()
            && !key_event.alt()
            && !key_event.super_modifier()
        {
            context.direct_commit(".");
            return Ok(ProcessorResult::Consume);
        }
        return Ok(ProcessorResult::Forward);
    }
    // 缓冲态遇可打印标点：先确认组合，再把原键交标点表。
    let codepoint = key_event.keycode;
    if !state.buffered_text.is_empty()
        && (33..=126).contains(&codepoint)
        && (codepoint as u8 as char).is_ascii_punctuation()
        && !key_event.ctrl()
        && !key_event.alt()
        && !key_event.super_modifier()
    {
        context.confirm_current_selection();
        return Ok(ProcessorResult::Forward);
    }
    if repr == "Return" || repr == "KP_Enter" {
        live.pending.clear();
        live.baseline = None;
        let text = format!(
            "{}{}",
            state.buffered_text,
            String::from_utf8_lossy(&live_input(context))
        );
        context.direct_commit(&text);
        context.clear();
        state.reset(context, false);
        return Ok(ProcessorResult::Consume);
    }
    if repr == "Escape" {
        live.pending.clear();
        live.baseline = None;
        context.clear();
        state.reset(context, false);
        return Ok(ProcessorResult::Consume);
    }
    if repr == "BackSpace" || repr == "Delete" {
        live.pending.clear();
        live.baseline = None;
        state.tab_pending = false;
        reset_early_evidence(state);
        state.empty_code_pending = None;
        if !state.buffered_text.is_empty() {
            let raw = live_input(context);
            let caret = input_caret(context);
            if repr == "BackSpace" && raw.is_empty() {
                let mut letters: Vec<char> = state.buffered_text.chars().collect();
                let removed = letters.pop();
                state.buffered_text = letters.into_iter().collect();
                let removed_length = removed.map(char::len_utf8).unwrap_or(0);
                let keep = state.committed_text.len().saturating_sub(removed_length);
                state.committed_text.truncate(keep);
                if state.buffered_text.is_empty() {
                    state.reset(context, false);
                } else {
                    state.locks = vec![Lock {
                        raw: state.committed_raw.clone(),
                        text: state.committed_text.clone(),
                        boundaries: format!(
                            "{},{};",
                            state.committed_raw.len(),
                            state.committed_text.len()
                        ),
                    }];
                    state.save(context);
                }
                restore_composition_input(context, &raw);
                return Ok(ProcessorResult::Consume);
            }
            let first = if repr == "BackSpace" {
                caret as isize - 1
            } else {
                caret as isize
            };
            if first < 0 || first >= raw.len() as isize {
                return Ok(ProcessorResult::Consume);
            }
            let first = first as usize;
            let mut remaining = raw[..first].to_vec();
            remaining.extend_from_slice(&raw[first + 1..]);
            invalidate_edit_state(
                context,
                state,
                state.committed_raw.len() + first,
                state.committed_raw.len() + remaining.len(),
            );
            restore_composition_input(context, &remaining);
            context.set_caret(first + 1);
            return Ok(ProcessorResult::Consume);
        }
        if state.active_lock().is_some() {
            let raw = live_input(context);
            let caret = input_caret(context);
            let first = if repr == "BackSpace" {
                caret as isize - 1
            } else {
                caret as isize
            };
            if first < 0 || first >= raw.len() as isize {
                state.save(context);
                return Ok(ProcessorResult::Forward);
            }
            let first = first as usize;
            let mut remaining = raw[..first].to_vec();
            remaining.extend_from_slice(&raw[first + 1..]);
            invalidate_edit_state(
                context,
                state,
                state.committed_raw.len() + first,
                state.committed_raw.len() + remaining.len(),
            );
            if remaining.is_empty() {
                context.clear();
                state.reset(context, false);
            } else if repr == "BackSpace" {
                context.pop_input(1);
            } else {
                context.delete_input(1);
            }
            return Ok(ProcessorResult::Consume);
        }
        state.save(context);
        return Ok(ProcessorResult::Forward);
    }
    if repr == "Left" || repr == "Right" || repr == "Home" || repr == "End" {
        live.pending.clear();
        live.baseline = None;
        // 手动光标导航不保留追加证据与待确认 Tab。
        state.tab_pending = false;
        reset_early_evidence(state);
        state.empty_code_pending = None;
        state.save(context);
        if !state.buffered_text.is_empty()
            && (repr == "Home" || (repr == "Left" && input_caret(context) == 0))
        {
            context.set_caret(1);
            return Ok(ProcessorResult::Consume);
        }
        return Ok(ProcessorResult::Forward);
    }
    if repr == "Tab" || repr == "ISO_Left_Tab" || repr == "Shift+Tab" {
        if !state.tab_pending && live.store_ready {
            let selection = learning_selection(decoder, context, state)?;
            live.baseline = selection.first;
        }
        reset_early_evidence(state);
        state.suspended = true;
        state.empty_code_pending = None;
        state.save(context);
        if cycle_candidate_highlight(context, if repr == "Tab" { 1 } else { -1 }) {
            state.tab_pending = true;
            state.save(context);
            return Ok(ProcessorResult::Consume);
        }
        // 菜单不可用时交给 schema 的 Down/Up 绑定与 navigate。
        return Ok(ProcessorResult::Forward);
    }
    if repr == "Up" || repr == "Down" || repr == "Page_Up" || repr == "Page_Down" {
        reset_early_evidence(state);
        state.suspended = true;
        state.empty_code_pending = None;
        state.save(context);
        return Ok(ProcessorResult::Forward);
    }
    if repr == "space" {
        if context.has_menu() {
            let selection = learning_selection(decoder, context, state)?;
            learning_stage(
                live,
                state,
                selection.selected.as_ref(),
                &selection.raw,
                None,
                env.now,
            );
            context.confirm_current_selection();
        }
        live.pending.clear();
        live.baseline = None;
        state.reset(context, false);
        return Ok(ProcessorResult::Consume);
    }
    Ok(ProcessorResult::Forward)
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

    #[test]
    fn load_migrates_legacy_committed_properties() {
        let mut context = Context::new();
        context.set_property(K_COMMITTED_RAW_LEGACY, "ab");
        context.set_property(K_COMMITTED_TEXT_LEGACY, "甲");
        let mut state = SentenceState::fresh(1);
        state.load(&mut context, 1);
        assert_eq!(state.committed_raw, "ab");
        assert_eq!(state.committed_text, "甲");
        assert_eq!(context.get_property(K_COMMITTED), Some("ab\t甲"));
        assert_eq!(context.get_property(K_COMMITTED_RAW_LEGACY), None);
        assert_eq!(context.get_property(K_COMMITTED_TEXT_LEGACY), None);
    }

    #[test]
    fn save_clears_legacy_keys_once() {
        let mut context = Context::new();
        context.set_property(K_CONFIDENCE_LEGACY, "x");
        context.set_property(K_EVIDENCE_RAW_LEGACY, "y");
        let mut state = SentenceState::fresh(1);
        state.save(&mut context);
        assert!(state.legacy_cleared);
        assert_eq!(context.get_property(K_CONFIDENCE_LEGACY), None);
        assert_eq!(context.get_property(K_EVIDENCE_RAW_LEGACY), None);
    }

    #[test]
    fn model_generation_change_resets_transients() {
        let (context, mut state) = state_with_lock("ab", "甲");
        let _ = context;
        state.last_seen_raw = "raw".to_string();
        assert!(state.synchronize_model_state(2));
        assert!(state.last_seen_raw.is_empty());
        assert!(!state.synchronize_model_state(2));
        assert_eq!(state.model_generation, 2);
    }

    #[test]
    fn duplicate_single_option_reads_context() {
        let mut context = Context::new();
        // 参照 `set_allow_duplicate_single`：缺省 true，仅显式关闭为 false。
        assert!(set_allow_duplicate_single(&context));
        context.set_option(OPTION_ALLOW_DUPLICATE_SINGLE, false);
        assert!(!set_allow_duplicate_single(&context));
        context.set_option(OPTION_ALLOW_DUPLICATE_SINGLE, true);
        assert!(set_allow_duplicate_single(&context));
    }

    #[test]
    fn early_commit_helpers() {
        // has_selection_suffix
        assert!(has_selection_suffix(b"ab1"));
        assert!(has_selection_suffix(b"ab;"));
        assert!(has_selection_suffix(b"ab'"));
        assert!(!has_selection_suffix(b"abc"));
        // common_text_prefix
        assert_eq!(common_text_prefix("甲乙丙", "甲乙丁"), "甲乙");
        assert_eq!(common_text_prefix("甲", "乙"), "");
        // tracker_better：字符数优先，其次份额，最后短边界
        let base = Tracker {
            text: "甲".to_string(),
            text_char_count: 1,
            raw_length: 2,
            evidence_count: 0,
            strong_count: 0,
            gap_count: 0,
            last_share: 0.9,
        };
        let mut longer = base.clone();
        longer.text = "甲乙".to_string();
        longer.text_char_count = 2;
        assert!(tracker_better(&longer, &base));
        let mut higher_share = base.clone();
        higher_share.last_share = 0.99;
        assert!(tracker_better(&higher_share, &base));
        let mut shorter_boundary = base.clone();
        shorter_boundary.raw_length = 1;
        assert!(tracker_better(&shorter_boundary, &base));
    }

    #[test]
    fn prefix_evidence_retention_and_contradiction() {
        use crate::decode::{Evidence, PrefixEvidence};
        let prefix = |text: &str, raw: usize, share: f64| PrefixEvidence {
            text: text.to_string(),
            raw_length: raw,
            share,
            boundary_share: share,
            boundary_closed: share >= 0.99999,
            text_char_count: text.chars().count(),
        };
        let evidence = Evidence {
            prefixes: vec![prefix("甲", 2, 0.5), prefix("甲乙", 2, 0.3)],
            by_boundary: [(
                2usize,
                [("甲".to_string(), 0), ("甲乙".to_string(), 1)]
                    .into_iter()
                    .collect(),
            )]
            .into_iter()
            .collect(),
            proposal: String::new(),
            proposal_share: 0.0,
            raw_lengths: Default::default(),
            neutral_incomplete_tail: false,
            merged_incomplete_tail: false,
            neutral_low_confidence: false,
            confidence_truncated: false,
        };
        // "甲乙" 与同名 tracker 不矛盾；含更高份额的异名共享词干前缀则矛盾。
        let tracker = Tracker {
            text: "甲乙".to_string(),
            text_char_count: 2,
            raw_length: 2,
            evidence_count: 1,
            strong_count: 0,
            gap_count: 0,
            last_share: 0.3,
        };
        assert!(!prefix_contradicted(&tracker, &evidence));
        let other = Tracker {
            text: "甲丙".to_string(),
            text_char_count: 2,
            raw_length: 2,
            evidence_count: 1,
            strong_count: 0,
            gap_count: 0,
            last_share: 0.2,
        };
        assert!(prefix_contradicted(&other, &evidence));
        // retain：缺失或矛盾时丢弃，保留时 gap+1 且最多 3 次
        let mut trackers = HashMap::new();
        trackers.insert("keep".to_string(), other.clone());
        trackers.insert(
            "gone".to_string(),
            Tracker {
                text: "不存在".to_string(),
                ..other.clone()
            },
        );
        let retained = retain_trackers_without_counting(&trackers, &evidence);
        assert!(retained.is_empty());
        let mut stable = tracker.clone();
        stable.gap_count = 3;
        stable.last_share = 0.3;
        let mut map = HashMap::new();
        map.insert("stable".to_string(), stable.clone());
        let retained = retain_trackers_without_counting(&map, &evidence);
        assert!(retained.is_empty(), "gap_count 超过上限应丢弃");
    }

    #[test]
    fn implicit_rank_and_submit_early() {
        let candidate = Evaluated {
            text: "甲乙".to_string(),
            score: 0.0,
            confidence_score: 0.0,
            max_rank: 2,
            supplement_score: 0.0,
            learning_score: 0.0,
            edge_count: 1,
            path: 0,
            segmented: String::new(),
            previous_raw_length: 2,
            previous_text: Some("甲".to_string()),
        };
        assert!(implicit_rank_allowed(&candidate, b"ab", false, true));
        assert!(!implicit_rank_allowed(&candidate, b"ab", true, false));
        assert!(implicit_rank_allowed(&candidate, b"ab", true, true));
        assert!(implicit_rank_allowed(&candidate, b"ab1", true, false));

        let mut context = Context::new();
        let mut state = SentenceState::fresh(1);
        state.committed_raw = "ab".to_string();
        state.committed_text = "甲".to_string();
        assert_eq!(
            submit_early(&mut context, &mut state, "乙"),
            Some("乙".to_string())
        );
        context.set_option(OPTION_EARLY_COMMIT_TO_PREEDIT, true);
        assert_eq!(submit_early(&mut context, &mut state, "丙"), None);
        assert_eq!(state.buffered_text, "丙");
        assert_eq!(state.locks.len(), 1);
        assert_eq!(state.locks[0].raw, "ab");
    }

    #[test]
    fn reset_empties_committed_and_locks() {
        let (mut context, mut state) = state_with_lock("ab", "甲");
        state.reset(&mut context, true);
        assert!(state.committed_raw.is_empty());
        assert!(state.locks.is_empty());
        assert!(state.continuation_after_auto_commit);
        assert_eq!(context.get_property(K_COMMITTED), Some("\t"));
        assert_eq!(context.get_property(K_LOCKS), None);
    }

    #[test]
    fn trim_segmented_prefix() {
        assert_eq!(trim_segmented_after_raw_prefix("ab cd ef", 2), "cd ef");
        assert_eq!(trim_segmented_after_raw_prefix("ab cd", 1), "b cd");
        assert_eq!(trim_segmented_after_raw_prefix("ab", 2), "");
        assert_eq!(trim_segmented_after_raw_prefix("", 3), "");
        assert_eq!(trim_segmented_after_raw_prefix("ab", 0), "ab");
    }

    #[test]
    fn reverse_comment_formats() {
        let dir =
            std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../goldens/lexicon");
        let lexicon = Lexicon::load(std::slice::from_ref(&dir), 1500);
        // 来：codes.txt 源序 a, ah, ahb
        assert_eq!(
            reverse_comment(&lexicon, "来").expect("来 has codes"),
            " a / ah / ahb"
        );
        let multi = reverse_comment(&lexicon, "来X").expect("multi");
        assert!(multi.starts_with(" 来:"), "{multi}");
        assert!(multi.contains(" X:?"), "{multi}");
        assert!(reverse_comment(&lexicon, "X").is_none());
        assert!(reverse_comment(&lexicon, "").is_none());
    }

    #[test]
    fn buffer_filter_keeps_only_buffered() {
        let plain = Candidate::new("sentence", 0, 2, "甲", "");
        let buffered = Candidate::new("sentence_buffered", 0, 2, "乙", "");
        let all = vec![plain.clone(), buffered.clone()];
        assert_eq!(buffer_filter(&all, false), all);
        assert_eq!(buffer_filter(&all, true), vec![buffered]);
    }

    #[test]
    fn translate_smoke() {
        let dir =
            std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../goldens/lexicon");
        let lexicon = Lexicon::load(std::slice::from_ref(&dir), 1500);
        let supplement = crate::lexicon::Supplement::load_default(Some(&dir));
        let mut decoder = Decoder::new(lexicon, supplement, None);
        let context = Context::new();
        let state = SentenceState::fresh(1);
        let mut out = Vec::new();
        translate(&mut decoder, &context, &state, b"ab", 0, 2, &mut out).expect("translate");
        assert!(!out.is_empty());
        assert!(out.iter().all(|candidate| candidate.kind == "sentence"));
        assert!(out.iter().all(|candidate| !candidate.text.is_empty()));
        // 缓冲态：`~` 标记 + 单锁 → buffered 快捷候选
        let mut buffered_state = SentenceState::fresh(1);
        buffered_state.buffered_text = "甲".to_string();
        buffered_state.committed_raw = "ab".to_string();
        buffered_state.committed_text = "甲".to_string();
        buffered_state.locks.push(Lock {
            raw: "ab".to_string(),
            text: "甲".to_string(),
            boundaries: "2,3;".to_string(),
        });
        let mut buffered_out = Vec::new();
        translate(
            &mut decoder,
            &context,
            &buffered_state,
            b"~",
            0,
            1,
            &mut buffered_out,
        )
        .expect("translate buffered");
        assert_eq!(buffered_out.len(), 1);
        assert_eq!(buffered_out[0].kind, "sentence_buffered");
        assert_eq!(buffered_out[0].preedit, "甲");
    }

    #[test]
    fn translate_guards() {
        let dir =
            std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../goldens/lexicon");
        let lexicon = Lexicon::load(std::slice::from_ref(&dir), 1500);
        let supplement = crate::lexicon::Supplement::load_default(Some(&dir));
        let mut decoder = Decoder::new(lexicon, supplement, None);
        let context = Context::new();
        let state = SentenceState::fresh(1);
        // 反查段（` 前缀）由 reverse lookup 处理，translator 不产出候选。
        let mut out = Vec::new();
        translate(&mut decoder, &context, &state, b"`ni", 0, 3, &mut out).expect("translate");
        assert!(out.is_empty());
        // 缓冲态下非零起点（后续段）不翻译。
        let mut buffered_state = SentenceState::fresh(1);
        buffered_state.buffered_text = "甲".to_string();
        let mut out = Vec::new();
        translate(
            &mut decoder,
            &context,
            &buffered_state,
            b"~ab",
            2,
            5,
            &mut out,
        )
        .expect("translate");
        assert!(out.is_empty());
        // 缓冲态缺少 `~` 标记同样不翻译。
        let mut out = Vec::new();
        translate(
            &mut decoder,
            &context,
            &buffered_state,
            b"ab",
            0,
            2,
            &mut out,
        )
        .expect("translate");
        assert!(out.is_empty());
    }

    fn lexicon_fixture() -> Decoder {
        let dir =
            std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../goldens/lexicon");
        let lexicon = Lexicon::load(std::slice::from_ref(&dir), 1500);
        let supplement = crate::lexicon::Supplement::load_default(Some(&dir));
        Decoder::new(lexicon, supplement, None)
    }

    #[test]
    fn learning_selection_shapes() {
        let mut decoder = lexicon_fixture();
        let mut context = Context::new();
        let state = SentenceState::fresh(1);
        // 无输入 → 无选中项
        let selection = learning_selection(&mut decoder, &context, &state).expect("selection");
        assert!(selection.selected.is_none() && selection.first.is_none());
        // 有实时输入 → selected == first（目标序号 0），raw = 已确认 + 实时
        context.push_input(b"abab");
        let selection = learning_selection(&mut decoder, &context, &state).expect("selection");
        assert_eq!(selection.raw, b"abab");
        let selected = selection.selected.clone().expect("selected");
        assert_eq!(
            selection.first.as_ref().map(|item| &item.text),
            Some(&selected.text)
        );
        assert!(selected.text.starts_with('交'));
        assert_eq!(selected.raw_length, 4);
        assert!(selected.diff.path.len() >= 2);
        // 缓冲空闲兜底：已确认前缀即选中项（无可见候选）
        let mut buffered_state = SentenceState::fresh(1);
        buffered_state.committed_raw = "ab".to_string();
        buffered_state.committed_text = "交".to_string();
        buffered_state.buffered_text = "交".to_string();
        let context = Context::new();
        let selection =
            learning_selection(&mut decoder, &context, &buffered_state).expect("selection");
        assert!(selection.first.is_none());
        let selected = selection.selected.expect("buffered selected");
        assert_eq!(selected.text, "交");
        assert_eq!(selected.raw_length, 2);
        assert_eq!(selected.diff.path.len(), 1);
    }

    // 交交/交疒 的手工路径（码表事实：ab → 交 rank1、疒 rank2）。
    fn diff_item(text: &str) -> DiffItem {
        DiffItem {
            text: text.to_string(),
            path: vec![
                DiffPathNode {
                    raw_length: 2,
                    text_length: 3,
                },
                DiffPathNode {
                    raw_length: 4,
                    text_length: 6,
                },
            ],
        }
    }

    fn selected_item(text: &str) -> Selected {
        Selected {
            text: text.to_string(),
            raw_length: 4,
            diff: diff_item(text),
        }
    }

    #[test]
    fn learning_stage_pends_and_submit_consumes() {
        let mode = "sentence-v1|rules=|optimal=1500|dup=1";
        let baseline = selected_item("交交");
        let selected = selected_item("交疒");
        let mut state = SentenceState::fresh(1);
        state.committed_raw = "ab".to_string();
        state.committed_text = "交".to_string();
        let mut live = LiveLearning {
            mode: mode.to_string(),
            ..LiveLearning::default()
        };
        // 非 Tab 流程：baseline 取 submitted_first（首个可见候选）
        learning_stage(
            &mut live,
            &state,
            Some(&selected),
            b"abab",
            Some(&baseline),
            100.0,
        );
        assert_eq!(live.pending.len(), 1);
        assert_eq!(live.pending[0].text, "疒");
        assert_eq!(live.pending[0].code, "ab");
        assert_eq!(live.pending[0].time, 100.0);
        assert_eq!(live.pending[0].raw_start, 2);
        assert_eq!(live.pending[0].text_start, 3);
        assert!(live.baseline.is_none());
        // 提交匹配 → 接受事件并无条件清空 pending
        let accepted = learning_submit(&mut live, Some(&selected), "交疒", "交疒");
        assert_eq!(accepted.len(), 1);
        assert_eq!(accepted[0].text, "疒");
        assert_eq!(accepted[0].mode, mode);
        assert!(live.pending.is_empty());
        // 提交不匹配 → 事件被丢弃（消费语义），不会重复强化
        learning_stage(
            &mut live,
            &state,
            Some(&selected),
            b"abab",
            Some(&baseline),
            200.0,
        );
        assert_eq!(live.pending.len(), 1);
        let accepted = learning_submit(&mut live, Some(&selected), "交", "交疒");
        assert!(accepted.is_empty());
        assert!(live.pending.is_empty());
    }

    #[test]
    fn learning_stage_keeps_tab_baseline_until_used() {
        let mode = "m";
        let baseline = selected_item("交交");
        let selected = selected_item("交疒");
        let mut state = SentenceState::fresh(1);
        state.tab_pending = true;
        let mut live = LiveLearning {
            mode: mode.to_string(),
            baseline: Some(baseline.clone()),
            ..LiveLearning::default()
        };
        // Tab 流程使用 live.baseline（而非 submitted_first）
        learning_stage(
            &mut live,
            &state,
            Some(&selected),
            b"abab",
            Some(&selected),
            0.0,
        );
        assert_eq!(live.pending.len(), 1);
        assert_eq!(live.pending[0].text, "疒");
        assert!(live.baseline.is_none());
        // mode 为空 → 不暂存
        let mut idle = LiveLearning::default();
        learning_stage(
            &mut idle,
            &state,
            Some(&selected),
            b"abab",
            Some(&baseline),
            0.0,
        );
        assert!(idle.pending.is_empty());
    }

    #[test]
    fn translate_applies_duplicate_single_option() {
        let mut decoder = lexicon_fixture();
        let mut context = Context::new();
        let state = SentenceState::fresh(1);
        context.set_option(OPTION_ALLOW_DUPLICATE_SINGLE, false);
        let mut out = Vec::new();
        translate(&mut decoder, &context, &state, b"abab", 0, 4, &mut out).expect("translate");
        let texts: Vec<String> = out.iter().map(|candidate| candidate.text.clone()).collect();
        assert!(!texts.is_empty());
        assert!(!texts.iter().any(|text| text.contains('疒')), "{texts:?}");
        context.set_option(OPTION_ALLOW_DUPLICATE_SINGLE, true);
        let mut out = Vec::new();
        translate(&mut decoder, &context, &state, b"abab", 0, 4, &mut out).expect("translate");
        let texts: Vec<String> = out.iter().map(|candidate| candidate.text.clone()).collect();
        assert!(texts.iter().any(|text| text.contains('疒')), "{texts:?}");
    }

    fn key_of(repr: &str) -> KeyEvent {
        KeyEvent::from_repr(repr).expect("key repr")
    }

    struct Harness {
        decoder: Decoder,
        context: Context,
        state: SentenceState,
        live: LiveLearning,
        dot_armed: bool,
    }

    impl Harness {
        fn new() -> Self {
            Self {
                decoder: lexicon_fixture(),
                context: Context::new(),
                state: SentenceState::fresh(1),
                live: LiveLearning::default(),
                dot_armed: false,
            }
        }

        fn press_event(&mut self, key: &KeyEvent) -> ProcessorResult {
            let mut env = ProcessorEnv {
                now: 0.0,
                dot_armed: &mut self.dot_armed,
                min_retained: None,
            };
            processor(
                key,
                &mut self.context,
                &mut self.state,
                &mut self.decoder,
                &mut self.live,
                &mut env,
            )
            .expect("processor")
        }

        fn press(&mut self, repr: &str) -> ProcessorResult {
            let key = key_of(repr);
            self.press_event(&key)
        }

        fn push_segment(&mut self, input: &[u8], texts: &[&str]) {
            self.context.set_input(input);
            let candidates = texts
                .iter()
                .map(|text| Candidate::new("sentence", 0, input.len(), text, ""))
                .collect();
            self.context.composition.segments.push(Segment {
                start: 0,
                end: input.len(),
                tags: Vec::new(),
                selected_index: 0,
                candidates,
                selected: false,
            });
        }
    }

    #[test]
    fn processor_idle_keys_and_dot_armed() {
        let mut h = Harness::new();
        // 释放事件交宿主
        let release = KeyEvent::new(
            crate::key::keycode_by_name("a").expect("a"),
            crate::key::K_RELEASE_MASK,
        );
        assert_eq!(h.press_event(&release), ProcessorResult::Forward);
        // 空闲分号/引号交标点处理器
        assert_eq!(h.press("semicolon"), ProcessorResult::Forward);
        assert_eq!(h.press("apostrophe"), ProcessorResult::Forward);
        // 空闲数字直接上屏并置待发
        assert_eq!(h.press("5"), ProcessorResult::Consume);
        assert_eq!(h.context.get_commit_text(), "5");
        assert!(h.dot_armed);
        // 紧随的句点按 ASCII 小数点上屏
        assert_eq!(h.press("period"), ProcessorResult::Consume);
        assert_eq!(h.context.get_commit_text(), ".");
        assert!(!h.dot_armed);
        // 无待发状态时句点交宿主
        assert_eq!(h.press("period"), ProcessorResult::Forward);
    }

    #[test]
    fn processor_types_and_commits() {
        let mut h = Harness::new();
        assert_eq!(h.press("a"), ProcessorResult::Consume);
        assert_eq!(h.press("b"), ProcessorResult::Consume);
        assert_eq!(h.context.input(), b"ab");
        // 真实会话中组合由 translator 建立；这里手工合成后再走提交/清空分支。
        h.push_segment(b"ab", &["交"]);
        // Return：提交「缓冲 + 实时输入」并清空
        assert_eq!(h.press("Return"), ProcessorResult::Consume);
        assert_eq!(h.context.get_commit_text(), "ab");
        assert!(h.context.input().is_empty());
        // Escape：直接清空
        h.push_segment(b"a", &["甲"]);
        assert_eq!(h.press("Escape"), ProcessorResult::Consume);
        assert!(h.context.input().is_empty());
        assert!(h.state.committed_raw.is_empty());
    }

    #[test]
    fn lexicon_learning_rules_matches_oracle() {
        let dir =
            std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../goldens/lexicon");
        let lexicon = Lexicon::load(std::slice::from_ref(&dir), 1500);
        // oracle：参照 `learning.hash(codes.."\0"..ranks.."\0"..whitelist)`（pin 版 Lua 直算）。
        assert_eq!(lexicon.learning_rules, "99f336c6e74e055e");
        // 数据缺失时三份内容均为空串。
        let missing = Lexicon::load(&[], 1500);
        assert_eq!(missing.learning_rules, crate::learning::hash("\0\0"));
    }

    #[test]
    fn options_sync_and_observe() {
        let mut context = Context::new();
        let mut options = Options::new(option_defaults());
        options.sync(&mut context);
        assert!(context.get_option("tiger_sentence_early_commit"));
        assert!(context.get_option("tiger_sentence_allow_duplicate_single"));
        assert!(!context.get_option("tiger_sentence_early_commit_to_preedit"));
        // 用户改选项 → observe 记录并请求持久化；重复观察不再请求
        context.set_option("tiger_sentence_early_commit_to_preedit", true);
        assert!(options.observe(&context, "tiger_sentence_early_commit_to_preedit"));
        assert_eq!(options.revision, 1);
        assert!(!options.observe(&context, "tiger_sentence_early_commit_to_preedit"));
        assert!(!options.observe(&context, "other_option"));
        // 持久化值优先于 schema 缺省
        options
            .values
            .insert("tiger_sentence_early_commit".to_string(), false);
        context.set_option("tiger_sentence_early_commit", true);
        options.sync(&mut context);
        assert!(!context.get_option("tiger_sentence_early_commit"));
    }

    #[test]
    fn ascii_switch_styles_normalize_buffered_exits() {
        let mut source = HashMap::new();
        source.insert("Shift_L".to_string(), "commit_code".to_string());
        source.insert("Shift_R".to_string(), "inline_ascii".to_string());
        source.insert("Control_L".to_string(), "noop".to_string());
        let styles = ascii_switch_styles(&source);
        assert_eq!(styles["Shift_L"], "commit_text");
        assert_eq!(styles["Shift_R"], "commit_text");
        assert_eq!(styles["Control_L"], "noop");
        assert_eq!(styles["Caps_Lock"], "noop");
        assert_eq!(styles.len(), ASCII_SWITCH_KEYS.len());
    }

    #[test]
    fn processor_inserts_at_caret() {
        let mut h = Harness::new();
        h.context.set_input(b"ab");
        h.context.set_caret(1);
        assert_eq!(h.press("c"), ProcessorResult::Consume);
        assert_eq!(h.context.input(), b"acb");
        assert_eq!(h.context.caret(), 2);
    }

    #[test]
    fn processor_navigation_and_buffer_guard() {
        let mut h = Harness::new();
        // 缓冲空闲：菜单导航键拦给宿主
        h.state.buffered_text = "交".to_string();
        assert_eq!(h.press("Tab"), ProcessorResult::Consume);
        assert_eq!(h.press("Up"), ProcessorResult::Consume);
        // 无缓冲：Up 交宿主；Tab 无菜单可用时同样交宿主
        h.state.buffered_text.clear();
        assert_eq!(h.press("Up"), ProcessorResult::Forward);
        assert_eq!(h.press("Tab"), ProcessorResult::Forward);
    }

    #[test]
    fn processor_space_confirms_and_backspace_edits_locked_input() {
        let mut h = Harness::new();
        h.push_segment(b"ab", &["交"]);
        assert_eq!(h.press("space"), ProcessorResult::Consume);
        assert!(h.state.committed_raw.is_empty());
        // 锁分支：退格在锁下走 pop_input
        h.push_segment(b"ab", &["交"]);
        h.state.locks.push(Lock {
            raw: "a".to_string(),
            text: "交".to_string(),
            boundaries: "1,3;".to_string(),
        });
        h.state.committed_raw = "a".to_string();
        h.state.committed_text = "交".to_string();
        assert_eq!(h.press("BackSpace"), ProcessorResult::Consume);
        assert_eq!(h.context.input(), b"a");
    }
}
