// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;

/// 数字直选（`DigitSelect`）：菜单可见时 1–9 直接上屏当前页候选，0=第 10 个。
#[test]
fn digit_select_commits_page_candidate() {
    let _guard = serial();
    COMMITS.lock().unwrap().clear();
    UPDATES.lock().unwrap().clear();
    let mut engine = TestEngine::new(host(), fixture_dirs(), None, None);
    engine.apply_settings(Settings {
        digit_select: true,
        page_size: 10,
        ..Default::default()
    });
    for code in *b"ja" {
        engine.key(u32::from(code), 0, false);
    }
    let (_, _, candidates, _, _, _) = last_update();
    assert!(candidates.len() >= 10, "夹具 ja 应有至少 10 个候选");
    let tenth = candidates[9].clone();
    assert!(engine.key(u32::from(b'0'), 0, false), "0 应被消费");
    assert_eq!(COMMITS.lock().unwrap().last().unwrap(), &tenth);
}

/// 数字直选关闭时：数字仍是编码字符（选重后缀），不直接上屏。
#[test]
fn digit_select_off_keeps_rank_suffix() {
    let _guard = serial();
    COMMITS.lock().unwrap().clear();
    let mut engine = TestEngine::new(host(), fixture_dirs(), None, None);
    engine.apply_settings(Settings {
        digit_select: false,
        ..Default::default()
    });
    for code in *b"ja" {
        engine.key(u32::from(code), 0, false);
    }
    assert!(engine.key(u32::from(b'2'), 0, false));
    assert!(
        COMMITS.lock().unwrap().is_empty(),
        "关闭时：数字不应直接上屏"
    );
    assert!(engine.session().context.input().ends_with(b"2"));
}

/// 数字直选：页大小 5 时 `0`（第 10 个）不在页内，按普通数字输入处理。
#[test]
fn digit_select_out_of_page_falls_through() {
    let _guard = serial();
    COMMITS.lock().unwrap().clear();
    let mut engine = TestEngine::new(host(), fixture_dirs(), None, None);
    engine.apply_settings(Settings {
        digit_select: true,
        ..Default::default()
    });
    for code in *b"ja" {
        engine.key(u32::from(code), 0, false);
    }
    assert!(engine.key(u32::from(b'0'), 0, false));
    assert!(COMMITS.lock().unwrap().is_empty(), "页外数字不应直接上屏");
    assert!(engine.session().context.input().ends_with(b"0"));
}
