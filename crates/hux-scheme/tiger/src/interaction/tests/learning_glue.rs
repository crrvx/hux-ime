// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! 学习事件编排（`interaction/learning_glue.rs`）的用例。

use super::*;

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
/// `before.text == selected.text` 使 `diff` 恒为空 ⇒ **不产出学习事件**
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

// ------------------------------------------------- 融合事件（产出侧 + 接受侧）

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
fn lexicon_learning_rules_matches_oracle() {
    let dir = hux_test_support::repo_path("goldens/lexicon");
    let lexicon = Lexicon::load(std::slice::from_ref(&dir), 1500);
    // oracle：参照 `learning.hash(codes.."\0"..ranks.."\0"..whitelist)`（pin 版 Lua 直算）。
    assert_eq!(lexicon.learning_rules, "99f336c6e74e055e");
    // 数据缺失时三份内容均为空串。
    let missing = Lexicon::load(&[], 1500);
    assert_eq!(missing.learning_rules, hux_core::learning::hash("\0\0"));
}
