// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;
use hux_core::host::HostOptions;
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

/// 参照 `d30867a`：竞争切分前瞻的三条向量取自上游
/// `tools/test_tiger_sentence_incremental.lua`（真实码表：`nv`=有、`nvt`=郁、`tah`=衅、`ahx`=闷）。
/// 用入库码表夹具（`goldens/lexicon`）加载，等价于参照的 `lexicon_state.codes`。
#[test]
fn competing_boundary_end_aligns_competing_paths_by_text_elements() {
    let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../goldens/lexicon");
    // `high_freq_limit` 只影响码表条目过滤，不改变 `codes` 键；用 0 保留全部条目。
    let lexicon = Lexicon::load(std::slice::from_ref(&dir), 0);
    let texts = |code: &str| {
        lexicon.codes.get(code).map(|entries| {
            entries
                .iter()
                .map(|entry| entry.text.clone())
                .collect::<Vec<_>>()
        })
    };
    assert_eq!(texts("nv"), Some(vec!["有".to_string()]));
    assert_eq!(texts("nvt"), Some(vec!["郁".to_string()]));
    assert_eq!(texts("tah"), Some(vec!["衅".to_string()]));
    assert_eq!(texts("ahx"), Some(vec!["闷".to_string()]));
    // `nv` 提交「有」（1 字素）时必须等到 `nvt`（1 字素）⇒ 边界前移到 7。
    assert_eq!(
        competing_boundary_end(b"jreynvtah", &lexicon, 4, 6, 1),
        7,
        "aligned retention missed nv | nvt competition"
    );
    // `nv|tah` 已输出两个字素，不得拖延一字提交。
    assert_eq!(
        competing_boundary_end(b"jreynvtahx", &lexicon, 4, 7, 1),
        7,
        "second output element incorrectly delayed a one-element boundary"
    );
    // 两字素保留：`nv|tah` 对 `nvt|ahx` ⇒ 边界前移到 10。
    assert_eq!(
        competing_boundary_end(b"jreynvtahx", &lexicon, 4, 9, 2),
        10,
        "two-element retention missed nv|tah versus nvt|ahx"
    );
    // 参数非法一律原样返回（参照的三条前置守卫；不触碰码表）。
    assert_eq!(competing_boundary_end(b"jreynvtah", &lexicon, 4, 4, 1), 4);
    assert_eq!(competing_boundary_end(b"jreynvtah", &lexicon, 4, 20, 1), 20);
    assert_eq!(competing_boundary_end(b"jreynvtah", &lexicon, 4, 6, 0), 6);
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
        base_share: share,
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
    let retained = retain_trackers_without_counting(&trackers, &evidence, false);
    assert!(retained.is_empty());
    // gap_count 超过上限（3）应丢弃
    let mut stable = tracker("甲乙", 0.3);
    stable.gap_count = 3;
    let mut map = HashMap::new();
    map.insert("stable".to_string(), stable);
    let retained = retain_trackers_without_counting(&map, &evidence, false);
    assert!(retained.is_empty(), "gap_count 超过上限应丢弃");
}

/// 参照 `d30867a` + 上游增量测试：低置信度（`neutral_low_confidence`）的比较型缺口
/// 只保住 tracker 的**身份**，把成熟度（`evidence_count`/`strong_count`）清零；
/// 普通比较型缺口保留成熟度（上游 `a low-confidence gap preserved stale maturity` 的反例）。
#[test]
fn retain_trackers_resets_maturity_only_for_low_confidence_gaps() {
    use crate::decode::{Evidence, PrefixEvidence};
    // 上游 `stale_prefixes`：单条同名前缀（不与 tracker 矛盾），份额 0.99。
    let stale = Evidence {
        prefixes: vec![PrefixEvidence {
            text: "甲乙".to_string(),
            raw_length: 2,
            share: 0.99,
            base_share: 0.99,
            boundary_share: 0.99,
            boundary_closed: false,
            text_char_count: 2,
        }],
        by_boundary: [(2usize, [("甲乙".to_string(), 0)].into_iter().collect())]
            .into_iter()
            .collect(),
        proposal: String::new(),
        proposal_share: 0.0,
        raw_lengths: Default::default(),
        neutral_incomplete_tail: false,
        merged_incomplete_tail: false,
        neutral_low_confidence: true,
        confidence_truncated: false,
    };
    let mature = || {
        let mut tracker = tracker("甲乙", 0.99);
        tracker.raw_length = 2;
        tracker.evidence_count = 3;
        tracker.strong_count = 2;
        tracker.gap_count = 0;
        tracker
    };
    let mut map = HashMap::new();
    map.insert("mature".to_string(), mature());
    let kept = retain_trackers_without_counting(&map, &stale, true);
    let kept = kept.get("mature").expect("低置信度缺口仍保留身份");
    assert_eq!(kept.gap_count, 1);
    assert_eq!(kept.evidence_count, 0, "低置信度缺口不得带走成熟度");
    assert_eq!(kept.strong_count, 0, "强证据计数同样清零");
    assert_eq!(kept.last_share, 0.99);

    let mut map = HashMap::new();
    map.insert("mature".to_string(), mature());
    let kept = retain_trackers_without_counting(&map, &stale, false);
    let kept = kept.get("mature").expect("普通比较型缺口保留");
    assert_eq!(kept.evidence_count, 3, "普通比较型缺口保留成熟度");
    assert_eq!(kept.strong_count, 2);
}

fn evaluated_candidate() -> Evaluated {
    Evaluated {
        text: "甲乙".to_string(),
        score: 0.0,
        confidence_score: 0.0,
        early_commit_confidence_score: 0.0,
        code_score: 0.0,
        lexical_score: 0.0,
        max_rank: 2,
        supplement_score: 0.0,
        learning_score: 0.0,
        edge_count: 1,
        // 未标记来源（判定上既非 Direct 也非 Composed-only）。
        source_mask: 0,
        direct_rank: f64::INFINITY,
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
        // 参照测试的 composed 场景：`source_mask = 2`（composed-only）。
        source_mask: 2,
        fusion_ahead: Vec::new(),
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
    let mode = "sentence-v2|rules=|optimal=1500|dup=1";
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

/// 参照 `7b220ce`：删除「稳定确认」增量路线（`learning.reinforce`）——未按 Tab、
/// 提交项即本菜单首个可见候选时，`learning_stage` 仍走 `diff`，而
/// `before.text == selected.text` 使 `diff` 恒为空 ⇒ **不再产出学习事件**
/// （上游 `tools/test_sentence_learning.lua`：`ordinary learned first choice never reinforces`）。
#[test]
fn learning_stage_does_not_reinforce_stable_first_choice() {
    let mode = "m";
    let selected = selected_item("交疒");
    let state = learning_state();
    let mut live = learning_live(mode);
    learning_stage(
        &mut live,
        &state,
        Some(&selected),
        b"abab",
        Some(&selected),
        100.0,
    );
    assert!(
        live.pending.is_empty(),
        "首选重复确认不再是纠错事件（等级只由人工纠错推进）"
    );

    // 提交项与首选不同 → 仍走 diff（人工纠错）。
    let mut live = learning_live(mode);
    let baseline = selected_item("交交");
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
    assert_eq!(live.pending[0].raw_start, 2);
}

#[test]
fn learning_submit_accepts_matching_and_drops_mismatch() {
    let mode = "sentence-v2|rules=|optimal=1500|dup=1";
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
    let mode = "sentence-v2|rules=|optimal=1500|dup=1";
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
    // stage：兜底项没有来源标记（mask 0），被 composed 门挡下 ⇒ 不产出事件。
    // 注意 `c69c1a8` 之后门在 `learning.diff` 之前短路，参照函数末尾的
    // `live.baseline = nil` 因此照常执行（旧版是 diff 报错被 pcall 吞掉，
    // 于是这一句没跑到、baseline 被保留）。
    learning_stage(&mut live, &state, Some(&fallback), b"ab", None, 100.0);
    assert!(live.pending.is_empty());
    assert!(live.baseline.is_none(), "参照末尾无条件清空 baseline");
    // submit：参照对兜底项无特判；pending 为空 ⇒ 无接受事件，且同样无条件清空 baseline。
    let accepted = learning_submit(&mut live, Some(&fallback), "交", "交");
    assert!(accepted.is_empty());
    assert!(live.baseline.is_none());
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
        let host_options = HostOptions::default();
        let mut env = ProcessorEnv {
            now: 0.0,
            dot_armed: &mut self.dot_armed,
            min_retained: None,
            page_size: self.page_size,
            host_options: &host_options,
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

/// 参照 `d30867a` 的回归场景（上游 `jreynvtahx` 用例）：`nv` 提交「有」时，
/// 必须把保留量算到竞争切分 `nvt`（郁）的边界上，而不是 tracker 自己的 raw 边界。
///
/// 金样覆盖不到该路径（`key_sequence`/`sound_to_char_shape` 的合成夹具里竞争边界
/// 恒等于 tracker 边界），故这里直接驱动 `try_commit_mature_prefix` 的调用点：
/// 同一 tracker 在 `jreynvtah`（竞争边界 7 ⇒ 只剩 2 键前瞻）下不得上屏，
/// 在 `jreynvtahx`（再多 1 键 ⇒ 满足 retain=3）下才上屏。
#[test]
fn mature_prefix_waits_for_the_competing_boundary() {
    let separator = '\u{1f}';
    let mk_state = || {
        let mut state = SentenceState::fresh(1);
        state.committed_raw = "jrey".to_string();
        state.trackers.insert(
            format!("有{separator}6"),
            Tracker {
                text: "有".to_string(),
                text_char_count: 1,
                raw_length: 6,
                evidence_count: 3,
                strong_count: 0,
                gap_count: 0,
                last_share: 0.995,
            },
        );
        state
    };
    // 竞争边界 `nv|tvt`：`jreynvtah` 只给到 7，9-7=2 < retain(3) ⇒ 不上屏。
    let mut state = mk_state();
    let mut context = Context::new();
    let mut dot_armed = false;
    let mut decoder = lexicon_fixture();
    let mut live = LiveLearning::default();
    let mut learning = LearningCommit {
        decoder: &mut decoder,
        live: &mut live,
        now: 0.0,
    };
    assert!(!try_commit_mature_prefix(
        &mut learning,
        &mut context,
        &mut state,
        b"jreynvtah",
        0,
        None,
        &mut dot_armed,
    ));
    assert_eq!(state.committed_text, "", "竞争边界未满足前不得提前上屏");
    // 再多 1 键：10-7=3 ≥ retain ⇒ 上屏「有」并停在竞争边界 `jreynvt`。
    let mut state = mk_state();
    let mut context = Context::new();
    let mut decoder = lexicon_fixture();
    let mut live = LiveLearning::default();
    let mut learning = LearningCommit {
        decoder: &mut decoder,
        live: &mut live,
        now: 0.0,
    };
    assert!(try_commit_mature_prefix(
        &mut learning,
        &mut context,
        &mut state,
        b"jreynvtahx",
        0,
        None,
        &mut dot_armed,
    ));
    assert_eq!(state.committed_text, "有");
    // 上屏边界仍是 tracker 自己的 raw 边界（竞争边界只作「是否够前瞻」的闸门）。
    assert_eq!(state.committed_raw, "jreynv");
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

/// 回归（既有缺陷）：Tab 锁确认路径必须**在清 `state.tab_pending` 之前** stage 学习事件。
///
/// 参照 `processor` 的 Tab 确认分支顺序是
/// `learning_stage(env, state, selected, full_before)` → `state.tab_pending = false`；
/// `learning_stage` 用该标志选基线（`tab_pending and live.baseline or submitted_first`），
/// 且稳定确认（`reinforce`）路线要求 `!tab_pending`。本仓曾只用分支末尾的提交点补 stage，
/// 于是基线取成 `submitted_first` 并可能误走 reinforce——金样覆盖不到（探针
/// `store_ready == false` 使该路径短路），故这里经 `processor` 走端到端。
#[test]
fn processor_tab_confirm_stages_against_live_baseline() {
    let mode = "sentence-v2|rules=|optimal=1500|dup=1";
    let mut h = Harness::new();
    h.live.mode = mode.to_string();
    h.live.store_ready = true;
    for repr in ["a", "b", "a", "b"] {
        assert_eq!(h.press(repr), ProcessorResult::Consume);
    }
    assert_eq!(h.context.input(), b"abab");
    // 真实会话里菜单由 translator 建立（交交 / 交疒 = 两条 2 码边，均为 composed-only）。
    h.push_segment(b"abab", &["交交", "交疒"]);
    // Tab：写基线（本菜单首个可见候选）并把高亮移到下一项
    assert_eq!(h.press("Tab"), ProcessorResult::Consume);
    assert!(h.state.tab_pending);
    let baseline = h.live.baseline.clone().expect("Tab 按下时应写入基线");
    assert_eq!(baseline.text, "交交");
    // 高亮已在第 2 项：此刻按同一解码取到的 selected 就是确认分支将选中的候选。
    let selection = learning_selection(&mut h.decoder, &h.context, &h.state).expect("selection");
    let selected = selection.selected.clone().expect("第 2 个候选");
    assert_eq!(selected.text, "交疒");
    let expected = learning::diff(
        b"abab",
        Some(&baseline.diff),
        Some(&selected.diff),
        0,
        mode,
        0.0,
    );
    assert_eq!(expected.len(), 1, "夹具前提：交交 / 交疒 仅末段不同");
    // 字母确认走 Tab 确认分支（候选 raw 长度超过已确认前缀）
    assert_eq!(h.press("c"), ProcessorResult::Consume);
    assert!(!h.state.tab_pending);
    assert!(
        h.live.baseline.is_none(),
        "stage 必须已按 tab_pending 分支消费基线"
    );
    // 早提交选项关闭 ⇒ 不触发提交点通知器：pending 只能来自 Tab 分支里的 stage。
    assert_eq!(h.live.pending.len(), expected.len());
    for (got, want) in h.live.pending.iter().zip(expected.iter()) {
        assert_eq!(got, want);
    }
    // 融合事件不参与：DiffEvent 的模式仍是实时模式，而非 `fusion-v1|…`。
    assert_eq!(h.live.pending[0].mode, mode);
}

/// 参照 `abad411`：**菜单可见**（不要求缓冲态）时遇可打印 ASCII 标点，必须先按当前选中项
/// stage 学习、确认组合（`_auto_commit` 下即上屏），再把原键交标点表。
///
/// 参照依据（提交信息）：标点段一旦被 punctuator 追加进组合，`learning_selection` 就再也
/// 解不出该输入（如 `zhhbi,`）或取不回句子的选中项；故判据由 `state.buffered_text ~= ""`
/// 改为 `context:has_menu()`。金样覆盖不到该学习路径（探针 `store_ready == false` 短路），
/// 故这里经 `processor` 端到端钉住：本用例的 `buffered_text` 为空，旧判据**不**成立。
#[test]
fn processor_menu_punctuation_stages_learning_before_the_punctuator() {
    let mode = "sentence-v2|rules=|optimal=1500|dup=1";
    let mut h = Harness::new();
    h.context.set_option("_auto_commit", true);
    h.live.mode = mode.to_string();
    h.live.store_ready = true;
    for repr in ["a", "b", "a", "b"] {
        assert_eq!(h.press(repr), ProcessorResult::Consume);
    }
    // 真实会话里菜单由 translator 建立（交交 / 交疒 = 两条 2 码边，均为 composed-only）。
    h.push_segment(b"abab", &["交交", "交疒"]);
    assert!(h.state.buffered_text.is_empty(), "旧判据在此不成立");
    h.context.highlight(1); // 人工纠错：选中第二项
    let selection = learning_selection(&mut h.decoder, &h.context, &h.state).expect("selection");
    let selected = selection.selected.clone().expect("第 2 个候选");
    let first = selection.first.clone().expect("首个候选");
    let expected = learning::diff(
        b"abab",
        Some(&first.diff),
        Some(&selected.diff),
        0,
        mode,
        0.0,
    );
    assert_eq!(expected.len(), 1, "夹具前提：交交 / 交疒 仅末段不同");
    // 逗号：确认组合（上屏「交疒」）后原键仍交标点表。
    assert_eq!(h.press("comma"), ProcessorResult::Forward);
    assert!(h.context.input().is_empty());
    assert_eq!(h.context.last_commit_text(), "交疒");
    assert!(h.live.pending.is_empty(), "提交点已消费 pending");
    assert_eq!(
        h.live.submitted.len(),
        expected.len(),
        "标点路径的纠错必须落学习"
    );
    for (got, want) in h.live.submitted.iter().zip(expected.iter()) {
        assert_eq!(got.time, want.time);
        assert_eq!(got.mode, want.mode);
        assert_eq!(got.code, want.code);
        assert_eq!(got.text, want.text);
        assert_eq!(got.context, want.context);
    }
    assert_eq!(h.live.submitted[0].mode, mode);
}

/// **本仓有意偏离上游 `abad411`**：菜单可见时，会被宿主判为翻页的标点键不由标点分支消费。
///
/// 上游对「菜单可见 + 可打印 ASCII 标点」一律「暂存学习 + 确认组合 + 交标点表」，于是
/// `-/=`（以及 schema 绑到翻页的 `[/]`）的 key_binder 绑定被永久遮蔽（最小复现 `j a equal`）。
/// 本仓在分支入口先问宿主同一套判据 `hux_core::host::paging_action`：
/// `=`（`when: has_menu`）与落入 `paging` 标签后的 `-`（`when: paging`）让给宿主翻页，
/// 不确认组合、不暂存学习；未翻页的 `-` 与普通标点（`,`）维持上游行为。
/// 理由、最小复现与金样登记见 `docs/refactor.md` §8「有意偏离上游」。
#[test]
fn processor_menu_paging_keys_bypass_the_punctuation_branch() {
    let mut h = Harness::new();
    h.context.set_option("_auto_commit", true);
    h.live.store_ready = true;
    h.push_segment(b"ab", &["交", "疒"]);
    assert!(h.context.has_menu());
    h.context.highlight(1); // 人工纠错候选：若走上游标点分支会立刻上屏「疒」
    assert_eq!(h.context.last_commit_text(), "");

    // `=`：`when: has_menu` 成立 ⇒ 让给宿主下翻页（不消费、不确认组合、不 stage 学习）。
    assert_eq!(h.press("equal"), ProcessorResult::Forward);
    assert_eq!(h.context.last_commit_text(), "", "`=` 不得确认组合");
    assert_eq!(h.context.input(), b"ab", "`=` 后组合原样保留（交宿主翻页）");
    assert!(h.context.has_menu(), "`=` 后菜单仍在（交宿主翻页）");

    // `-`：未翻页、`when: paging` 不成立 ⇒ 仍走上游标点分支（确认组合后交标点表）。
    assert_eq!(h.press("minus"), ProcessorResult::Forward);
    assert_eq!(
        h.context.last_commit_text(),
        "疒",
        "未翻页的 `-` 仍落上游路径"
    );
    assert!(h.context.input().is_empty());

    // 同一判据的另一半：落入 `paging` 标签（宿主翻页写入）后，`-` 同样让给宿主上翻页。
    h.push_segment(b"ab", &["交", "疒"]);
    h.context.highlight(1);
    h.context
        .composition
        .back_mut()
        .expect("段")
        .tags
        .push("paging".to_string());
    assert_eq!(h.press("minus"), ProcessorResult::Forward);
    assert_eq!(
        h.context.last_commit_text(),
        "疒",
        "翻页后 `-` 不得再确认新组合"
    );
    assert_eq!(h.context.input(), b"ab", "翻页后 `-` 不消费组合");
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
