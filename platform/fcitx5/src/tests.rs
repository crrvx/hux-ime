// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

use crate::*;
use std::sync::Mutex;

static COMMITS: Mutex<Vec<String>> = Mutex::new(Vec::new());
type UpdateSnapshot = (String, i32, Vec<String>, i32, String, String);
static UPDATES: Mutex<Vec<UpdateSnapshot>> = Mutex::new(Vec::new());
/// 回调记录为进程级静态：使用它们的测试串行执行，避免并发串扰。
static TEST_SEQUENCE: Mutex<()> = Mutex::new(());

fn serial() -> std::sync::MutexGuard<'static, ()> {
    TEST_SEQUENCE
        .lock()
        .unwrap_or_else(|error| error.into_inner())
}

unsafe extern "C" fn record_commit(_user: *mut c_void, text: *const c_char) {
    let text = unsafe { std::ffi::CStr::from_ptr(text) };
    COMMITS
        .lock()
        .unwrap()
        .push(text.to_string_lossy().into_owned());
}

unsafe extern "C" fn record_update(
    _user: *mut c_void,
    preedit: *const c_char,
    cursor: i32,
    texts: *const *const c_char,
    _comments: *const *const c_char,
    count: i32,
    selected: i32,
    aux_up: *const c_char,
    aux_down: *const c_char,
) {
    let preedit = unsafe { std::ffi::CStr::from_ptr(preedit) }
        .to_string_lossy()
        .into_owned();
    let mut candidates = Vec::new();
    for index in 0..count {
        let text = unsafe { *texts.add(index as usize) };
        candidates.push(
            unsafe { std::ffi::CStr::from_ptr(text) }
                .to_string_lossy()
                .into_owned(),
        );
    }
    let read = |value: *const c_char| {
        if value.is_null() {
            String::new()
        } else {
            unsafe { std::ffi::CStr::from_ptr(value) }
                .to_string_lossy()
                .into_owned()
        }
    };
    let aux_up = read(aux_up);
    let aux_down = read(aux_down);
    UPDATES
        .lock()
        .unwrap()
        .push((preedit, cursor, candidates, selected, aux_up, aux_down));
}

fn host() -> Option<HostCallback> {
    Some(HostCallback {
        user: std::ptr::null_mut(),
        commit: Some(record_commit),
        update: Some(record_update),
    })
}

fn fixture_dirs() -> Vec<PathBuf> {
    vec![
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../goldens/key_sequence"),
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../data"),
    ]
}

fn temp_user_dir(tag: &str) -> PathBuf {
    hux_test_support::temp_dir(&format!("user-{tag}"))
}

/// 最近一次 UI 快照（preedit、字节光标、候选、高亮）。
fn last_update() -> UpdateSnapshot {
    UPDATES.lock().unwrap().last().cloned().expect("update")
}

/// 测试用引擎包装：预建一个会话，按键/重置/点击/周边文本自动带上会话 id。
struct TestEngine {
    engine: Engine,
    session: u64,
}

impl TestEngine {
    fn new(
        host: Option<HostCallback>,
        dirs: Vec<PathBuf>,
        model_path: Option<PathBuf>,
        options_dir: Option<PathBuf>,
    ) -> Self {
        let mut engine = Engine::new_with_dirs(host, dirs, model_path, options_dir);
        let session = engine.session_new();
        Self { engine, session }
    }

    fn key(&mut self, keysym: u32, states: u32, release: bool) -> bool {
        self.engine.key(self.session, keysym, states, release)
    }

    fn reset(&mut self) {
        self.engine.reset(self.session);
    }

    fn select_candidate(&mut self, index: usize) -> bool {
        self.engine.select_candidate(self.session, index)
    }

    fn set_surrounding(&mut self, text: Option<&str>, cursor_chars: usize) {
        self.engine
            .set_surrounding(self.session, text, cursor_chars);
    }

    fn session(&self) -> &Session {
        self.engine.sessions.get(&self.session).expect("session")
    }
}

impl std::ops::Deref for TestEngine {
    type Target = Engine;
    fn deref(&self) -> &Engine {
        &self.engine
    }
}

impl std::ops::DerefMut for TestEngine {
    fn deref_mut(&mut self) -> &mut Engine {
        &mut self.engine
    }
}

#[test]
fn engine_enables_learning_store() {
    let _guard = serial();
    let dir = temp_user_dir("learning");
    let engine = TestEngine::new(host(), fixture_dirs(), None, Some(dir.clone()));
    assert!(
        engine.engine.learning.store_ready(),
        "用户目录可用时学习库应就绪"
    );
    assert!(
        engine
            .engine
            .learning_mode
            .starts_with("sentence-v1|rules=")
    );
    assert!(
        dir.join(format!(
            "{}.userdb",
            learning_store::store_name(hux_scheme_tiger::scheme::SCHEME_ID)
        ))
        .is_dir()
    );
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn engine_applies_learning_after_key() {
    // 学习索引的「已应用版本」属方案内部状态（见 `crates/hux-scheme/tiger` 的单测）；
    // 平台侧只验证接线：按键路径把当前库索引交给方案且库保持就绪。
    let _guard = serial();
    let dir = temp_user_dir("learning-apply");
    let mut engine = TestEngine::new(host(), fixture_dirs(), None, Some(dir.clone()));
    engine.key(u32::from(b'a'), 0, false);
    assert!(engine.engine.learning.store_ready(), "学习库应保持就绪");
    assert!(
        engine
            .engine
            .learning_mode
            .starts_with("sentence-v1|rules=")
    );
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn typing_shows_preedit_and_candidates() {
    let _guard = serial();
    UPDATES.lock().unwrap().clear();
    let mut engine = TestEngine::new(host(), fixture_dirs(), None, None);
    // 「甲/乙」共用码 ab：输入两个键后出现候选。
    assert!(engine.key(u32::from(b'a'), 0, false));
    assert!(engine.key(u32::from(b'b'), 0, false));
    assert_eq!(engine.session().context.input(), b"ab");
    let (preedit, cursor, candidates, selected, _, _) = last_update();
    assert_eq!(preedit, "ab");
    assert_eq!(cursor, 2);
    assert_eq!(candidates, vec!["甲".to_string(), "乙".to_string()]);
    assert_eq!(selected, 0);
}

/// 会话隔离：两个输入上下文各自维护组合，互不影响。
#[test]
fn sessions_are_isolated() {
    let _guard = serial();
    COMMITS.lock().unwrap().clear();
    UPDATES.lock().unwrap().clear();
    let mut engine = Engine::new_with_dirs(host(), fixture_dirs(), None, None);
    let first = engine.session_new();
    let second = engine.session_new();
    assert_ne!(first, second, "会话 id 应递增且不同");
    for code in *b"ab" {
        assert!(engine.key(first, u32::from(code), 0, false));
    }
    for code in *b"ja" {
        assert!(engine.key(second, u32::from(code), 0, false));
    }
    assert_eq!(engine.sessions[&first].context.input(), b"ab");
    assert_eq!(engine.sessions[&second].context.input(), b"ja");
    // 一个会话上屏不影响另一个。
    assert!(engine.key(first, 0x20, 0, false));
    assert_eq!(COMMITS.lock().unwrap().last().unwrap(), "甲");
    assert_eq!(engine.sessions[&second].context.input(), b"ja");
}

/// 重置直接丢弃组合（不提交），面板清空；失焦的提交由核心/前端处理（不在本层）。
#[test]
fn reset_discards_composition_without_commit() {
    let _guard = serial();
    COMMITS.lock().unwrap().clear();
    UPDATES.lock().unwrap().clear();
    let mut engine = Engine::new_with_dirs(host(), fixture_dirs(), None, None);
    let session = engine.session_new();
    for code in *b"ab" {
        assert!(engine.key(session, u32::from(code), 0, false));
    }
    engine.reset(session);
    assert!(COMMITS.lock().unwrap().is_empty(), "重置不应提交组合");
    assert!(engine.sessions[&session].context.input().is_empty());
    let (preedit, _, candidates, _, aux_up, aux_down) = last_update();
    assert!(preedit.is_empty(), "预编辑应清空");
    assert!(candidates.is_empty(), "候选应清空");
    assert!(aux_up.is_empty() && aux_down.is_empty(), "辅助两排应清空");
}

/// 会话释放：销毁后按键与点击被忽略。
#[test]
fn session_free_drops_state() {
    let _guard = serial();
    let mut engine = Engine::new_with_dirs(host(), fixture_dirs(), None, None);
    let session = engine.session_new();
    assert!(engine.key(session, u32::from(b'a'), 0, false));
    engine.session_free(session);
    assert!(!engine.sessions.contains_key(&session));
    assert!(
        !engine.key(session, u32::from(b'a'), 0, false),
        "已释放会话忽略按键"
    );
    assert!(!engine.select_candidate(session, 0), "已释放会话忽略点击");
}

/// 运行时开关（状态菜单）作用于全部会话；新会话继承当前值（含无存储情形）。
#[test]
fn runtime_option_applies_to_all_sessions() {
    let _guard = serial();
    let mut engine = Engine::new_with_dirs(host(), fixture_dirs(), None, None);
    let first = engine.session_new();
    let second = engine.session_new();
    assert!(engine.set_option_value("full_shape", true));
    assert!(engine.sessions[&first].context.get_option("full_shape"));
    assert!(engine.sessions[&second].context.get_option("full_shape"));
    let third = engine.session_new();
    assert!(
        engine.sessions[&third].context.get_option("full_shape"),
        "新会话应继承当前开关"
    );
}

/// 配置变更（设置页）作用于已存在的会话。
#[test]
fn apply_settings_reaches_existing_sessions() {
    let _guard = serial();
    let mut engine = Engine::new_with_dirs(host(), fixture_dirs(), None, None);
    let session = engine.session_new();
    engine.apply_settings(Settings {
        full_shape: true,
        ascii_punct: true,
        ..Default::default()
    });
    assert!(engine.sessions[&session].context.get_option("full_shape"));
    assert!(engine.sessions[&session].context.get_option("ascii_punct"));
}

#[test]
fn space_commits_highlighted_candidate() {
    let _guard = serial();
    COMMITS.lock().unwrap().clear();
    let mut engine = TestEngine::new(host(), fixture_dirs(), None, None);
    assert!(engine.key(u32::from(b'a'), 0, false));
    assert!(engine.key(u32::from(b'b'), 0, false));
    assert!(engine.key(0x20, 0, false)); // space
    assert_eq!(COMMITS.lock().unwrap().last().unwrap(), "甲");
    assert!(engine.session().context.input().is_empty());
}

/// 候选点击（面板 `CandidateWord::select`）：按全局索引选中并上屏。
#[test]
fn candidate_click_commits_selected_candidate() {
    let _guard = serial();
    COMMITS.lock().unwrap().clear();
    UPDATES.lock().unwrap().clear();
    let mut engine = TestEngine::new(host(), fixture_dirs(), None, None);
    for code in *b"ab" {
        engine.key(u32::from(code), 0, false);
    }
    let (_, _, candidates, _, _, _) = last_update();
    assert!(candidates.len() >= 2, "夹具 ab 应有至少 2 个候选");
    let second = candidates[1].clone();
    assert!(engine.select_candidate(1), "点击页内候选应被处理");
    assert_eq!(COMMITS.lock().unwrap().last().unwrap(), &second);
    assert!(
        engine.session().context.input().is_empty(),
        "上屏后组合应清空"
    );
}

/// 候选点击越界（如列表已被刷新）：忽略，不产生提交、组合不变。
#[test]
fn candidate_click_out_of_range_ignored() {
    let _guard = serial();
    COMMITS.lock().unwrap().clear();
    UPDATES.lock().unwrap().clear();
    let mut engine = TestEngine::new(host(), fixture_dirs(), None, None);
    for code in *b"ab" {
        engine.key(u32::from(code), 0, false);
    }
    assert!(!engine.select_candidate(99));
    assert!(COMMITS.lock().unwrap().is_empty());
    assert_eq!(engine.session().context.input(), b"ab");
}

/// 宿主自发提交接学习：Tab 选字后由宿主链提交（组合中大写字母），事件应落库。
#[test]
fn host_commit_records_tab_learning() {
    let _guard = serial();
    let dir = temp_user_dir("host-learning");
    COMMITS.lock().unwrap().clear();
    UPDATES.lock().unwrap().clear();
    let mut engine = TestEngine::new(host(), fixture_dirs(), None, Some(dir.clone()));
    for code in *b"ab" {
        engine.key(u32::from(code), 0, false);
    }
    assert!(engine.key(0xff09, 0, false), "Tab 应被消费");
    let before = engine.learning.index_version();
    // 大写 A（0x41）：core 交宿主链 `char_handler`，先提交组合再交应用。
    engine.key(0x41, 0, false);
    assert_eq!(COMMITS.lock().unwrap().last().unwrap(), "乙");
    assert_ne!(
        engine.learning.index_version(),
        before,
        "宿主提交应写入学习库"
    );
    std::fs::remove_dir_all(&dir).ok();
}

/// 上翻页键：候选菜单可见即消费（首屏也不落作标点/输入）。
#[test]
fn page_up_is_consumed_with_menu() {
    let _guard = serial();
    COMMITS.lock().unwrap().clear();
    UPDATES.lock().unwrap().clear();
    let mut engine = TestEngine::new(host(), fixture_dirs(), None, None);
    for code in *b"ja" {
        engine.key(u32::from(code), 0, false);
    }
    assert!(engine.key(0x2d, 0, false), "- 菜单可见时应被消费");
    assert!(COMMITS.lock().unwrap().is_empty(), "不应作为标点/输入上屏");
}

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

/// 多项触发键（`KeyList`）：两项均可进入音反查。
#[test]
fn sound_to_char_shape_accepts_multiple_trigger_keys() {
    let _guard = serial();
    UPDATES.lock().unwrap().clear();
    let mut engine = TestEngine::new(host(), fixture_dirs(), None, None);
    engine.apply_settings(Settings {
        sound_to_char_shape_keys: vec!["grave".to_string(), "semicolon".to_string()],
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

/// 运行时开关（状态菜单）：白名单读写往返，未知选项拒绝。
#[test]
fn runtime_option_roundtrip_and_whitelist() {
    let _guard = serial();
    let mut engine = TestEngine::new(host(), fixture_dirs(), None, None);
    for name in engine.runtime_options() {
        let value = engine.option_value(name).expect("白名单选项");
        assert!(engine.set_option_value(name, !value), "{name} 应可设置");
        assert_eq!(engine.option_value(name), Some(!value));
    }
    assert_eq!(engine.option_value("not_an_option"), None);
    assert!(!engine.set_option_value("not_an_option", true));
}

/// 运行时开关落盘到 `tiger_sentence.options.yaml`，重启后保持。
#[test]
fn runtime_option_persists_to_store() {
    let _guard = serial();
    let dir = temp_user_dir("runtime-option");
    let mut engine = TestEngine::new(host(), fixture_dirs(), None, Some(dir.clone()));
    assert!(engine.set_option_value("full_shape", true));
    let text = std::fs::read_to_string(dir.join(OPTIONS_FILE)).expect("options.yaml");
    assert!(text.contains("full_shape: true"), "{text}");
    let restarted = TestEngine::new(host(), fixture_dirs(), None, Some(dir.clone()));
    assert!(
        restarted.session().context.get_option("full_shape"),
        "重启后应保持"
    );
    std::fs::remove_dir_all(&dir).ok();
}

/// 合并顺序：`options.yaml`（状态菜单开关）优先于配置界面设置。
#[test]
fn apply_settings_respects_store_values() {
    let _guard = serial();
    let dir = temp_user_dir("settings-order");
    std::fs::write(dir.join(OPTIONS_FILE), "options:\n  full_shape: true\n").expect("write");
    let mut engine = TestEngine::new(host(), fixture_dirs(), None, Some(dir.clone()));
    engine.apply_settings(Settings {
        full_shape: false,
        ..Default::default()
    });
    assert!(
        engine.session().context.get_option("full_shape"),
        "options.yaml 应优先于设置"
    );
    std::fs::remove_dir_all(&dir).ok();
}

/// FFI：运行时开关读写（含未知选项）。
#[test]
fn ffi_runtime_option_roundtrip() {
    let _guard = serial();
    let dir = temp_user_dir("ffi-option");
    let mut engine = TestEngine::new(host(), fixture_dirs(), None, Some(dir.clone()));
    let name = CString::new("full_shape").expect("name");
    assert_eq!(
        unsafe { hux_engine_option_value(&mut *engine, name.as_ptr()) },
        0
    );
    assert_eq!(
        unsafe { hux_engine_set_option(&mut *engine, name.as_ptr(), 1) },
        1
    );
    assert_eq!(
        unsafe { hux_engine_option_value(&mut *engine, name.as_ptr()) },
        1
    );
    let unknown = CString::new("not_an_option").expect("name");
    assert_eq!(
        unsafe { hux_engine_option_value(&mut *engine, unknown.as_ptr()) },
        -1
    );
    assert_eq!(
        unsafe { hux_engine_set_option(&mut *engine, unknown.as_ptr(), 1) },
        0
    );
    std::fs::remove_dir_all(&dir).ok();
}

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

#[test]
fn composing_left_right_move_caret_and_toggle_candidates() {
    let _guard = serial();
    UPDATES.lock().unwrap().clear();
    let mut engine = TestEngine::new(host(), fixture_dirs(), None, None);
    assert!(engine.key(u32::from(b'a'), 0, false));
    assert!(engine.key(u32::from(b'b'), 0, false));
    // ←：光标左移；组合按 caret 前缀重建（候选清空）
    assert!(engine.key(0xff51, 0, false), "组合中 Left 应被消费");
    let (preedit, cursor, candidates, _, _, _) = last_update();
    assert_eq!(preedit, "ab");
    assert_eq!(cursor, 1);
    assert!(candidates.is_empty(), "光标在输入中间时无候选");
    // →：回到末尾，候选恢复
    assert!(engine.key(0xff53, 0, false));
    let (_, cursor, candidates, _, _, _) = last_update();
    assert_eq!(cursor, 2);
    assert_eq!(candidates, vec!["甲".to_string(), "乙".to_string()]);
}

#[test]
fn composing_up_down_move_highlight() {
    let _guard = serial();
    UPDATES.lock().unwrap().clear();
    let mut engine = TestEngine::new(host(), fixture_dirs(), None, None);
    assert!(engine.key(u32::from(b'a'), 0, false));
    assert!(engine.key(u32::from(b'b'), 0, false));
    // ↓：高亮下移；↑ 到首项
    assert!(engine.key(0xff54, 0, false));
    let (_, _, _, selected, _, _) = last_update();
    assert_eq!(selected, 1);
    assert!(engine.key(0xff52, 0, false));
    let (_, _, _, selected, _, _) = last_update();
    assert_eq!(selected, 0);
}

#[test]
fn composing_backspace_deletes_input_and_clears_composition() {
    let _guard = serial();
    UPDATES.lock().unwrap().clear();
    let mut engine = TestEngine::new(host(), fixture_dirs(), None, None);
    assert!(engine.key(u32::from(b'a'), 0, false));
    assert!(engine.key(u32::from(b'b'), 0, false));
    // 退格：删除输入
    assert!(engine.key(0xff08, 0, false));
    let (preedit, cursor, candidates, _, _, _) = last_update();
    assert_eq!(preedit, "a");
    assert_eq!(cursor, 1);
    assert!(candidates.is_empty());
    // 再退格清空组合
    assert!(engine.key(0xff08, 0, false));
    let (preedit, _, candidates, _, _, _) = last_update();
    assert!(preedit.is_empty());
    assert!(candidates.is_empty());
}

#[test]
fn punctuation_commits_when_idle() {
    let _guard = serial();
    COMMITS.lock().unwrap().clear();
    let mut engine = TestEngine::new(host(), fixture_dirs(), None, None);
    // symbols.yaml half_shape："." → 。
    assert!(engine.key(0x2e, 0, false), "period 应被消费");
    assert_eq!(COMMITS.lock().unwrap().last().unwrap(), "。");
}

#[test]
fn punctuation_appends_to_composition() {
    let _guard = serial();
    COMMITS.lock().unwrap().clear();
    let mut engine = TestEngine::new(host(), fixture_dirs(), None, None);
    assert!(engine.key(u32::from(b'a'), 0, false));
    assert!(engine.key(u32::from(b'b'), 0, false));
    assert!(engine.key(0x2c, 0, false), "comma 应被消费");
    assert_eq!(COMMITS.lock().unwrap().last().unwrap(), "甲，");
    assert!(engine.session().context.input().is_empty());
}

#[test]
fn punctuation_pair_alternates() {
    let _guard = serial();
    COMMITS.lock().unwrap().clear();
    let mut engine = TestEngine::new(host(), fixture_dirs(), None, None);
    // apostrophe：'‘' / '’'
    for text in ["‘", "’"] {
        assert!(engine.key(0x27, 0, false));
        assert_eq!(COMMITS.lock().unwrap().last().unwrap(), text);
    }
}

#[test]
fn punctuation_passes_unmapped_space() {
    let _guard = serial();
    let mut engine = TestEngine::new(host(), fixture_dirs(), None, None);
    assert!(!engine.key(0x20, 0, false), "空闲空格交宿主");
}

#[test]
fn uppercase_commits_composition_and_requests_forward() {
    let _guard = serial();
    // 用户报告：组合中收到大写字母时，应先上屏当前候选（而非把字母插到预编辑之前）。
    // 核心语义保持「提交 + 不消费」（同 librime）；宿主层据 `forward_after_commit`
    // 消费该键并以 forwardKey 重发，保证「候选 → 字母」送达顺序。
    COMMITS.lock().unwrap().clear();
    UPDATES.lock().unwrap().clear();
    let mut engine = TestEngine::new(host(), fixture_dirs(), None, None);
    assert!(engine.key(u32::from(b'a'), 0, false));
    assert!(!engine.forward_after_commit, "普通输入不应请求转发");
    assert!(engine.key(u32::from(b'b'), 0, false));
    assert!(
        !engine.key(0x41, FCITX_SHIFT, false),
        "大写字母应交宿主（不消费）"
    );
    assert!(
        engine.forward_after_commit,
        "提交且未消费 → 宿主应消费并重发该键"
    );
    assert_eq!(COMMITS.lock().unwrap().last().unwrap(), "甲");
    assert!(
        engine.session().context.input().is_empty(),
        "组合已提交并清空"
    );
}

#[test]
fn idle_uppercase_does_not_request_forward() {
    let _guard = serial();
    let mut engine = TestEngine::new(host(), fixture_dirs(), None, None);
    assert!(!engine.key(0x41, FCITX_SHIFT, false));
    assert!(!engine.forward_after_commit);
}

#[test]
fn apply_settings_switches_context_options() {
    let _guard = serial();
    let mut engine = TestEngine::new(host(), fixture_dirs(), None, None);
    engine.apply_settings(Settings {
        full_shape: true,
        ascii_punct: true,
        ..Default::default()
    });
    assert!(engine.session().context.get_option("full_shape"));
    assert!(engine.session().context.get_option("ascii_punct"));
}

#[test]
fn apply_settings_disables_learning_mode() {
    let _guard = serial();
    let mut engine = TestEngine::new(host(), fixture_dirs(), None, None);
    engine.apply_settings(Settings {
        tab_learning: false,
        ..Default::default()
    });
    assert!(
        engine.engine.learning_mode.is_empty(),
        "关闭 Tab 学习 → 学习 mode 为空"
    );
}

fn key_list(keys: &[(i32, i32)]) -> HuxKeyList {
    let mut list = HuxKeyList::default();
    for (index, (sym, states)) in keys.iter().enumerate().take(HUX_MAX_KEYS) {
        list.sym[index] = *sym;
        list.states[index] = *states;
    }
    list.count = keys.len().min(HUX_MAX_KEYS) as i32;
    list
}

fn ffi_options() -> HuxOptions {
    HuxOptions {
        early_commit: 0,
        early_commit_to_preedit: 1,
        allow_duplicate_single: 1,
        full_shape: 1,
        ascii_punct: 1,
        tab_learning: 0,
        high_freq_limit: 800,
        // 音反查：`；`（无修饰）与 Shift+`；`（= `:`）。
        sound_to_char_shape: key_list(&[(0x3b, 0), (0x3a, 0)]),
        // 字反查：Shift+`（= `~`）。
        char_to_sound_shape: key_list(&[(0x60, 1)]),
        page_size: 7,
        // 翻页：`.` 与 `]`。
        page_up: key_list(&[(0x2c, 0)]),
        page_down: key_list(&[(0x2e, 0), (0x5d, 0)]),
        digit_select: 1,
        candidate_layout: 0,
        preedit_mode: 0,
        page_cycle: 0,
        min_retained_raw_length: 0,
    }
}

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
        engine.learning_mode.is_empty(),
        "tab_learning=0 → 学习 mode 为空"
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
        min_retained_raw_length: 4,
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
        min_retained_raw_length: 999,
        ..ffi_options()
    };
    assert_eq!(
        unsafe { hux_engine_apply_settings(&mut engine, &clamped) },
        1
    );
    assert_eq!(
        engine.settings.min_retained_raw_length,
        MAX_MIN_RETAINED_RAW_LENGTH
    );
    std::fs::remove_dir_all(&dir).ok();
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
    let engine = unsafe { hux_engine_new(std::ptr::null()) };
    assert!(!engine.is_null());
    let options = ffi_options();
    assert_eq!(unsafe { hux_engine_apply_settings(engine, &options) }, 1);
    let state = unsafe { &mut *engine };
    assert_eq!(
        state.settings.sound_to_char_shape_keys,
        vec!["semicolon", "colon"]
    );
    assert_eq!(state.settings.char_to_sound_shape_keys, vec!["Shift+grave"]);
    unsafe { hux_engine_free(engine) };
}

#[test]
fn ffi_apply_settings_maps_page_options() {
    let _guard = serial();
    let engine = unsafe { hux_engine_new(std::ptr::null()) };
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
fn reset_clears_panel() {
    let _guard = serial();
    UPDATES.lock().unwrap().clear();
    let mut engine = TestEngine::new(host(), fixture_dirs(), None, None);
    engine.key(u32::from(b'a'), 0, false);
    engine.reset();
    let (preedit, _, candidates, _, _, _) =
        UPDATES.lock().unwrap().last().cloned().expect("update");
    assert!(preedit.is_empty());
    assert!(candidates.is_empty());
    assert!(engine.session().context.input().is_empty());
}

/// 音反查（⑧-1）端到端：设置 → 前缀识别 → 候选/注释 → 预编辑提示 → 空格上屏。
#[test]
fn sound_to_char_shape_end_to_end() {
    let _guard = serial();
    COMMITS.lock().unwrap().clear();
    UPDATES.lock().unwrap().clear();
    let dirs = vec![
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../goldens/sound_to_char_shape"),
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../data"),
    ];
    let mut engine = TestEngine::new(host(), dirs, None, None);
    assert!(engine.key(0x3a, FCITX_ALT, false), "音反查触发键应被消费");
    for code in *b"zho" {
        assert!(engine.key(u32::from(code), 0, false), "音反查输入应被消费");
    }
    let (preedit, _, candidates, _, _, _) = last_update();
    assert_eq!(preedit, ":zho〔拼音〕");
    assert_eq!(
        candidates,
        vec!["中哦", "中龘", "中欧", "找哦", "兆欧", "找欧"]
    );
    assert!(engine.key(0x20, 0, false));
    assert_eq!(COMMITS.lock().unwrap().last().unwrap(), "中哦");
    // 音反查预编辑「按音节分码」：全拼音节之间插空格。
    engine.reset();
    assert!(engine.key(0x3a, FCITX_ALT, false));
    for code in *b"zhongguo" {
        assert!(engine.key(u32::from(code), 0, false));
    }
    let (preedit, _, candidates, _, _, _) = last_update();
    assert_eq!(candidates.first().map(String::as_str), Some("中国"));
    assert_eq!(preedit, ":zhong guo〔拼音〕");
}

/// 字反查（⑧-2）：默认 Alt+" 进入组合（**带修饰键不给默认候选**）；
/// 上排 = 光标左侧 1 字拼音、下排 = 虎码，步长 1；改为单字符键时才给默认可上屏候选。
#[test]
fn char_to_sound_shape_end_to_end() {
    let _guard = serial();
    COMMITS.lock().unwrap().clear();
    UPDATES.lock().unwrap().clear();
    let dirs = vec![
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../goldens/sound_to_char_shape"),
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../data"),
    ];
    let mut engine = TestEngine::new(host(), dirs, None, None);
    // 应用侧周边文本「中欧中兴」，光标在第 2 个字符后（锚点 = 2）。
    engine.set_surrounding(Some("中欧中兴"), 2);
    // 默认 Alt+"（带修饰）→ 组合无默认候选；上排「咅」、下排「虍」。
    assert!(engine.key(0x22, FCITX_ALT, false), "Alt+\" 应被消费");
    let (preedit, _, candidates, _, up, down) = last_update();
    assert_eq!(engine.session().context.input(), b"\"");
    assert!(preedit.is_empty(), "查码段不下发预编辑：{preedit:?}");
    assert!(
        candidates.is_empty(),
        "带修饰触发键不给默认候选：{candidates:?}"
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
    // 音反查：带修饰键（默认 Alt+:）**不给**默认候选；单字符键（;）才给。
    engine.reset();
    assert!(engine.key(0x3a, FCITX_ALT, false), "Alt+: 应被消费");
    let (_, _, candidates, _, _, _) = last_update();
    assert!(
        candidates.is_empty(),
        "带修饰触发键不给默认候选：{candidates:?}"
    );
    engine.reset();
    engine.apply_settings(Settings {
        sound_to_char_shape_keys: vec!["semicolon".to_string()],
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
        sound_to_char_shape_keys: vec!["grave".to_string()],
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
        char_to_sound_shape_keys: vec!["asciitilde".to_string()],
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

/// 字反查（⑧-2）夹具目录。
fn char_to_sound_shape_dirs() -> Vec<PathBuf> {
    vec![
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../goldens/sound_to_char_shape"),
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../data"),
    ]
}

/// 字反查（⑧-2）：周边文本不可用（如终端）时不显示提示，两排均为空。
#[test]
fn char_to_sound_shape_without_surrounding_shows_nothing() {
    let _guard = serial();
    UPDATES.lock().unwrap().clear();
    let mut engine = TestEngine::new(host(), char_to_sound_shape_dirs(), None, None);
    engine.set_surrounding(None, 0);
    assert!(engine.key(0x22, FCITX_ALT, false), "Alt+\" 应被消费");
    let (_, _, _, _, up, down) = last_update();
    assert!(up.is_empty(), "周边文本不可用时上排应为空：{up:?}");
    assert!(down.is_empty(), "周边文本不可用时下排应为空：{down:?}");
}

/// 字反查（⑧-2）：周边文本恢复后，同一查码段在下一次按键刷新出两排。
#[test]
fn char_to_sound_shape_refreshes_when_surrounding_available() {
    let _guard = serial();
    UPDATES.lock().unwrap().clear();
    let mut engine = TestEngine::new(host(), char_to_sound_shape_dirs(), None, None);
    engine.set_surrounding(None, 0);
    assert!(engine.key(0x22, FCITX_ALT, false), "Alt+\" 应被消费");
    engine.set_surrounding(Some("中欧中兴"), 2);
    assert!(!engine.key(0xffe1, 0, false), "修饰键不消费（触发刷新）");
    let (_, _, _, _, up, down) = last_update();
    assert_eq!(up, "咅 ?");
    assert_eq!(down, "虍 nbe/nbeq");
}

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

#[test]
fn ffi_roundtrip() {
    let _guard = serial();
    let engine = unsafe { hux_engine_new(std::ptr::null()) };
    assert!(!engine.is_null());
    let status = unsafe { hux_engine_status(engine) };
    assert!(!status.is_null());
    let session = unsafe { hux_engine_session_new(engine) };
    assert_ne!(session, 0, "会话创建应返回有效 id");
    let consumed = unsafe { hux_engine_key(engine, session, u32::from(b'a'), 0, 0) };
    assert_eq!(consumed & HUX_KEY_CONSUMED, HUX_KEY_CONSUMED);
    unsafe { hux_engine_session_free(engine, session) };
    unsafe { hux_engine_free(engine) };
}
