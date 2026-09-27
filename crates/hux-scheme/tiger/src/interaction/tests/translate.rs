// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! 翻译与分段（`interaction/translate.rs`）的用例。

use super::*;
use crate::lexicon::code_comment;

#[test]
fn ends_with_digit_detects_digit_tail() {
    assert!(ends_with_digit("甲1"));
    assert!(ends_with_digit("甲１"));
    assert!(!ends_with_digit("甲"));
    assert!(!ends_with_digit(""));
}

/// 分段常量口径：`speller/delimiter` 追踪反查分支尖端 `92a0b54`
/// 的 `" '"`——撇号按音节分隔符处理 ⇒ 段内 `'` 之后必须是首字母，数字/`;` 在此断开。
/// 主干 pin `abad411`（`" "`）下 `ab'1` 是**单段**；该差异在金样层登记为本仓偏离
/// （`tests/key_sequence_differential.rs` 的 `DEVIATIONS`：`apostrophe_digit_page` 等）。
#[test]
fn abc_segmentor_splits_after_a_delimiter_before_a_digit() {
    assert_eq!(SEGMENTATION_DELIMITER, " '");
    // `'` + 数字：abc 段止于撇号，数字落 raw 段（末段是 raw ⇒ 宿主导航键不消费）。
    let mut composition = Composition::default();
    calculate_segmentation(&mut composition, b"ab'1", 4, &[], &[]);
    assert_eq!(composition.segments.len(), 2);
    assert_eq!(
        (composition.segments[0].start, composition.segments[0].end),
        (0, 3)
    );
    assert!(composition.segments[1].has_tag("raw"));
    assert_eq!(
        (composition.segments[1].start, composition.segments[1].end),
        (3, 4)
    );
    // `'` + `;`：同理（分号在 alphabet 内但不是首字母）。
    let mut composition = Composition::default();
    calculate_segmentation(&mut composition, b"ab';", 4, &[], &[]);
    assert_eq!(composition.segments.len(), 2);
    assert_eq!(
        (composition.segments[0].start, composition.segments[0].end),
        (0, 3)
    );
    // `'` + 首字母：单段（与上游主干一致 ⇒ 金样 `apostrophe_*_split` 逐位通过）。
    let mut composition = Composition::default();
    calculate_segmentation(&mut composition, b"ab'c", 4, &[], &[]);
    assert_eq!(composition.segments.len(), 1);
    assert_eq!(
        (composition.segments[0].start, composition.segments[0].end),
        (0, 4)
    );
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
fn code_comment_formats() {
    let dir = hux_test_support::repo_path("goldens/lexicon");
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
fn translate_produces_sentence_candidates() {
    let dir = hux_test_support::repo_path("goldens/lexicon");
    let lexicon = Lexicon::load(std::slice::from_ref(&dir), 1500);
    let supplement = crate::lexicon::Supplement::load_default(Some(&dir));
    let mut decoder = Decoder::new(lexicon, supplement, None);
    let context = Context::new();
    let state = SentenceState::fresh(1);
    let mut out = Vec::new();
    translate_composition(&mut decoder, &context, &state, b"ab", 0, 2, &mut out)
        .expect("translate");
    assert!(!out.is_empty());
    assert!(out.iter().all(|candidate| candidate.kind == "sentence"));
    assert!(out.iter().all(|candidate| !candidate.text.is_empty()));
}

#[test]
fn translate_emits_buffered_candidate() {
    let dir = hux_test_support::repo_path("goldens/lexicon");
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
    translate_composition(
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
    let dir = hux_test_support::repo_path("goldens/lexicon");
    let lexicon = Lexicon::load(std::slice::from_ref(&dir), 1500);
    let supplement = crate::lexicon::Supplement::load_default(Some(&dir));
    let mut decoder = Decoder::new(lexicon, supplement, None);
    let context = Context::new();
    let state = SentenceState::fresh(1);
    // 音反查段（` 前缀）由 `sound_to_char_shape` 模块处理，translator 不产出候选。
    let mut out = Vec::new();
    translate_composition(&mut decoder, &context, &state, b"`ni", 0, 3, &mut out)
        .expect("translate");
    assert!(out.is_empty());
}

#[test]
fn translate_requires_buffer_marker() {
    let dir = hux_test_support::repo_path("goldens/lexicon");
    let lexicon = Lexicon::load(std::slice::from_ref(&dir), 1500);
    let supplement = crate::lexicon::Supplement::load_default(Some(&dir));
    let mut decoder = Decoder::new(lexicon, supplement, None);
    let context = Context::new();
    let mut buffered_state = SentenceState::fresh(1);
    buffered_state.buffered_text = "甲".to_string();
    // 缓冲态下非零起点（后续段）不翻译。
    let mut out = Vec::new();
    translate_composition(
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
    translate_composition(
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

#[test]
fn translate_applies_duplicate_single_option() {
    let mut decoder = lexicon_fixture();
    let mut context = Context::new();
    let state = SentenceState::fresh(1);
    context.set_option(OPTION_ALLOW_DUPLICATE_SINGLE, false);
    let mut out = Vec::new();
    translate_composition(&mut decoder, &context, &state, b"abab", 0, 4, &mut out)
        .expect("translate");
    let texts: Vec<String> = out.iter().map(|candidate| candidate.text.clone()).collect();
    assert!(!texts.is_empty());
    assert!(!texts.iter().any(|text| text.contains('疒')), "{texts:?}");
    context.set_option(OPTION_ALLOW_DUPLICATE_SINGLE, true);
    let mut out = Vec::new();
    translate_composition(&mut decoder, &context, &state, b"abab", 0, 4, &mut out)
        .expect("translate");
    let texts: Vec<String> = out.iter().map(|candidate| candidate.text.clone()).collect();
    assert!(texts.iter().any(|text| text.contains('疒')), "{texts:?}");
}
