// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! 修饰键、按键释放与空闲态编辑键一律放行宿主（`HUX_KEY_PASS`）。

use super::*;

#[test]
fn modified_keys_pass_through() {
    let _guard = serial();
    let mut engine = TestEngine::new(host(), fixture_dirs(), None, None);
    assert!(!engine.key(u32::from(b'a'), FCITX_CTRL, false)); // Ctrl+a 交宿主
}

#[test]
fn key_releases_pass_through() {
    let _guard = serial();
    let mut engine = TestEngine::new(host(), fixture_dirs(), None, None);
    assert!(!engine.key(u32::from(b'a'), 0, true));
}

#[test]
fn idle_return_passes_through() {
    let _guard = serial();
    let mut engine = TestEngine::new(host(), fixture_dirs(), None, None);
    assert!(!engine.key(0xff0d, 0, false)); // Return 空闲交宿主
}

#[test]
fn idle_editing_keys_pass_through() {
    let _guard = serial();
    let mut engine = TestEngine::new(host(), fixture_dirs(), None, None);
    // BackSpace/Delete/Left/Right/Up/Down/Home/End/Page_Up/Page_Down/Escape/Tab
    for keysym in [
        0xff08, 0xffff, 0xff51, 0xff53, 0xff52, 0xff54, 0xff50, 0xff57, 0xff55, 0xff56, 0xff1b,
        0xff09,
    ] {
        assert!(
            !engine.key(keysym, 0, false),
            "keysym {keysym:#x} 空闲时应交宿主"
        );
    }
}
