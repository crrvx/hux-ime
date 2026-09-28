// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! 确认选择（`interaction/select.rs` 的 `confirm_selection`）与数字直选（页内定位、
//! 越页惰性消费、页大小为 10 时的第 10 槽、以及关闭时的编码后缀路径）。

use super::*;

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
    hux_core::session::set_property_if_changed(&mut context, K_BUFFERED, "乙");
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
///
/// 跨层分工：平台侧 `platform/fcitx5/src/tests.rs` 的
/// `digit_select_off_keeps_rank_suffix` 负责引擎可见结果（按键被消费、候选列表为空、
/// 输入尾部是 `2`）；本用例只补充内核独有的结构：数字进的是**编码路径**，
/// 候选段（候选、高亮、区间、确认位）逐字段不变。
#[test]
fn processor_digit_select_off_keeps_rank_suffix() {
    let mut h = Harness::new();
    h.context.set_option("_auto_commit", true);
    h.push_segment(b"ab", &["交", "疒"]);
    let before = h.context.composition.back().expect("段存在").clone();
    assert_eq!(h.press("2"), ProcessorResult::Consume);
    let after = h.context.composition.back().expect("段仍存在");
    // 编码后缀：进入原始输入并把光标推后。
    assert_eq!(h.context.input(), b"ab2");
    assert_eq!(h.context.caret(), 3);
    assert_eq!(h.context.last_commit_text(), "");
    // 结构不变：digit_select 关掉时不会走 `select_candidate_at` ⇒ 段未被确认、高亮未动。
    assert!(!after.selected, "编码分支不得确认候选段");
    assert_eq!(after.selected_index, before.selected_index);
    assert_eq!(after.translated, before.translated);
    assert_eq!(after.candidates, before.candidates);
    assert_eq!((after.start, after.end), (before.start, before.end));
    assert!(h.state.locks.is_empty() && h.state.committed_raw.is_empty());
}

/// 数字直选：页内没有该位置时不消费（交回普通数字处理）。
///
/// 跨层分工：平台侧 `digit_select_out_of_page_falls_through` 负责引擎可见结果
/// （按键被消费、无上屏、输入尾部是 `0`）；本用例补充内核独有的结构：
/// 越页判定（页大小 5、`0` 的页内位置是第 10 槽）发生在**改动候选段之前**，
/// 整段逐字段保持原样。
#[test]
fn processor_digit_select_out_of_page_falls_through() {
    let mut h = Harness::new();
    h.context.set_option(OPTION_DIGIT_SELECT, true);
    h.context.set_option("_auto_commit", true);
    h.push_segment(b"ab", &["交", "疒"]);
    let before = h.context.composition.back().expect("段存在").clone();
    assert_eq!(h.press("0"), ProcessorResult::Consume);
    assert_eq!(h.context.last_commit_text(), "");
    assert_eq!(h.context.input(), b"ab0");
    let after = h.context.composition.back().expect("段仍存在");
    assert!(!after.selected);
    assert_eq!(after.selected_index, before.selected_index);
    assert_eq!(after.candidates, before.candidates);
    assert_eq!((after.start, after.end), (before.start, before.end));
    assert_eq!(h.context.caret(), 3);
    assert!(h.state.locks.is_empty());
}

/// 数字直选：页大小 10 时 `0` 上屏当前页第 10 个候选。
///
/// 跨层分工：平台侧 `digit_select_commits_page_candidate` 负责引擎可见结果
/// （候选列表长度、`COMMITS` 末项）；本用例补充内核独有的结构：候选数是 12
/// 而页大小是 10，`0` 必须落在**页内第 10 槽**（索引 9）而不是末位候选（索引 11），
/// 且整段确认上屏后组合与句子状态机一并归零。
#[test]
fn processor_digit_select_zero_picks_tenth_on_ten_page() {
    let mut h = Harness::new();
    h.context.set_option(OPTION_DIGIT_SELECT, true);
    h.page_size = 10;
    h.context.set_option("_auto_commit", true);
    let texts: Vec<String> = (0..12).map(|index| format!("候{index}")).collect();
    let refs: Vec<&str> = texts.iter().map(String::as_str).collect();
    h.push_segment(b"ab", &refs);
    let before = h.context.composition.back().expect("段存在").clone();
    assert_eq!(before.candidates.len(), 12);
    let tenth = before.candidates[9].text.clone();
    assert_ne!(tenth, before.candidates[11].text, "第 10 槽不是末位候选");
    assert_eq!(h.press("0"), ProcessorResult::Consume);
    assert_eq!(h.context.last_commit_text(), tenth);
    assert!(h.context.input().is_empty());
    assert_eq!(h.context.caret(), 0);
    assert!(h.context.composition.segments.is_empty());
    assert!(!h.context.is_composing());
    assert!(h.state.locks.is_empty());
    assert!(h.state.committed_raw.is_empty());
    assert!(h.state.committed_text.is_empty());
    assert!(h.state.buffered_text.is_empty());
}
