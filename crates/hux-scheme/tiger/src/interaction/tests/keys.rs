// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! 按键分类（`interaction/keys.rs`）的用例。

use super::*;

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
