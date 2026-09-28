// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! 预编辑分码：按词分段的 preedit 文本，以及移动光标时的分段保持。
//!
//! 用共享 UI 快照（`last_update()`）核对；夹具与 `serial()` 串行约定见父模块 `tests.rs`。

use super::*;

/// 预编辑「按词分码」：使用高亮候选的 preedit（`ab cd`）。
#[test]
fn preedit_segments_word_codes() {
    let _guard = serial();
    UPDATES.lock().unwrap().clear();
    let mut engine = TestEngine::new(host(), fixture_dirs(), None, None);
    for code in *b"abcd" {
        engine.key(u32::from(code), 0, false);
    }
    let (preedit, cursor, candidates, _, _, _) = last_update();
    assert!(!candidates.is_empty(), "abcd 应有候选");
    assert_eq!(preedit, "ab cd");
    assert_eq!(cursor, 5);
}

/// 预编辑：单字不分段（`ab`）。
#[test]
fn preedit_single_char_is_unsegmented() {
    let _guard = serial();
    UPDATES.lock().unwrap().clear();
    let mut engine = TestEngine::new(host(), fixture_dirs(), None, None);
    for code in *b"ab" {
        engine.key(u32::from(code), 0, false);
    }
    let (preedit, cursor, _, _, _, _) = last_update();
    assert_eq!(preedit, "ab");
    assert_eq!(cursor, 2);
}

/// 按字分码：左右移动光标时保持分码显示，组合之后的原始尾部接在其后。
#[test]
fn preedit_keeps_segmented_codes_while_moving_caret() {
    let _guard = serial();
    UPDATES.lock().unwrap().clear();
    let mut engine = TestEngine::new(host(), fixture_dirs(), None, None);
    for code in *b"abcdja" {
        engine.key(u32::from(code), 0, false);
    }
    // 末尾：整段按词分码（ab cd ja）。
    let (preedit, cursor, candidates, _, _, _) = last_update();
    assert_eq!(preedit, "ab cd ja");
    assert_eq!(cursor, 8);
    assert!(!candidates.is_empty());
    // ←×2：组合重建为 `abcd`（分码 `ab cd`），光标之后接上原始尾部 `ja`。
    assert!(engine.key(0xff51, 0, false));
    assert!(engine.key(0xff51, 0, false));
    let (preedit, cursor, candidates, _, _, _) = last_update();
    assert_eq!(preedit, "ab cdja");
    assert_eq!(cursor, 5);
    assert!(!candidates.is_empty(), "前缀 `abcd` 应有候选");
    // →×2：回到末尾，恢复整段分码。
    assert!(engine.key(0xff53, 0, false));
    assert!(engine.key(0xff53, 0, false));
    let (preedit, cursor, _, _, _, _) = last_update();
    assert_eq!(preedit, "ab cd ja");
    assert_eq!(cursor, 8);
}
