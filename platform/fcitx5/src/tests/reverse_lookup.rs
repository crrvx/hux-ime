// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;

/// 多项触发键（`KeyList`）：两项均可进入音反查。
#[test]
fn reverse_lookup_pronunciation_accepts_multiple_trigger_keys() {
    let _guard = serial();
    UPDATES.lock().unwrap().clear();
    let mut engine = TestEngine::new(host(), fixture_dirs(), None, None);
    engine.apply_settings(Settings {
        reverse_lookup_pronunciation_keys: vec!["grave".to_string(), "semicolon".to_string()],
        ..Default::default()
    });
    // 第二绑定（`;`）触发，入段字符为 `;`。
    assert!(engine.key(0x3b, 0, false), "; 应被消费");
    assert_eq!(engine.session().context.input(), b";");
    engine.reset();
    // 第一绑定（`` ` ``）触发，入段字符为 `` ` ``。
    assert!(engine.key(0x60, 0, false), "` 应被消费");
    assert_eq!(engine.session().context.input(), b"`");
}

/// 音反查端到端：设置 → 前缀识别 → 候选/注释 → 预编辑提示 → 空格上屏。
#[test]
fn reverse_lookup_pronunciation_end_to_end() {
    let _guard = serial();
    COMMITS.lock().unwrap().clear();
    UPDATES.lock().unwrap().clear();
    let dirs = vec![
        hux_test_support::repo_path("goldens/sound_to_char_shape"),
        hux_test_support::repo_path("data"),
    ];
    let mut engine = TestEngine::new(host(), dirs, None, None);
    assert!(engine.key(0x60, 0, false), "音反查触发键（默认 `）应被消费");
    for code in *b"zho" {
        assert!(engine.key(u32::from(code), 0, false), "音反查输入应被消费");
    }
    let (preedit, _, candidates, _, _, _) = last_update();
    assert_eq!(preedit, "`zho〔拼音〕");
    assert_eq!(
        candidates,
        vec!["中哦", "中龘", "中欧", "找哦", "兆欧", "找欧"]
    );
    assert!(engine.key(0x20, 0, false));
    assert_eq!(COMMITS.lock().unwrap().last().unwrap(), "中哦");
    // 音反查预编辑「按音节分码」：全拼音节之间插空格。
    engine.reset();
    assert!(engine.key(0x60, 0, false));
    for code in *b"zhongguo" {
        assert!(engine.key(u32::from(code), 0, false));
    }
    let (preedit, _, candidates, _, _, _) = last_update();
    assert_eq!(candidates.first().map(String::as_str), Some("中国"));
    assert_eq!(preedit, "`zhong guo〔拼音〕");
    // 音节分隔符在输入过程中**直接可见**：`zh' 当场显示 `zh'（不必等音节切分完成）。
    engine.reset();
    assert!(engine.key(0x60, 0, false));
    for code in *b"zh'" {
        assert!(engine.key(u32::from(code), 0, false));
    }
    let (preedit, _, candidates, _, _, _) = last_update();
    assert_eq!(preedit, "`zh'〔拼音〕");
    assert!(!candidates.is_empty());
    // 连续撇号只保留第一个：多余的丢弃、不录入（输入串与预编辑都不出现 `''`）。
    assert!(engine.key(u32::from(b'\''), 0, false));
    let (preedit, _, _, _, _, _) = last_update();
    assert_eq!(preedit, "`zh'〔拼音〕");
    assert_eq!(engine.session().context.input(), &b"`zh'"[..]);
    // 分隔符把音节切开：`zh'guo 的撇号原样保留（对照全拼 `zhong guo 由音节边界插空格）。
    for code in *b"guo" {
        assert!(engine.key(u32::from(code), 0, false));
    }
    let (preedit, _, candidates, _, _, _) = last_update();
    assert_eq!(preedit, "`zh'guo〔拼音〕");
    assert!(candidates.iter().any(|c| c == "中国"), "{candidates:?}");
    // 尚无候选（空码）时也当场可见：回退预编辑直接显示原始输入。
    engine.reset();
    assert!(engine.key(0x60, 0, false));
    assert!(engine.key(u32::from(b'\''), 0, false));
    let (preedit, _, candidates, _, _, _) = last_update();
    assert_eq!(preedit, "`'〔拼音〕");
    assert!(candidates.is_empty(), "{candidates:?}");
}

/// 字反查：默认 `~` 进入组合（**单字符触发键 ⇒ 给默认可上屏候选**）；
/// 上排 = 光标左侧 1 字拼音、下排 = 虎码，步长 1；改成带修饰的触发键时不给默认候选。
#[test]
fn reverse_lookup_character_end_to_end() {
    let _guard = serial();
    COMMITS.lock().unwrap().clear();
    UPDATES.lock().unwrap().clear();
    let dirs = vec![
        hux_test_support::repo_path("goldens/sound_to_char_shape"),
        hux_test_support::repo_path("data"),
    ];
    let mut engine = TestEngine::new(host(), dirs, None, None);
    // 应用侧周边文本「中欧中兴」，光标在第 2 个字符后（锚点 = 2）。
    engine.set_surrounding(Some("中欧中兴"), 2);
    // 默认 ~（无修饰单字符）→ 有默认可上屏候选（触发字符本身）；上排「咅」、下排「虍」。
    assert!(engine.key(0x7e, 0, false), "~ 应被消费");
    let (preedit, _, candidates, _, up, down) = last_update();
    assert_eq!(engine.session().context.input(), b"~");
    assert!(preedit.is_empty(), "查码段不下发预编辑：{preedit:?}");
    assert!(
        candidates.iter().any(|candidate| candidate == "~"),
        "单字符触发键应给默认候选：{candidates:?}"
    );
    assert_eq!(up, "咅 ?");
    assert_eq!(down, "虍 nbe/nbeq");
    // ←/→ 交应用（不消费）；周边文本光标随动后，两排在下一次按键刷新。
    assert!(!engine.key(0xff51, 0, false), "Left 应交应用");
    assert!(!engine.key(0xff53, 0, false), "Right 应交应用");
    assert!(!engine.key(0xff52, 0, false), "Up 应交应用");
    assert!(!engine.key(0xff54, 0, false), "Down 应交应用");
    engine.set_surrounding(Some("中欧中兴"), 1);
    assert!(!engine.key(0xffe1, 0, false), "修饰键不消费");
    let (_, _, _, _, up, down) = last_update();
    assert_eq!(up, "咅 zhong");
    assert_eq!(down, "虍 d/dg/dgs");
    // 其它键：退出查码段并照常处理。
    assert!(engine.key(u32::from(b'a'), 0, false), "普通键照常处理");
    assert_eq!(engine.session().context.input(), b"a");
    // 同一契约的另一半：显式把触发键改成带修饰的 Alt+" → **不给**默认候选。
    engine.reset();
    engine.apply_settings(Settings {
        reverse_lookup_character_keys: vec!["Alt+quotedbl".to_string()],
        ..Settings::default()
    });
    assert!(engine.key(0x22, FCITX_ALT, false), "Alt+\" 应被消费");
    let (_, _, candidates, _, _, _) = last_update();
    assert!(
        candidates.is_empty(),
        "带修饰触发键不给默认候选：{candidates:?}"
    );
    // 音反查：默认 `（无修饰单字符）给默认候选；带修饰键（显式 Alt+:）不给。
    engine.reset();
    assert!(engine.key(0x60, 0, false), "默认 ` 应被消费");
    let (_, _, candidates, _, _, _) = last_update();
    assert!(
        candidates.iter().any(|candidate| candidate == "`"),
        "音反查单字符触发键应给默认候选：{candidates:?}"
    );
    engine.reset();
    engine.apply_settings(Settings {
        reverse_lookup_pronunciation_keys: vec!["Alt+colon".to_string()],
        ..Settings::default()
    });
    assert!(engine.key(0x3a, FCITX_ALT, false), "Alt+: 应被消费");
    let (_, _, candidates, _, _, _) = last_update();
    assert!(
        candidates.is_empty(),
        "带修饰触发键不给默认候选：{candidates:?}"
    );
    engine.reset();
    engine.apply_settings(Settings {
        reverse_lookup_pronunciation_keys: vec!["semicolon".to_string()],
        ..Settings::default()
    });
    assert!(engine.key(0x3b, 0, false), "; 应被消费");
    let (_, _, candidates, _, _, _) = last_update();
    assert!(
        // 候选为标点表映射（half_shape 的 ; → ；）。
        candidates.iter().any(|candidate| candidate == "；"),
        "单字符触发键应给默认候选：{candidates:?}"
    );
    // 音反查：单字符触发键（`）→ 同样给默认可上屏候选，空格上屏。
    engine.reset();
    engine.apply_settings(Settings {
        reverse_lookup_pronunciation_keys: vec!["grave".to_string()],
        ..Settings::default()
    });
    assert!(engine.key(0x60, 0, false), "` 应被消费");
    let (_, _, candidates, _, _, _) = last_update();
    assert!(
        candidates.iter().any(|candidate| candidate == "`"),
        "音反查单字符触发键应给默认候选：{candidates:?}"
    );
    assert!(engine.key(0x20, 0, false), "空格确认候选");
    assert_eq!(COMMITS.lock().unwrap().last().unwrap(), "`");
    // 单字符触发键（~）→ 提供默认可上屏候选，空格上屏。
    engine.reset();
    engine.apply_settings(Settings {
        reverse_lookup_character_keys: vec!["asciitilde".to_string()],
        ..Settings::default()
    });
    engine.set_surrounding(Some("中欧中兴"), 2);
    assert!(engine.key(0x7e, 0, false), "~ 应被消费");
    let (_, _, candidates, _, _, _) = last_update();
    assert!(
        candidates.iter().any(|candidate| candidate == "~"),
        "单字符触发键应给默认候选：{candidates:?}"
    );
    assert!(engine.key(0x20, 0, false), "空格确认候选");
    assert_eq!(COMMITS.lock().unwrap().last().unwrap(), "~");
}

/// 字反查：周边文本不可用（如终端）时不显示提示，两排均为空。
#[test]
fn reverse_lookup_character_without_surrounding_shows_nothing() {
    let _guard = serial();
    UPDATES.lock().unwrap().clear();
    let mut engine = TestEngine::new(host(), reverse_lookup_character_dirs(), None, None);
    engine.set_surrounding(None, 0);
    assert!(engine.key(0x7e, 0, false), "~ 应被消费");
    let (_, _, _, _, up, down) = last_update();
    assert!(up.is_empty(), "周边文本不可用时上排应为空：{up:?}");
    assert!(down.is_empty(), "周边文本不可用时下排应为空：{down:?}");
}

/// 字反查：周边文本恢复后，同一查码段在下一次按键刷新出两排。
#[test]
fn reverse_lookup_character_refreshes_when_surrounding_available() {
    let _guard = serial();
    UPDATES.lock().unwrap().clear();
    let mut engine = TestEngine::new(host(), reverse_lookup_character_dirs(), None, None);
    engine.set_surrounding(None, 0);
    assert!(engine.key(0x7e, 0, false), "~ 应被消费");
    engine.set_surrounding(Some("中欧中兴"), 2);
    assert!(!engine.key(0xffe1, 0, false), "修饰键不消费（触发刷新）");
    let (_, _, _, _, up, down) = last_update();
    assert_eq!(up, "咅 ?");
    assert_eq!(down, "虍 nbe/nbeq");
}
