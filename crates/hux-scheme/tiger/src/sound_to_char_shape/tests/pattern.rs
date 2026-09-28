// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! 模式识别与标点注释的用例。

use super::*;

#[test]
fn pattern_matches_reference_recognizer() {
    // 参照 `tiger_sentence.schema.yaml` @92a0b54：`^`[a-z']*$`；
    // 上游 `tools/test_reverse_lookup.lua` 同一批断言。
    assert!(matches_pattern(b"`", '`'));
    assert!(matches_pattern(b"`zhong", '`'));
    assert!(matches_pattern(b"`xi'", '`'));
    assert!(matches_pattern(b"`xi'a", '`'));
    assert!(matches_pattern(b"`xi'an", '`'));
    assert!(matches_pattern(b"`xi'an'", '`'));
    assert!(!matches_pattern(b"`Z", '`'));
    assert!(!matches_pattern(b"a`", '`'));
    assert!(!matches_pattern(b"``", '`'));
    assert!(!matches_pattern(b"`1", '`'));
    assert!(!matches_pattern(b"`ni2", '`'));
    assert!(!matches_pattern(b"`xi'an2", '`'));
    assert!(!matches_pattern(b"`xi a", '`'));
    assert!(!matches_pattern(b"xi'an", '`'));
}

#[test]
fn punct_shape_comments_match_reference() {
    assert_eq!(punct_shape_comment("`"), "〔半角〕");
    assert_eq!(punct_shape_comment("｀"), "〔全角〕");
    assert_eq!(punct_shape_comment(""), "");
    assert_eq!(punct_shape_comment("ab"), "");
}

#[test]
fn fixture_index_reports_counts() {
    let index = fixture_index();
    assert_eq!(index.entry_count(), 27);
    assert_eq!(index.spellings.len(), 22);
}
