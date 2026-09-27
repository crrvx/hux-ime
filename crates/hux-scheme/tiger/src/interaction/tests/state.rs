// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! 句子状态机（`interaction/state.rs`）与输入缓冲派生属性的用例。

use super::*;

#[test]
fn buffered_property_derives_live_input_and_caret() {
    let mut context = Context::new();
    hux_core::session::set_property_if_changed(&mut context, K_BUFFERED, "甲");
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
fn invalidate_removes_affected_locks_only() {
    let mut context = Context::new();
    let mut state = state_with_lock("ab", "甲");
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
fn model_generation_change_resets_transients() {
    let mut state = state_with_lock("ab", "甲");
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
fn reset_empties_committed_and_locks() {
    let mut context = Context::new();
    let mut state = state_with_lock("ab", "甲");
    state.buffered_text = "甲".to_string();
    state.save(&mut context);
    assert_eq!(buffered_text(&context), "甲");
    state.reset(&mut context, true);
    assert!(state.committed_raw.is_empty());
    assert!(state.locks.is_empty());
    assert!(state.continuation_after_auto_commit);
    // 属性层只剩缓冲前缀：重置后同步清空（不再有 committed/locks 快照）。
    assert_eq!(buffered_text(&context), "");
    assert!(state.buffered_text.is_empty());
}
