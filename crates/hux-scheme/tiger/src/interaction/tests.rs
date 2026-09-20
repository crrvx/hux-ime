// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;
use hux_core::session::Segment;

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
}

#[test]
fn locks_round_trip_with_empty_raw() {
    // 空字段同样可往返（0: 帧）。
    let (mut context, mut state) = (Context::new(), SentenceState::fresh(1));
    state.locks.push(Lock {
        raw: String::new(),
        text: "甲".to_string(),
        boundaries: "0,3;".to_string(),
    });
    state.save(&mut context);
    assert_eq!(read_locks(&context), state.locks);
}

#[test]
fn read_locks_rejects_malformed_framing() {
    let mut context = Context::new();
    context.set_property(K_LOCKS, "9:ab");
    assert!(read_locks(&context).is_empty());
}

#[test]
fn committed_property_parses_or_empties() {
    assert_eq!(
        parse_committed_property("ab\t甲"),
        ("ab".to_string(), "甲".to_string())
    );
    assert_eq!(
        parse_committed_property("no-tab"),
        (String::new(), String::new())
    );
}

#[test]
fn buffered_property_derives_live_input_and_caret() {
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
fn read_locks_rejects_overflowing_length() {
    let mut context = Context::new();
    // 长度字段溢出/超范围：严格解析返回空表，不得 panic。
    set_property_if_changed(&mut context, K_LOCKS, "18446744073709551615:x");
    assert!(read_locks(&context).is_empty());
    set_property_if_changed(&mut context, K_LOCKS, "99999999999999999999:x");
    assert!(read_locks(&context).is_empty());
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
        segment.candidates.push(hux_core::session::Candidate::new(
            "sentence", 0, 2, text, "",
        ));
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
fn plain_char_key_accepts_printable_chars() {
    let key = KeyEvent::new(0x61, 0);
    assert_eq!(is_plain_char_key(&key, "a"), Some('a'));
    assert_eq!(is_plain_char_key(&key, "semicolon"), Some(';'));
    assert_eq!(is_plain_char_key(&key, "apostrophe"), Some('\''));
    assert_eq!(is_plain_char_key(&key, "7"), Some('7'));
    assert_eq!(is_plain_char_key(&key, "KP_3"), Some('3'));
    assert_eq!(is_plain_char_key(&key, "A"), None);
    assert_eq!(is_plain_char_key(&key, "space"), None);
    let ctrl = KeyEvent::new(0x61, hux_core::key::K_CONTROL_MASK);
    assert_eq!(is_plain_char_key(&ctrl, "a"), None);
}

#[test]
fn is_modifier_repr_matches_modifier_prefixes() {
    assert!(is_modifier_repr("Shift_L"));
    assert!(is_modifier_repr("ISO_Level3_Shift"));
    assert!(is_modifier_repr("Mode_switch"));
    // 参照按前缀匹配：带修饰的组合键同样命中（用于“小数点待发”判定）。
    assert!(is_modifier_repr("Shift+a"));
    assert!(!is_modifier_repr("a"));
}

#[test]
fn ends_with_digit_detects_digit_tail() {
    assert!(ends_with_digit("甲1"));
    assert!(ends_with_digit("甲１"));
    assert!(!ends_with_digit("甲"));
    assert!(!ends_with_digit(""));
}

#[test]
fn min_retained_raw_length_clamps() {
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
fn has_selection_suffix_detects_selectors() {
    assert!(has_selection_suffix(b"ab1"));
    assert!(has_selection_suffix(b"ab;"));
    assert!(has_selection_suffix(b"ab'"));
    assert!(!has_selection_suffix(b"abc"));
}

#[test]
fn common_text_prefix_returns_shared_prefix() {
    assert_eq!(common_text_prefix("甲乙丙", "甲乙丁"), "甲乙");
    assert_eq!(common_text_prefix("甲", "乙"), "");
}

#[test]
fn tracker_better_prefers_chars_share_then_short_boundary() {
    let base = Tracker {
        text: "甲".to_string(),
        text_char_count: 1,
        raw_length: 2,
        evidence_count: 0,
        strong_count: 0,
        gap_count: 0,
        last_share: 0.9,
    };
    // 字符数优先，其次份额，最后短边界
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

fn prefix_evidence() -> crate::decode::Evidence {
    use crate::decode::{Evidence, PrefixEvidence};
    let prefix = |text: &str, raw: usize, share: f64| PrefixEvidence {
        text: text.to_string(),
        raw_length: raw,
        share,
        boundary_share: share,
        boundary_closed: share >= 0.99999,
        text_char_count: text.chars().count(),
    };
    Evidence {
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
    }
}

fn tracker(text: &str, share: f64) -> Tracker {
    Tracker {
        text: text.to_string(),
        text_char_count: text.chars().count(),
        raw_length: 2,
        evidence_count: 1,
        strong_count: 0,
        gap_count: 0,
        last_share: share,
    }
}

#[test]
fn prefix_contradicted_detects_higher_share_stem() {
    let evidence = prefix_evidence();
    // "甲乙" 与同名 tracker 不矛盾；含更高份额的异名共享词干前缀则矛盾。
    assert!(!prefix_contradicted(&tracker("甲乙", 0.3), &evidence));
    assert!(prefix_contradicted(&tracker("甲丙", 0.2), &evidence));
}

#[test]
fn retain_trackers_drops_missing_or_contradicted() {
    let evidence = prefix_evidence();
    // 缺失或矛盾时丢弃
    let mut trackers = HashMap::new();
    trackers.insert("keep".to_string(), tracker("甲丙", 0.2));
    trackers.insert("gone".to_string(), tracker("不存在", 0.2));
    let retained = retain_trackers_without_counting(&trackers, &evidence);
    assert!(retained.is_empty());
    // gap_count 超过上限（3）应丢弃
    let mut stable = tracker("甲乙", 0.3);
    stable.gap_count = 3;
    let mut map = HashMap::new();
    map.insert("stable".to_string(), stable);
    let retained = retain_trackers_without_counting(&map, &evidence);
    assert!(retained.is_empty(), "gap_count 超过上限应丢弃");
}

fn evaluated_candidate() -> Evaluated {
    Evaluated {
        text: "甲乙".to_string(),
        score: 0.0,
        confidence_score: 0.0,
        code_score: 0.0,
        lexical_score: 0.0,
        max_rank: 2,
        supplement_score: 0.0,
        learning_score: 0.0,
        edge_count: 1,
        path: 0,
        segmented: String::new(),
        previous_raw_length: 2,
        previous_text: Some("甲".to_string()),
    }
}

#[test]
fn implicit_rank_allowed_gates_by_suffix_and_duplicate() {
    let candidate = evaluated_candidate();
    assert!(implicit_rank_allowed(&candidate, b"ab", false, true));
    assert!(!implicit_rank_allowed(&candidate, b"ab", true, false));
    assert!(implicit_rank_allowed(&candidate, b"ab", true, true));
    assert!(implicit_rank_allowed(&candidate, b"ab1", true, false));
}

#[test]
fn submit_early_commits_or_buffers() {
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

/// 参照上游 `test_tiger_sentence_incremental.lua` 新增断言：
/// 排名先验（词先验/学习重排）不得授权与显示首选不一致的自动提交前缀。
#[test]
fn auto_commit_matches_visible_top_guard() {
    assert!(!auto_commit_matches_visible_top(Some("鼎丁"), "甲乙"));
    assert!(auto_commit_matches_visible_top(Some("鼎丁"), "鼎"));
    assert!(auto_commit_matches_visible_top(None, "甲乙"));
}

#[test]
fn code_comment_formats() {
    let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../goldens/lexicon");
    let lexicon = Lexicon::load(std::slice::from_ref(&dir), 1500);
    // 来：codes.txt 源序 a, ah, ahb
    assert_eq!(
        code_comment(&lexicon, "来").expect("来 has codes"),
        " a / ah / ahb"
    );
    let multi = code_comment(&lexicon, "来X").expect("multi");
    assert!(multi.starts_with(" 来:"), "{multi}");
    assert!(multi.contains(" X:?"), "{multi}");
    assert!(code_comment(&lexicon, "X").is_none());
    assert!(code_comment(&lexicon, "").is_none());
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
fn translate_produces_sentence_candidates() {
    let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../goldens/lexicon");
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
}

#[test]
fn translate_emits_buffered_candidate() {
    let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../goldens/lexicon");
    let lexicon = Lexicon::load(std::slice::from_ref(&dir), 1500);
    let supplement = crate::lexicon::Supplement::load_default(Some(&dir));
    let mut decoder = Decoder::new(lexicon, supplement, None);
    let context = Context::new();
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
    let mut out = Vec::new();
    translate(
        &mut decoder,
        &context,
        &buffered_state,
        b"~",
        0,
        1,
        &mut out,
    )
    .expect("translate buffered");
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].kind, "sentence_buffered");
    assert_eq!(out[0].preedit, "甲");
}

#[test]
fn translate_skips_lookup_segments() {
    let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../goldens/lexicon");
    let lexicon = Lexicon::load(std::slice::from_ref(&dir), 1500);
    let supplement = crate::lexicon::Supplement::load_default(Some(&dir));
    let mut decoder = Decoder::new(lexicon, supplement, None);
    let context = Context::new();
    let state = SentenceState::fresh(1);
    // 音反查段（` 前缀）由 `sound_to_char_shape` 模块处理，translator 不产出候选。
    let mut out = Vec::new();
    translate(&mut decoder, &context, &state, b"`ni", 0, 3, &mut out).expect("translate");
    assert!(out.is_empty());
}

#[test]
fn translate_requires_buffer_marker() {
    let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../goldens/lexicon");
    let lexicon = Lexicon::load(std::slice::from_ref(&dir), 1500);
    let supplement = crate::lexicon::Supplement::load_default(Some(&dir));
    let mut decoder = Decoder::new(lexicon, supplement, None);
    let context = Context::new();
    let mut buffered_state = SentenceState::fresh(1);
    buffered_state.buffered_text = "甲".to_string();
    // 缓冲态下非零起点（后续段）不翻译。
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
    let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../goldens/lexicon");
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
    let selection = learning_selection(&mut decoder, &context, &buffered_state).expect("selection");
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
        buffered_fallback: false,
    }
}

fn learning_state() -> SentenceState {
    let mut state = SentenceState::fresh(1);
    state.committed_raw = "ab".to_string();
    state.committed_text = "交".to_string();
    state
}

fn learning_live(mode: &str) -> LiveLearning {
    LiveLearning {
        mode: mode.to_string(),
        ..LiveLearning::default()
    }
}

#[test]
fn learning_stage_pends_event_with_offsets() {
    let mode = "sentence-v1|rules=|optimal=1500|dup=1";
    let baseline = selected_item("交交");
    let selected = selected_item("交疒");
    let state = learning_state();
    let mut live = learning_live(mode);
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
}

#[test]
fn learning_submit_accepts_matching_and_drops_mismatch() {
    let mode = "sentence-v1|rules=|optimal=1500|dup=1";
    let baseline = selected_item("交交");
    let selected = selected_item("交疒");
    let state = learning_state();
    let mut live = learning_live(mode);
    learning_stage(
        &mut live,
        &state,
        Some(&selected),
        b"abab",
        Some(&baseline),
        100.0,
    );
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
fn buffered_fallback_produces_no_learning_events() {
    let mode = "sentence-v1|rules=|optimal=1500|dup=1";
    let baseline = selected_item("交交");
    let fallback = Selected::buffered("ab", "交");
    let mut state = SentenceState::fresh(1);
    state.buffered_text = "交".to_string();
    state.tab_pending = true;
    let mut live = LiveLearning {
        mode: mode.to_string(),
        baseline: Some(baseline.clone()),
        ..LiveLearning::default()
    };
    // stage：兜底项不产出事件，baseline 保留（参照 pcall 吞错的有效行为）
    learning_stage(&mut live, &state, Some(&fallback), b"ab", None, 100.0);
    assert!(live.pending.is_empty());
    assert!(live.baseline.is_some());
    // submit：不消费 pending/baseline
    let accepted = learning_submit(&mut live, Some(&fallback), "交", "交");
    assert!(accepted.is_empty());
    assert!(live.baseline.is_some());
}

fn rebuild(
    builder: &mut CompositionBuilder,
    decoder: &mut Decoder,
    context: &mut Context,
    state: &SentenceState,
    invalidated: bool,
) -> bool {
    builder
        .rebuild(decoder, context, state, invalidated, None)
        .expect("rebuild")
}

#[test]
fn composition_builder_preserves_unchanged_segment() {
    let mut decoder = lexicon_fixture();
    let mut context = Context::new();
    let state = SentenceState::fresh(1);
    let mut builder = CompositionBuilder::default();
    // 首次：建立组合（段存在即可，候选数取决于夹具表）
    context.push_input(b"ab");
    assert!(rebuild(
        &mut builder,
        &mut decoder,
        &mut context,
        &state,
        false
    ));
    let segment = context.composition.back().expect("segment");
    assert_eq!(segment.end, 2);
    assert!(segment.translated);
    // 输入未变：段与菜单保留（标记仍在、高亮不重置）
    context
        .composition
        .back_mut()
        .expect("segment")
        .tags
        .push("marker".to_string());
    context
        .composition
        .back_mut()
        .expect("segment")
        .selected_index = 1;
    rebuild(&mut builder, &mut decoder, &mut context, &state, false);
    let segment = context.composition.back().expect("segment");
    assert!(segment.has_tag("marker"));
    assert_eq!(segment.selected_index, 1);
}

#[test]
fn composition_builder_rebuilds_on_invalidation_or_input_change() {
    let mut decoder = lexicon_fixture();
    let mut context = Context::new();
    let state = SentenceState::fresh(1);
    let mut builder = CompositionBuilder::default();
    context.push_input(b"ab");
    rebuild(&mut builder, &mut decoder, &mut context, &state, false);
    context
        .composition
        .back_mut()
        .expect("segment")
        .tags
        .push("marker".to_string());
    // 提交失效：段重建（标记与高亮消失）
    rebuild(&mut builder, &mut decoder, &mut context, &state, true);
    let segment = context.composition.back().expect("segment");
    assert!(!segment.has_tag("marker"));
    assert_eq!(segment.selected_index, 0);
    // 输入变化：重建（更长的段覆盖旧段）
    context.push_input(b"c");
    rebuild(&mut builder, &mut decoder, &mut context, &state, false);
    assert_eq!(context.composition.back().expect("segment").end, 3);
}

#[test]
fn composition_builder_follows_caret_prefix() {
    let mut decoder = lexicon_fixture();
    let mut context = Context::new();
    let state = SentenceState::fresh(1);
    let mut builder = CompositionBuilder::default();
    context.push_input(b"abc");
    rebuild(&mut builder, &mut decoder, &mut context, &state, false);
    // 光标移入输入中间：组合只覆盖 caret 前缀（参照 Compose 语义）
    context.set_caret(1);
    rebuild(&mut builder, &mut decoder, &mut context, &state, false);
    assert_eq!(context.composition.back().expect("segment").end, 1);
    // 光标移回末尾：重新覆盖完整输入
    context.set_caret(3);
    rebuild(&mut builder, &mut decoder, &mut context, &state, false);
    assert_eq!(context.composition.back().expect("segment").end, 3);
}

#[test]
fn learning_commit_requires_mode_and_input() {
    let mut decoder = lexicon_fixture();
    let context = Context::new();
    let state = SentenceState::fresh(1);
    let mut live = LiveLearning::default();
    // 未就绪 / 无 mode：不记录
    learning_commit(&mut decoder, &context, &state, &mut live, 0.0, "x");
    assert!(live.submitted_raw.is_none());
    live.mode = "m".to_string();
    live.store_ready = true;
    // raw 为空：不记录
    learning_commit(&mut decoder, &context, &state, &mut live, 0.0, "x");
    assert!(live.submitted_raw.is_none());
}

#[test]
fn learning_commit_dedups_same_raw() {
    let mut decoder = lexicon_fixture();
    let mut context = Context::new();
    let state = SentenceState::fresh(1);
    let mut live = LiveLearning {
        mode: "m".to_string(),
        store_ready: true,
        ..LiveLearning::default()
    };
    // raw 非空：记录 submitted_raw；同 raw 再次调用被去重
    context.push_input(b"ab");
    learning_commit(&mut decoder, &context, &state, &mut live, 0.0, "x");
    assert_eq!(live.submitted_raw.as_deref(), Some("ab"));
    let pending_before = live.pending.len();
    learning_commit(&mut decoder, &context, &state, &mut live, 0.0, "x");
    assert_eq!(live.pending.len(), pending_before);
}

#[test]
fn commit_with_learning_queues_accepted_events() {
    let mut decoder = lexicon_fixture();
    let mut context = Context::new();
    let mut state = SentenceState::fresh(1);
    let mut live = LiveLearning {
        mode: "m".to_string(),
        store_ready: true,
        pending: vec![DiffEvent {
            time: 0.0,
            mode: "m".to_string(),
            code: "ab".to_string(),
            text: "疒".to_string(),
            context: String::new(),
            raw_start: 0,
            raw_end: 2,
            text_start: 3,
            text_end: 6,
        }],
        ..LiveLearning::default()
    };
    LearningCommit {
        decoder: &mut decoder,
        live: &mut live,
        now: 0.0,
    }
    .commit_with_learning(&mut context, &mut state, "疒", "交疒", 2);
    // 接受的事件进入持久化队列；pending 被消费
    assert_eq!(live.submitted.len(), 1);
    assert_eq!(live.submitted[0].text, "疒");
    assert!(live.pending.is_empty());
}

#[test]
fn learning_stage_prefers_live_baseline_on_tab() {
    let baseline = selected_item("交交");
    let selected = selected_item("交疒");
    let mut state = SentenceState::fresh(1);
    state.tab_pending = true;
    let mut live = LiveLearning {
        mode: "m".to_string(),
        baseline: Some(baseline),
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
}

#[test]
fn learning_stage_skips_without_mode() {
    let baseline = selected_item("交交");
    let selected = selected_item("交疒");
    let state = SentenceState::fresh(1);
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
    page_size: usize,
}

impl Harness {
    fn new() -> Self {
        Self {
            decoder: lexicon_fixture(),
            context: Context::new(),
            state: SentenceState::fresh(1),
            live: LiveLearning::default(),
            dot_armed: false,
            page_size: 5,
        }
    }

    fn press_event(&mut self, key: &KeyEvent) -> ProcessorResult {
        let mut env = ProcessorEnv {
            now: 0.0,
            dot_armed: &mut self.dot_armed,
            min_retained: None,
            page_size: self.page_size,
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
            prompt: String::new(),
            selected_index: 0,
            candidates,
            selected: false,
            translated: true,
        });
    }
}

#[test]
fn processor_forwards_release_and_idle_punct() {
    let mut h = Harness::new();
    // 释放事件交宿主
    let release = KeyEvent::new(
        hux_core::key::keycode_by_name("a").expect("a"),
        hux_core::key::K_RELEASE_MASK,
    );
    assert_eq!(h.press_event(&release), ProcessorResult::Forward);
    // 空闲分号/引号交标点处理器
    assert_eq!(h.press("semicolon"), ProcessorResult::Forward);
    assert_eq!(h.press("apostrophe"), ProcessorResult::Forward);
}

#[test]
fn processor_commits_idle_digit_and_arms_dot() {
    let mut h = Harness::new();
    // 空闲数字直接上屏并置待发
    assert_eq!(h.press("5"), ProcessorResult::Consume);
    assert_eq!(h.context.last_commit_text(), "5");
    assert!(h.dot_armed);
    // 紧随的句点按 ASCII 小数点上屏
    assert_eq!(h.press("period"), ProcessorResult::Consume);
    assert_eq!(h.context.last_commit_text(), ".");
    assert!(!h.dot_armed);
    // 无待发状态时句点交宿主
    assert_eq!(h.press("period"), ProcessorResult::Forward);
}

#[test]
fn processor_return_commits_buffer_and_input() {
    let mut h = Harness::new();
    assert_eq!(h.press("a"), ProcessorResult::Consume);
    assert_eq!(h.press("b"), ProcessorResult::Consume);
    assert_eq!(h.context.input(), b"ab");
    // 真实会话中组合由 translator 建立；这里手工合成后再走提交/清空分支。
    h.push_segment(b"ab", &["交"]);
    // Return：提交「缓冲 + 实时输入」并清空
    assert_eq!(h.press("Return"), ProcessorResult::Consume);
    assert_eq!(h.context.last_commit_text(), "ab");
    assert!(h.context.input().is_empty());
}

#[test]
fn processor_escape_clears_composition() {
    let mut h = Harness::new();
    h.push_segment(b"a", &["甲"]);
    // Escape：直接清空
    assert_eq!(h.press("Escape"), ProcessorResult::Consume);
    assert!(h.context.input().is_empty());
    assert!(h.state.committed_raw.is_empty());
}

#[test]
fn lexicon_learning_rules_matches_oracle() {
    let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../goldens/lexicon");
    let lexicon = Lexicon::load(std::slice::from_ref(&dir), 1500);
    // oracle：参照 `learning.hash(codes.."\0"..ranks.."\0"..whitelist)`（pin 版 Lua 直算）。
    assert_eq!(lexicon.learning_rules, "99f336c6e74e055e");
    // 数据缺失时三份内容均为空串。
    let missing = Lexicon::load(&[], 1500);
    assert_eq!(missing.learning_rules, hux_core::learning::hash("\0\0"));
}

fn segment_with_candidate(candidate: Candidate) -> Segment {
    Segment {
        start: 0,
        end: 2,
        tags: Vec::new(),
        prompt: String::new(),
        selected_index: 0,
        candidates: vec![candidate],
        selected: false,
        translated: true,
    }
}

#[test]
fn confirm_selection_honors_auto_commit() {
    let mut context = Context::new();
    context.set_input(b"ab");
    context
        .composition
        .segments
        .push(segment_with_candidate(Candidate::new(
            "sentence", 0, 2, "甲", "",
        )));
    // `_auto_commit` 关闭：只标记选中，不提交（对应 librime 的 Forward 分支）
    confirm_selection(None, &mut context, &mut SentenceState::fresh(1));
    assert!(context.composition.back().unwrap().selected);
    assert_eq!(context.input(), b"ab");
    // 打开后：确认即提交
    context.set_option("_auto_commit", true);
    confirm_selection(None, &mut context, &mut SentenceState::fresh(1));
    assert_eq!(context.last_commit_text(), "甲");
    assert!(context.input().is_empty());
}

#[test]
fn confirm_selection_merges_buffered_prefix() {
    let mut context = Context::new();
    context.set_option("_auto_commit", true);
    set_property_if_changed(&mut context, K_BUFFERED, "乙");
    context.set_input(b"~c");
    context
        .composition
        .segments
        .push(segment_with_candidate(Candidate::new(
            "sentence_buffered",
            0,
            2,
            "c",
            "",
        )));
    // 缓冲候选：提交前并入缓冲前缀
    confirm_selection(None, &mut context, &mut SentenceState::fresh(1));
    assert_eq!(context.last_commit_text(), "乙c");
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
fn processor_guards_menu_navigation_while_buffered() {
    let mut h = Harness::new();
    // 缓冲空闲：菜单导航键拦给宿主
    h.state.buffered_text = "交".to_string();
    assert_eq!(h.press("Tab"), ProcessorResult::Consume);
    assert_eq!(h.press("Up"), ProcessorResult::Consume);
}

#[test]
fn processor_forwards_navigation_without_menu() {
    let mut h = Harness::new();
    // 无缓冲：Up 交宿主；Tab 无菜单可用时同样交宿主
    assert_eq!(h.press("Up"), ProcessorResult::Forward);
    assert_eq!(h.press("Tab"), ProcessorResult::Forward);
}

#[test]
fn processor_space_confirms_candidate() {
    let mut h = Harness::new();
    h.push_segment(b"ab", &["交"]);
    assert_eq!(h.press("space"), ProcessorResult::Consume);
    assert!(h.state.committed_raw.is_empty());
}

/// 数字直选（`DigitSelect`）：菜单可见时按页位置直接上屏（1–9；0=10）。
#[test]
fn processor_digit_select_commits_page_candidate() {
    let mut h = Harness::new();
    h.context.set_option(OPTION_DIGIT_SELECT, true);
    h.context.set_option("_auto_commit", true);
    h.push_segment(b"ab", &["交", "疒"]);
    assert!(h.context.has_menu());
    assert_eq!(h.press("2"), ProcessorResult::Consume);
    assert_eq!(h.context.last_commit_text(), "疒");
    assert!(h.context.input().is_empty());
}

/// 数字直选默认关：数字仍作为编码字符（选重后缀）。
#[test]
fn processor_digit_select_off_keeps_rank_suffix() {
    let mut h = Harness::new();
    h.context.set_option("_auto_commit", true);
    h.push_segment(b"ab", &["交", "疒"]);
    assert_eq!(h.press("2"), ProcessorResult::Consume);
    assert_eq!(h.context.last_commit_text(), "");
    assert_eq!(h.context.input(), b"ab2");
}

/// 数字直选：页内没有该位置时不消费（交回普通数字处理）。
#[test]
fn processor_digit_select_out_of_page_falls_through() {
    let mut h = Harness::new();
    h.context.set_option(OPTION_DIGIT_SELECT, true);
    h.context.set_option("_auto_commit", true);
    h.push_segment(b"ab", &["交", "疒"]);
    // 页大小 5：`0`（第 10 个）不在页内 → 作为编码后缀进入输入。
    assert_eq!(h.press("0"), ProcessorResult::Consume);
    assert_eq!(h.context.last_commit_text(), "");
    assert_eq!(h.context.input(), b"ab0");
}

/// 数字直选：页大小 10 时 `0` 上屏当前页第 10 个候选。
#[test]
fn processor_digit_select_zero_picks_tenth_on_ten_page() {
    let mut h = Harness::new();
    h.context.set_option(OPTION_DIGIT_SELECT, true);
    h.page_size = 10;
    h.context.set_option("_auto_commit", true);
    let texts: Vec<String> = (0..12).map(|index| format!("候{index}")).collect();
    let refs: Vec<&str> = texts.iter().map(String::as_str).collect();
    h.push_segment(b"ab", &refs);
    assert_eq!(h.press("0"), ProcessorResult::Consume);
    assert_eq!(h.context.last_commit_text(), "候9");
}

/// 多项触发键（`KeyList`）：任一配置键都可进入音反查，入段字符取命中键的字符。
#[test]
fn processor_sound_to_char_shape_accepts_multiple_triggers() {
    let mut h = Harness::new();
    h.context
        .set_property(K_SOUND_TO_CHAR_SHAPE_KEY, "grave,semicolon");
    assert_eq!(h.press("semicolon"), ProcessorResult::Consume);
    assert_eq!(h.context.input(), b";");
    h.context.clear();
    assert_eq!(h.press("grave"), ProcessorResult::Consume);
    assert_eq!(h.context.input(), b"`");
}

#[test]
fn processor_backspace_pops_locked_input() {
    let mut h = Harness::new();
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

#[test]
fn backspace_with_inconsistent_committed_text_does_not_panic() {
    let mut h = Harness::new();
    // 组合存在（缓冲退格分支的前提）且 live input 为空。
    h.push_segment(b"", &[]);
    // 属性可能来自旧版本/外部：committed_text 尾字符与 buffered 尾字符不一致时，
    // 退格只按字符边界截断，不得 panic。
    h.state.buffered_text = "A".to_string();
    h.state.committed_text = "甲".to_string();
    h.state.committed_raw = "a".to_string();
    assert_eq!(h.press("BackSpace"), ProcessorResult::Consume);
}
