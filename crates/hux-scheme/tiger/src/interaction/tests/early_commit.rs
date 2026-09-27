// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! 提前提交证据链（`interaction/early_commit.rs`）的用例。

use super::*;

#[test]
fn min_retained_raw_length_clamps() {
    assert_eq!(min_retained_raw_length(Some(3)), 3);
    assert_eq!(min_retained_raw_length(Some(-1)), 0);
    assert_eq!(min_retained_raw_length(None), 0);
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
    let dir = hux_test_support::repo_path("goldens/lexicon");
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
    // 三元全等：参照在此对两者都返回 false ⇒ 胜者取决于哈希迭代序；
    // 本仓按 text 字典序兜底 ⇒ 判据自身反对称、与迭代序无关。
    let mut tied_earlier = base.clone();
    tied_earlier.text = "乙".to_string(); // U+4E59 < 甲 U+7532
    let mut tied_later = base.clone();
    tied_later.text = "甲".to_string();
    assert!(tracker_better(&tied_earlier, &tied_later));
    assert!(!tracker_better(&tied_later, &tied_earlier));
    assert!(!tracker_better(&tied_earlier, &tied_earlier));
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
    let mut trackers = Map::new();
    trackers.insert("keep".to_string(), tracker("甲丙", 0.2));
    trackers.insert("gone".to_string(), tracker("不存在", 0.2));
    let retained = retain_trackers_without_counting(&trackers, &evidence, false);
    assert!(retained.is_empty());
    // gap_count 超过上限（3）应丢弃
    let mut stable = tracker("甲乙", 0.3);
    stable.gap_count = 3;
    let mut map = Map::new();
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
    let mut map = Map::new();
    map.insert("mature".to_string(), mature());
    let kept = retain_trackers_without_counting(&map, &stale, true);
    let kept = kept.get("mature").expect("低置信度缺口仍保留身份");
    assert_eq!(kept.gap_count, 1);
    assert_eq!(kept.evidence_count, 0, "低置信度缺口不得带走成熟度");
    assert_eq!(kept.strong_count, 0, "强证据计数同样清零");
    assert_eq!(kept.last_share, 0.99);

    let mut map = Map::new();
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

/// 参照上游 `test_tiger_sentence_incremental.lua` 新增断言：
/// 排名先验（词先验/学习重排）不得授权与显示首选不一致的自动提交前缀。
#[test]
fn auto_commit_matches_visible_top_guard() {
    assert!(!auto_commit_matches_visible_top(Some("鼎丁"), "甲乙"));
    assert!(auto_commit_matches_visible_top(Some("鼎丁"), "鼎"));
    assert!(auto_commit_matches_visible_top(None, "甲乙"));
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
