// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;

#[test]
fn ffi_apply_settings_maps_engine_options() {
    let _guard = serial();
    let dir = temp_user_dir("ffi-settings");
    // 独立目录（无 options.yaml）：设置缺省即生效值，不受本机用户配置影响。
    let mut engine = Engine::new_with_dirs(host(), fixture_dirs(), None, Some(dir.clone()));
    let options = ffi_options();
    assert_eq!(
        unsafe { hux_engine_apply_settings(&mut engine, &options) },
        1
    );
    assert!(!engine.settings.early_commit);
    let session_id = engine.session_new();
    let session = engine.sessions.get(&session_id).expect("session");
    assert!(session.context.get_option("full_shape"));
    assert!(session.context.get_option("ascii_punct"));
    assert!(
        engine.scheme.learning_mode().is_empty(),
        "learning_on_tab=false → 学习 mode 为空"
    );
    assert_eq!(engine.settings.high_freq_limit, 800);
    std::fs::remove_dir_all(&dir).ok();
}

/// FFI：新增四项设置映射（候选排列 / 预编辑 / 翻页循环 / 最短保留码数）。
#[test]
fn ffi_apply_settings_maps_new_options() {
    let _guard = serial();
    let dir = temp_user_dir("ffi-new-options");
    let mut engine = Engine::new_with_dirs(host(), fixture_dirs(), None, Some(dir.clone()));
    let options = HuxOptions {
        candidate_layout: 2,
        preedit_mode: 2,
        page_cycle: 1,
        min_retained_input_length: 4,
        ..ffi_options()
    };
    assert_eq!(
        unsafe { hux_engine_apply_settings(&mut engine, &options) },
        1
    );
    assert_eq!(engine.settings.candidate_layout, CandidateLayout::Vertical);
    assert_eq!(engine.settings.preedit_mode, PreeditMode::Hidden);
    assert!(engine.scheme.host_options().page_cycle);
    let session_id = engine.session_new();
    let session = engine.sessions.get(&session_id).expect("session");
    assert!(
        session.context.get_option("_vertical"),
        "竖排应写入 `_vertical`"
    );
    // 最短保留码数属方案会话状态（见 `crates/hux-scheme/tiger`）：
    // 平台侧只验证配置已按竖排 / 页大小等映射到方案。
    assert_eq!(engine.scheme.host_options().page_size, 7);
    // 越界钳制（0..=20）。
    let clamped = HuxOptions {
        min_retained_input_length: 999,
        ..ffi_options()
    };
    assert_eq!(
        unsafe { hux_engine_apply_settings(&mut engine, &clamped) },
        1
    );
    assert_eq!(
        engine.settings.min_retained_input_length,
        MAX_MIN_RETAINED_INPUT_LENGTH
    );
    std::fs::remove_dir_all(&dir).ok();
}

/// 数据装载摘要（`hux_engine_data_info`）：设置推送 / 状态菜单切换后按**新词库**重算，
/// 引擎为空返回 NULL。
///
/// 夹具：主表来自 `goldens/key_sequence`（27 条 / 25 个单字 / 5 个码）+ 一张追加码表
/// （扩展 B 汉字 `𤕫` 与部首 `⽧`，各占一个新码）——`fixture_dirs` 的主表在 golden 夹具里，
/// 那里没有追加表，看不出字集开关的效果。
#[test]
fn data_info_tracks_settings_and_option_changes() {
    let _guard = serial();
    const EXTRA_TABLES: &str = "tiger_sentence.codes.txt,tiger_sentence.codes.huma.txt";
    let data = temp_user_dir("data-info-data");
    std::fs::copy(
        hux_test_support::repo_path("goldens/key_sequence/tiger_sentence.codes.txt"),
        data.join("tiger_sentence.codes.txt"),
    )
    .expect("复制夹具主表");
    std::fs::write(
        data.join("tiger_sentence.codes.huma.txt"),
        "𤕫\tzzzv\n⽧\tzzzw\n",
    )
    .expect("写追加码表");
    let user = temp_user_dir("data-info-user");
    let mut engine = Engine::new_with_dirs(host(), vec![data.clone()], None, Some(user.clone()));

    let info = |engine: &Engine| -> String {
        let pointer = unsafe { hux_engine_data_info(engine) };
        assert!(!pointer.is_null(), "引擎有效时摘要非空");
        unsafe { std::ffi::CStr::from_ptr(pointer) }
            .to_string_lossy()
            .into_owned()
    };
    let summary = |tables: &str, entries: usize, chars: usize, full: u8, filter: u8| {
        format!(
            "code_tables=[{tables}] entries={entries} chars={chars} full_charset={full} \
             filter_non_han={filter}"
        )
    };
    let main_only = "tiger_sentence.codes.txt";

    // 出厂口径：两个开关都开，追加表在（非法汉字行已被过滤）。
    assert_eq!(info(&engine), summary(EXTRA_TABLES, 28, 26, 1, 1));
    // 缓存：连续两次调用返回同一指针（摘要按需算一次，宿主不必自己缓存）。
    let first = unsafe { hux_engine_data_info(&engine) };
    assert_eq!(unsafe { hux_engine_data_info(&engine) }, first);

    // 配置页推送：关掉全字集 ⇒ 摘要与词库同时变，且设置值写回 `options.yaml`。
    engine.apply_settings(Settings {
        full_charset: false,
        ..Default::default()
    });
    assert_eq!(info(&engine), summary(main_only, 27, 25, 0, 1));
    let persisted = std::fs::read_to_string(user.join(OPTIONS_FILE)).expect("options.yaml");
    assert!(
        persisted.contains("tiger_sentence_full_charset: false"),
        "设置推送应写回持久化选项：{persisted}"
    );

    // 状态菜单：打开全字集、关掉过滤 ⇒ 追加表两行都入词库。
    assert!(engine.set_option_value("tiger_sentence_full_charset", true));
    assert!(engine.set_option_value("tiger_sentence_filter_non_han", false));
    assert_eq!(info(&engine), summary(EXTRA_TABLES, 29, 27, 1, 0));

    // 重新部署：重读数据与持久化选项，摘要照新状态重算。
    assert!(engine.redeploy());
    assert_eq!(info(&engine), summary(EXTRA_TABLES, 29, 27, 1, 0));

    // 空指针安全。
    assert!(unsafe { hux_engine_data_info(std::ptr::null()) }.is_null());
    std::fs::remove_dir_all(&data).ok();
    std::fs::remove_dir_all(&user).ok();
}

/// 候选竖排：作用于已存在会话（host 读取 `_vertical`）。
#[test]
fn candidate_layout_applies_to_existing_sessions() {
    let _guard = serial();
    let mut engine = TestEngine::new(host(), fixture_dirs(), None, None);
    engine.apply_settings(Settings {
        candidate_layout: CandidateLayout::Vertical,
        ..Default::default()
    });
    assert!(engine.session().context.get_option("_vertical"));
    engine.apply_settings(Settings::default());
    assert!(!engine.session().context.get_option("_vertical"));
}

/// 预编辑内容三态：候选分码（默认）/ 原始输入 / 不显示。
#[test]
fn preedit_mode_variants() {
    let _guard = serial();
    UPDATES.lock().unwrap().clear();
    let mut engine = TestEngine::new(host(), fixture_dirs(), None, None);
    let type_abcd = |engine: &mut TestEngine| {
        for code in *b"abcd" {
            engine.key(u32::from(code), 0, false);
        }
    };
    type_abcd(&mut engine);
    assert_eq!(last_update().0, "ab cd", "默认：候选分码");
    engine.apply_settings(Settings {
        preedit_mode: PreeditMode::RawInput,
        ..Default::default()
    });
    engine.reset();
    UPDATES.lock().unwrap().clear();
    type_abcd(&mut engine);
    assert_eq!(last_update().0, "abcd", "原始输入");
    engine.apply_settings(Settings {
        preedit_mode: PreeditMode::Hidden,
        ..Default::default()
    });
    engine.reset();
    UPDATES.lock().unwrap().clear();
    type_abcd(&mut engine);
    let (preedit, cursor, candidates, _, _, _) = last_update();
    assert!(preedit.is_empty(), "不显示：预编辑为空");
    assert_eq!(cursor, 0);
    assert!(!candidates.is_empty(), "候选不受影响");
}

#[test]
fn ffi_apply_settings_maps_lookup_keys() {
    let _guard = serial();
    let engine = ffi_engine(temp_user_dir("ffi-lookup-keys"));
    assert!(!engine.is_null());
    let options = ffi_options();
    assert_eq!(unsafe { hux_engine_apply_settings(engine, &options) }, 1);
    let state = unsafe { &mut *engine };
    assert_eq!(
        state.settings.reverse_lookup_pronunciation_keys,
        vec!["semicolon", "colon"]
    );
    assert_eq!(
        state.settings.reverse_lookup_character_keys,
        vec!["Shift+grave"]
    );
    unsafe { hux_engine_free(engine) };
}

#[test]
fn ffi_apply_settings_maps_page_options() {
    let _guard = serial();
    let engine = ffi_engine(temp_user_dir("ffi-page-options"));
    assert!(!engine.is_null());
    let options = ffi_options();
    assert_eq!(unsafe { hux_engine_apply_settings(engine, &options) }, 1);
    let state = unsafe { &mut *engine };
    assert_eq!(state.settings.page_size, 7);
    assert_eq!(state.settings.page_up_keys, vec!["comma"]);
    assert_eq!(
        state.settings.page_down_keys,
        vec!["period", "bracketright"]
    );
    assert!(state.settings.digit_select);
    assert_eq!(state.scheme.host_options().page_size, 7);
    assert_eq!(
        state.scheme.host_options().page_up_keys,
        vec![KeyEvent::from_repr("comma").unwrap()]
    );
    assert_eq!(
        state.scheme.host_options().page_down_keys,
        vec![
            KeyEvent::from_repr("period").unwrap(),
            KeyEvent::from_repr("bracketright").unwrap()
        ]
    );
    unsafe { hux_engine_free(engine) };
}

#[test]
fn ffi_roundtrip() {
    let _guard = serial();
    let engine = ffi_engine(temp_user_dir("ffi-roundtrip"));
    assert!(!engine.is_null());
    let status = unsafe { hux_engine_status(engine) };
    assert!(!status.is_null());
    // 另一半：临时用户目录里确实建起了学习库（不再是真实 `~/.local/share/fcitx5/hux`）。
    assert!(
        unsafe { &*engine }.learning.store_ready(),
        "临时用户目录的学习库应就绪"
    );
    let session = unsafe { hux_engine_session_new(engine) };
    assert_ne!(session, 0, "会话创建应返回有效 id");
    let consumed = unsafe { hux_engine_key(engine, session, u32::from(b'a'), 0, 0) };
    assert_eq!(consumed & HUX_KEY_CONSUMED, HUX_KEY_CONSUMED);
    unsafe { hux_engine_session_free(engine, session) };
    unsafe { hux_engine_free(engine) };
}
