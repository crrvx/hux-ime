// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;

/// 文本含 NUL 时剔除后送出，而不是整条丢空。
#[test]
fn nul_in_text_is_stripped_not_dropped() {
    assert_eq!(
        crate::ui::cstring_lossy("中\0文").to_str().expect("utf8"),
        "中文"
    );
}

/// 选项保存失败须在状态串可见：把 `options.yaml` 造成目录使其必然写失败。
#[test]
fn option_save_error_is_visible_in_status() {
    let _guard = serial();
    let dir = temp_user_dir("options-error");
    std::fs::create_dir_all(dir.join(hux_cfg::OPTIONS_FILE))
        .expect("make options path a directory");
    let mut engine = TestEngine::new(host(), fixture_dirs(), None, Some(dir.clone()));
    assert!(
        !engine
            .engine
            .status
            .to_str()
            .unwrap_or("")
            .contains("options:"),
        "初始状态串不含选项错误"
    );
    assert!(engine.set_option_value("tiger_sentence_early_commit", false));
    let status = engine.engine.status.to_str().unwrap_or("").to_string();
    assert!(
        status.contains("options: Unable to save"),
        "保存失败应在状态串可见：{status}"
    );
    std::fs::remove_dir_all(&dir).ok();
}

/// 配置页绑到**无名字的 keysym**（媒体键）时该绑定会被丢弃 ⇒ 必须点名。
///
/// 反向路径：ABI 把 `keysym + 状态位` 经 `KeyEvent::repr()` 转成键名（`0x1008ff14`），
/// `KeyEvent::from_repr` 不认；方案与 cfg 都在 `filter_map` 处静默丢。此处钉住
/// 「可解析 ⇒ 无诊断；不可解析 ⇒ 状态串点名 ⇒ C++ 壳落日志」。
#[test]
fn unparsable_hotkey_binding_reaches_the_status_string() {
    let _guard = serial();
    let mut engine = TestEngine::new(host(), fixture_dirs(), None, None);
    assert!(
        !engine
            .engine
            .status
            .to_str()
            .unwrap_or("")
            .contains("hotkeys:"),
        "缺省设置不应有热键诊断"
    );
    // 正例：可解析的键名不产生诊断。
    engine.engine.apply_settings(Settings {
        page_up_keys: vec!["Page_Up".to_string(), "bracketleft".to_string()],
        ..Default::default()
    });
    assert!(
        !engine
            .engine
            .status
            .to_str()
            .unwrap_or("")
            .contains("hotkeys:"),
        "可解析的绑定不应有诊断"
    );
    // 负例：X11 `XF86AudioPlay` = 0x1008ff14，rime 键名表里没有名字。
    engine.engine.apply_settings(Settings {
        page_up_keys: vec!["Page_Up".to_string(), "0x1008ff14".to_string()],
        reverse_lookup_character_keys: vec!["(unknown)".to_string()],
        ..Default::default()
    });
    let status = engine.engine.status.to_str().unwrap_or("").to_string();
    assert!(
        status.contains("hotkeys: 忽略无法识别的绑定"),
        "无法识别的绑定必须点名：{status}"
    );
    assert!(
        status.contains("page_up_keys=0x1008ff14"),
        "诊断须带角色与键名：{status}"
    );
    assert!(
        status.contains("char_to_sound_shape_keys=(unknown)"),
        "诊断须覆盖四项绑定：{status}"
    );
    // 改回可解析后诊断清空。
    engine.engine.apply_settings(Settings {
        page_up_keys: vec!["Page_Up".to_string()],
        ..Default::default()
    });
    assert!(
        !engine
            .engine
            .status
            .to_str()
            .unwrap_or("")
            .contains("hotkeys:"),
        "恢复后诊断应清空"
    );
}

/// `hux_engine_status` 的指针契约：状态串在刷新时被**替换**，
/// 契约是「每次调用取最新串，不得缓存指针」——故刷新后必须**重新调用**才能拿到新串。
///
/// 头文件此前写「随引擎存活」，与 `refresh_status` 换 `CString` 的实现不符；
/// 现契约与实现一致，本用例把「重新调用即最新」钉住（旧指针按契约已失效，无法安全断言）。
#[test]
fn status_pointer_must_be_read_again_after_a_refresh() {
    let _guard = serial();
    let dir = temp_user_dir("status-pointer");
    std::fs::create_dir_all(dir.join(hux_cfg::OPTIONS_FILE))
        .expect("make options path a directory");
    let mut engine = Engine::new_with_dirs(host(), fixture_dirs(), None, Some(dir.clone()));
    let read = |engine: &Engine| -> String {
        let pointer = unsafe { hux_engine_status(engine) };
        assert!(!pointer.is_null(), "状态串不应为 NULL");
        unsafe { std::ffi::CStr::from_ptr(pointer) }
            .to_string_lossy()
            .into_owned()
    };
    let before = read(&engine);
    assert!(!before.contains("options:"), "初始无选项错误：{before}");
    let session = engine.session_new();
    assert!(engine.set_option_value("tiger_sentence_early_commit", false));
    engine.key(session, u32::from(b'a'), 0, false);
    let after = read(&engine);
    assert!(
        after.contains("options: Unable to save"),
        "刷新后重新调用必须读到新串：{after}"
    );
    assert_ne!(before, after, "状态串内容应随刷新变化");
    std::fs::remove_dir_all(&dir).ok();
}

/// 配置袋诊断通道：逐角色诊断必须**进状态串**，且真实装配路径无诊断。
///
/// 此前 `Config::parse` 对未知角色 / 类型不符一律 `unwrap_or` 静默回退：单侧改名或把
/// `Count` 塞进开关角色，用户侧只表现为「设置没生效」，状态串里什么都没有。
#[test]
fn scheme_config_diagnostics_reach_the_status_string() {
    let _guard = serial();
    let mut engine = TestEngine::new(host(), fixture_dirs(), None, Some(temp_user_dir("cfgdiag")));
    let status = engine.engine.status.to_str().unwrap_or("").to_string();
    assert!(
        !status.contains("config:"),
        "真实装配路径不应有配置诊断：{status}"
    );

    // 类型不符：把 `Count` 装进开关角色 ⇒ 诊断进状态串（方案仍按缺省回退）。
    let bad = crate::engine::scheme_config(&engine.engine.settings).with(
        hux_cfg::roles::ROLE_LEARNING_ON_TAB,
        hux_core::scheme::Value::Count(1),
    );
    engine.engine.apply_scheme_config(bad);
    let status = engine.engine.status.to_str().unwrap_or("").to_string();
    assert!(
        status.contains("config: 角色 tab_learning 类型不符（期望 开关，实际 计数）"),
        "类型不符必须可见：{status}"
    );

    // 缺角色：漏装 `page_size` ⇒ 同样点名（不静默当作页大小 0）。
    let mut partial = hux_core::scheme::SchemeConfig::new();
    for (role, value) in [
        (
            hux_cfg::roles::ROLE_HIGH_FREQ_LIMIT,
            hux_core::scheme::Value::Count(1500),
        ),
        (
            hux_cfg::roles::ROLE_MIN_RETAINED_INPUT_LENGTH,
            hux_core::scheme::Value::Count(0),
        ),
    ] {
        partial.set(role, value);
    }
    engine.engine.apply_scheme_config(partial);
    let status = engine.engine.status.to_str().unwrap_or("").to_string();
    assert!(
        status.contains("config: 缺少角色 page_size"),
        "漏装角色必须可见：{status}"
    );

    // 重新下发完整配置袋（设置派生的角色 + 运行时开关的生效值）：诊断清空（状态串回到基线）。
    let good = engine.engine.scheme_config_with_runtime();
    engine.engine.apply_scheme_config(good);
    let status = engine.engine.status.to_str().unwrap_or("").to_string();
    assert!(
        !status.contains("config:"),
        "恢复完整配置袋后不应残留诊断：{status}"
    );
}
