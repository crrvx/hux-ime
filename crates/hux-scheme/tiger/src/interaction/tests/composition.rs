// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! 合成重建（`CompositionBuilder`）的用例。

use super::*;

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
