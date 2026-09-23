// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

use crate::*;
use hux_core::scheme::OptionDecl;
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
    // mode 串对平台是**不透明**的（§5）——平台只承诺「原样使用方案自算的串」，
    // 格式由方案自己的用例钉住（`tiger` 的 `learning_mode_follows_config_and_rules`）。
    let mode = engine.engine.scheme.learning_mode();
    assert!(!mode.is_empty(), "学习库就绪时 mode 串非空");
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
    // 平台侧验证接线**且索引确实作用到解码器**（只断言
    // `store_ready` 与 mode 非空，删掉 `Engine::finish` 里的 `apply_learning_index`
    // 调用仍全绿）。判据只用**已有可观测面**：宿主提交点落库一条纠错证据后，
    // 同码候选的排序必须随库版本变化而变（候选快照即宿主真实可见的输出，
    // 不为此新增测试专用 API）。
    let _guard = serial();
    let dir = temp_user_dir("learning-apply");
    UPDATES.lock().unwrap().clear();
    COMMITS.lock().unwrap().clear();
    let mut engine = TestEngine::new(host(), fixture_dirs(), None, Some(dir.clone()));
    assert!(engine.engine.learning.store_ready(), "学习库应就绪");
    // 只看「非空」，不看具体格式（格式归方案自己的用例）。
    assert!(
        !engine.engine.scheme.learning_mode().is_empty(),
        "学习库就绪时 mode 串非空"
    );

    // 基线：`abab`（两条同码 `ab` 边）的候选序。
    for code in *b"abab" {
        engine.key(u32::from(code), 0, false);
    }
    let baseline = last_update().2;
    assert_eq!(
        baseline,
        vec![
            "甲甲".to_string(),
            "乙甲".into(),
            "甲乙".into(),
            "乙乙".into()
        ],
        "夹具基线候选序"
    );

    // Tab 锁定第 2 个候选 → 大写 A 交宿主链提交：宿主提交点写入纠错学习事件。
    let before = engine.engine.learning.index_version();
    assert!(engine.key(0xff09, 0, false), "Tab 应被消费");
    engine.key(0x41, 0, false);
    assert_eq!(COMMITS.lock().unwrap().last().unwrap(), "乙甲");
    assert_ne!(
        engine.engine.learning.index_version(),
        before,
        "宿主提交应写入学习库（库版本变化）"
    );

    // 学习后重打同一串：候选序必须随新索引而变——索引未被应用到解码器时保持不变。
    UPDATES.lock().unwrap().clear();
    for code in *b"abab" {
        engine.key(u32::from(code), 0, false);
    }
    let learned = last_update().2;
    assert_ne!(
        learned, baseline,
        "学习索引必须作用到解码器排序（仅 store_ready / mode 非空不足为证）"
    );
    assert_eq!(
        learned,
        vec![
            "乙乙".to_string(),
            "乙甲".into(),
            "甲乙".into(),
            "甲甲".into()
        ],
        "学习后候选序（学的 `乙甲` 一侧上浮）"
    );
    assert!(engine.engine.learning.store_ready(), "学习库应保持就绪");
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

/// 学习库里的**坏帧**：跳过该条记录、不禁用整库，并进既有诊断。
///
/// 触发面：`<user dir>/tiger_sentence_learning_<hash>.userdb` 被损坏或被其它工具写坏
/// （值可能不是合法 UTF-8，`from_utf8_lossy` 会把 1 字节换成 3 字节 U+FFFD 使长度前缀错位），
/// 而 `LearningStore::open` 由 `hux_engine_new`（`extern "C"`）调用——
/// 旧实现用 `&value[a..b]` 按字节切片：落点不在字符边界即 panic，unwind 跨不过 C ABI ⇒ abort。
#[test]
fn learning_store_skips_undecodable_records_with_diagnostic() {
    let _guard = serial();
    let dir = temp_user_dir("bad-frame");
    let name = crate::learning_store::store_name("tiger_sentence");
    let path = dir.join(format!("{name}.userdb"));
    {
        let mut db = rusty_leveldb::DB::open(&path, rusty_leveldb::Options::default())
            .expect("open learning db");
        // 良构帧：时间 / mode / code / text / context。
        let good = hux_core::learning::frame(&[
            "1000.0".to_string(),
            "sentence-v2".to_string(),
            "ab".to_string(),
            "甲".to_string(),
            String::new(),
        ]);
        db.put(b"e/0000000001", good.as_bytes()).expect("put good");
        // 坏帧：长度前缀 1 落在 `é`（2 字节）中间——旧实现的 panic 点。
        db.put(b"e/0000000002", "1:é".as_bytes()).expect("put bad");
        db.flush().expect("flush");
    }
    let engine = TestEngine::new(host(), fixture_dirs(), None, Some(dir.clone()));
    let store = &engine.learning;
    assert!(store.store_ready(), "坏帧不得让整库不可用");
    assert_eq!(store.events.len(), 1, "坏帧跳过、良帧照常装载");
    assert_eq!(store.count, 2, "计数仍按库中记录数（上限判定不受影响）");
    let error = store.error.clone().unwrap_or_default();
    assert!(
        error.contains("skipped 1 undecodable record"),
        "坏帧必须计入既有诊断：{error}"
    );
    let status = engine.status.to_str().unwrap_or("").to_string();
    assert!(
        status.contains("skipped 1 undecodable record"),
        "诊断随状态串对用户可见：{status}"
    );
}

/// 宿主自发提交接学习：Tab 选字后由宿主链提交（组合中大写字母），事件应落库。
///
/// 输入取 `abab`（两条 2 码边 ⇒ composed-only）：`c69c1a8` 起差异学习只对
/// composed-only 的基线与选中项成对，整串直出（Direct）的确认不再产生事件。
#[test]
fn host_commit_records_learning_on_tab() {
    let _guard = serial();
    let dir = temp_user_dir("host-learning");
    COMMITS.lock().unwrap().clear();
    UPDATES.lock().unwrap().clear();
    let mut engine = TestEngine::new(host(), fixture_dirs(), None, Some(dir.clone()));
    for code in *b"abab" {
        engine.key(u32::from(code), 0, false);
    }
    assert!(engine.key(0xff09, 0, false), "Tab 应被消费");
    let before = engine.learning.index_version();
    // 大写 A（0x41）：core 交宿主链 `char_handler`，先提交组合再交应用。
    engine.key(0x41, 0, false);
    // 第 2 个可见候选：`甲乙`/`乙甲` 同分时按文本字节序（`乙` < `甲`）取后者。
    assert_eq!(COMMITS.lock().unwrap().last().unwrap(), "乙甲");
    assert_ne!(
        engine.learning.index_version(),
        before,
        "宿主提交应写入学习库"
    );
    std::fs::remove_dir_all(&dir).ok();
}

/// 整串直出（Direct）的 Tab 确认不再是纠错证据（参照 `78bfaf3` 的探针期望：
/// 「Direct→Direct 学习计数不变」）。`ab` 只有一条整串边 ⇒ 两个候选都是 Direct。
#[test]
fn host_commit_direct_choice_records_no_learning() {
    let _guard = serial();
    let dir = temp_user_dir("host-learning-direct");
    COMMITS.lock().unwrap().clear();
    UPDATES.lock().unwrap().clear();
    let mut engine = TestEngine::new(host(), fixture_dirs(), None, Some(dir.clone()));
    for code in *b"ab" {
        engine.key(u32::from(code), 0, false);
    }
    assert!(engine.key(0xff09, 0, false), "Tab 应被消费");
    let before = engine.learning.index_version();
    engine.key(0x41, 0, false);
    assert_eq!(COMMITS.lock().unwrap().last().unwrap(), "乙");
    assert_eq!(
        engine.learning.index_version(),
        before,
        "Direct → Direct 不产生学习事件"
    );
    std::fs::remove_dir_all(&dir).ok();
}

/// 菜单可见时的翻页键不再被方案标点分支遮蔽（**本仓有意偏离上游 `abad411`**）。
///
/// 上游标点分支对菜单可见的**所有**可打印 ASCII 标点先「暂存学习 + 确认组合」再交标点表，
/// 于是 schema 的 key_binder 翻页绑定（`-`/`=`，以及绑到翻页的 `[`/`]`）在这条路径上被遮蔽
/// （`Page_Down`/`Page_Up`/`Tab` 不受影响）。本仓在标点分支入口先问**与宿主同一套**判据
/// `hux_core::host::paging_action`：判为翻页的键不由标点分支消费，落回宿主链执行翻页。
/// 最小复现：`j a equal`（见 `interaction::tests` 的同名用例）；
/// 受影响的上游金样用例在差分测试中按 `DEVIATIONS` 登记（金样字节保持原样）。
///
/// **用户决定 B（语义强化）**：上翻页键与下翻页键**同前置**——只要菜单可见就判翻页，
/// 不再要求参照 `when: paging` 的「已翻过页」标签（该标签已随其唯一读取方删除）。
/// 代价：菜单可见时 `-`/`=`/`[`/`]` 不再能作为标点打出。
///
/// 覆盖：① `=` 下翻不提交；② 翻页后 `-` 上翻不提交；③ **首屏**（未翻页）`-` 同样上翻
/// （回归场景，不再依赖任何标签）；④ `Page_Down` 始终翻页；⑤ 无菜单时 `=`/`-` 落标点；
/// ⑥ **负向对照**：`ascii_mode` 打开时判据不成立 ⇒ `-` 不拦截、退回上游标点路径。
#[test]
fn menu_paging_keys_are_not_shadowed_by_the_punctuation_branch() {
    let _guard = serial();
    COMMITS.lock().unwrap().clear();
    UPDATES.lock().unwrap().clear();
    let mut engine = TestEngine::new(host(), fixture_dirs(), None, None);
    for code in *b"ja" {
        engine.key(u32::from(code), 0, false);
    }
    let first_page = last_update().2;
    assert!(first_page.len() >= 2, "夹具 ja 应有可翻页的多页候选");

    // ① 菜单可见按 `=`：下翻一页、不提交（上游会提交「…=」）。
    COMMITS.lock().unwrap().clear();
    let selected_before = last_update().3;
    assert!(engine.key(0x3d, 0, false), "`=` 应被消费（翻页）");
    assert!(
        COMMITS.lock().unwrap().is_empty(),
        "`=` 不得提交组合：翻页绑定优先于标点分支"
    );
    assert_eq!(
        last_update().3,
        selected_before + hux_core::host::DEFAULT_PAGE_SIZE as i32,
        "`=` 应下翻一页（高亮前进一页）"
    );
    assert_eq!(engine.session().context.input(), b"ja", "翻页不改动输入");

    // ② 已翻页后按 `-`：上翻一页、仍不提交。
    assert!(engine.key(0x2d, 0, false), "翻页后 `-` 应被消费（上翻页）");
    assert!(COMMITS.lock().unwrap().is_empty(), "上翻页不得提交组合");
    assert_eq!(last_update().3, selected_before, "`-` 应回到第一页");

    // ③ **首屏**按 `-`：菜单可见即判上翻页（不再要求 `when: paging` 标签）——
    //    先把高亮挪到第 2 项，再按 `-` ⇒ 归零高亮（参照 `PreviousPage` 的三元式）、不提交。
    let mut first_page_engine = TestEngine::new(host(), fixture_dirs(), None, None);
    for code in *b"ja" {
        first_page_engine.key(u32::from(code), 0, false);
    }
    let home_page = last_update().3;
    assert!(first_page_engine.key(0xff54, 0, false), "Down 前进一项");
    assert_eq!(last_update().3, home_page + 1, "Down 移动高亮");
    COMMITS.lock().unwrap().clear();
    assert!(
        first_page_engine.key(0x2d, 0, false),
        "首屏 `-` 应被消费（上翻页）"
    );
    assert!(
        COMMITS.lock().unwrap().is_empty(),
        "首屏 `-` 不得提交组合（上游会提交「…-」）"
    );
    assert_eq!(last_update().3, home_page, "首页上翻归零高亮（留在首页）");
    assert_eq!(
        first_page_engine.session().context.input(),
        b"ja",
        "翻页不改动输入"
    );
    //    同源路径（原场景）：显式 `Page_Up` 停在首页后再按 `-`，同样不得提交。
    COMMITS.lock().unwrap().clear();
    assert!(first_page_engine.key(0xff55, 0, false), "Page_Up 应被消费");
    assert!(COMMITS.lock().unwrap().is_empty(), "Page_Up 不提交");
    assert_eq!(last_update().3, home_page, "首页上翻归零高亮（留在首页）");
    assert!(
        first_page_engine.key(0x2d, 0, false),
        "Page_Up 后 `-` 应翻页"
    );
    assert!(
        COMMITS.lock().unwrap().is_empty(),
        "`-` 不得提交组合（原场景）"
    );
    assert_eq!(
        first_page_engine.session().context.input(),
        b"ja",
        "翻页不改动输入"
    );

    // ④ 显式 `Page_Down` 仍是翻页路径（不受本偏离影响）。
    let mut page_engine = TestEngine::new(host(), fixture_dirs(), None, None);
    for code in *b"ja" {
        page_engine.key(u32::from(code), 0, false);
    }
    let page_start = last_update().3;
    COMMITS.lock().unwrap().clear();
    assert!(page_engine.key(0xff56, 0, false), "Page_Down 应被消费");
    assert!(COMMITS.lock().unwrap().is_empty(), "Page_Down 不提交");
    assert_eq!(
        last_update().3,
        page_start + hux_core::host::DEFAULT_PAGE_SIZE as i32,
        "Page_Down 翻到下一页"
    );

    // ⑤ 无菜单（空闲）时 `=`/`-` 仍落标点：不进任何翻页路径。
    let mut idle_engine = TestEngine::new(host(), fixture_dirs(), None, None);
    COMMITS.lock().unwrap().clear();
    assert!(idle_engine.key(0x3d, 0, false), "空闲 `=` 由标点表消费");
    assert_eq!(COMMITS.lock().unwrap().clone(), vec!["=".to_string()]);
    COMMITS.lock().unwrap().clear();
    assert!(idle_engine.key(0x2d, 0, false), "空闲 `-` 由标点表消费");
    assert_eq!(COMMITS.lock().unwrap().clone(), vec!["-".to_string()]);

    // ⑥ **负向对照（用户决定 B 的另一半）**：`ascii_mode` 打开 ⇒ `menu_available` 不成立，
    //    翻页键一律不拦截，`-` 退回上游标点路径（确认组合 + 落标点）。
    //    平台侧无 `ascii_mode` 设置项（它是宿主/rime 标准选项，真机由 fcitx5 的
    //    V 模式直接写入会话上下文），故与参照探针同为「直接设置会话选项」。
    let mut ascii_engine = TestEngine::new(host(), fixture_dirs(), None, None);
    for code in *b"ja" {
        ascii_engine.key(u32::from(code), 0, false);
    }
    let ascii_sentence = last_update().2.first().cloned().expect("ja 候选");
    let session = ascii_engine.session;
    ascii_engine
        .sessions
        .get_mut(&session)
        .expect("会话")
        .context
        .set_option("ascii_mode", true);
    COMMITS.lock().unwrap().clear();
    assert!(ascii_engine.key(0x2d, 0, false), "`-` 由标点表消费");
    assert_eq!(
        COMMITS.lock().unwrap().clone(),
        vec![ascii_sentence, "-".to_string()],
        "`ascii_mode` 下 `-` 不判为翻页：确认组合 + 落标点"
    );
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
    // 循环体在返回空列表时一次都不执行 ⇒ 角色表整体失效会「空转通过」，
    // 故先钉住长度（与 ABI 角色序同源）。
    let roles = engine.runtime_options().to_vec();
    assert_eq!(
        roles.len(),
        hux_cfg::roles::RUNTIME_OPTION_ROLES.len(),
        "运行时选项表必须与 RUNTIME_OPTION_ROLES 等长：{roles:?}"
    );
    for name in &roles {
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
    // 参照 `abad411`：菜单可见时处理器先确认组合（librime `ConcreteEngine::OnSelect`
    // 在 `_auto_commit` 下同步 `Commit()`），标点随后独立落字；提交文本合计不变。
    assert_eq!(
        COMMITS.lock().unwrap().clone(),
        vec!["甲".to_string(), "，".to_string()]
    );
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

/// `forward_after_commit` 是**粘性输出标志**，未知 / 已释放会话
/// 不得沿用上一次按键的取值。此前 `with_session` 返回 `None` 时直接 `unwrap_or(false)`，
/// 标志保留 ⇒ `hux_engine_key` 只回 `HUX_KEY_FORWARD_AFTER_COMMIT`（无 CONSUMED），
/// 宿主会 `filterAndAccept` + `forwardKey` 一个并不存在的提交。
#[test]
fn unknown_session_does_not_reuse_the_sticky_forward_flag() {
    let _guard = serial();
    let mut engine = TestEngine::new(host(), fixture_dirs(), None, None);
    // 先造出「提交 + 未消费」（组合中按大写字母）：此时转发位为真。
    engine.key(u32::from(b'a'), 0, false);
    engine.key(u32::from(b'b'), 0, false);
    assert!(!engine.key(0x41, FCITX_SHIFT, false));
    assert!(engine.forward_after_commit);
    // 未知会话按键：必须清位（且不消费）。
    let unknown = engine.session + 1000;
    assert!(!engine.engine.key(unknown, u32::from(b'x'), 0, false));
    assert!(
        !engine.engine.forward_after_commit,
        "未知会话按键不得沿用上一次的转发位"
    );
    // 候选点击路径同理（重新置位后再走未知会话）。
    assert!(engine.key(u32::from(b'a'), 0, false));
    assert!(engine.key(u32::from(b'b'), 0, false));
    assert!(!engine.key(0x41, FCITX_SHIFT, false));
    assert!(engine.forward_after_commit);
    assert!(!engine.engine.select_candidate(unknown, 0));
    assert!(
        !engine.engine.forward_after_commit,
        "未知会话的候选点击不得沿用上一次的转发位"
    );
    // 已释放会话同理。
    assert!(engine.key(u32::from(b'a'), 0, false));
    assert!(engine.key(u32::from(b'b'), 0, false));
    assert!(!engine.key(0x41, FCITX_SHIFT, false));
    let released = engine.session;
    engine.engine.session_free(released);
    assert!(!engine.engine.key(released, u32::from(b'x'), 0, false));
    assert!(
        !engine.engine.forward_after_commit,
        "已释放会话按键不得沿用上一次的转发位"
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
        learning_on_tab: false,
        ..Default::default()
    });
    assert!(
        engine.engine.scheme.learning_mode().is_empty(),
        "关闭 Tab 学习 → 学习 mode 为空"
    );
}

/// 高频字上限经**配置页入口**下发即重建词库：引擎先按内建缺省（1500）装配方案，
/// 宿主随后才 `apply_settings`；只在 `load` 时生效等于「设置永不生效」。
///
/// 判据用宿主可见的候选列表（不是词库内部状态）：夹具码表里 `jvn` = 主码 `华` +
/// 高频字 `仍` 的非主码，上限放开后 `仍` 才能参与组句。
#[test]
fn apply_settings_rebuilds_the_lexicon_for_a_new_high_freq_limit() {
    let _guard = serial();
    UPDATES.lock().unwrap().clear();
    // 夹具词库（`goldens/lexicon`：码表 + 字频 + 白名单），自带字频文件才谈得上过滤。
    let dirs = vec![PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../goldens/lexicon")];
    let mut engine = TestEngine::new(host(), dirs, None, Some(temp_user_dir("high-freq")));
    let type_code = |engine: &mut TestEngine, code: &[u8]| {
        for key in code {
            engine.key(u32::from(*key), 0, false);
        }
        let candidates = last_update().2;
        engine.reset();
        candidates
    };
    assert_eq!(
        type_code(&mut engine, b"jvn"),
        vec!["华".to_string()],
        "缺省上限 1500 下 `仍` 的非主码被过滤"
    );
    // 配置页把上限调成 0（不限制）→ 词库必须重建，解码结果随之变化。
    engine.apply_settings(Settings {
        high_freq_limit: 0,
        ..Default::default()
    });
    assert_eq!(
        type_code(&mut engine, b"jvn"),
        vec!["华".to_string(), "仍".to_string()],
        "放开上限后 `仍` 应参与组句"
    );
    // 再收紧回 1500 → 同样重建（不是「只放开一次」）。
    engine.apply_settings(Settings {
        high_freq_limit: 1500,
        ..Default::default()
    });
    assert_eq!(type_code(&mut engine, b"jvn"), vec!["华".to_string()]);
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
        learning_on_tab: 0,
        high_freq_limit: 800,
        // 音反查：`；`（无修饰）与 Shift+`；`（= `:`）。
        reverse_lookup_pronunciation: key_list(&[(0x3b, 0), (0x3a, 0)]),
        // 字反查：Shift+`（= `~`）。
        reverse_lookup_character: key_list(&[(0x60, 1)]),
        page_size: 7,
        // 翻页：`.` 与 `]`。
        page_up: key_list(&[(0x2c, 0)]),
        page_down: key_list(&[(0x2e, 0), (0x5d, 0)]),
        digit_select: 1,
        candidate_layout: 0,
        preedit_mode: 0,
        page_cycle: 0,
        min_retained_input_length: 0,
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

/// FFI 用例的引擎指针：**显式临时用户目录**。
///
/// `hux_engine_new(std::ptr::null())` 会解析真实环境（`XDG_DATA_HOME`/`HOME`）并在
/// `~/.local/share/fcitx5/hux/` 打开（必要时创建）学习库：本机 fcitx5 正在运行时会命中
/// LevelDB 锁，且会污染/创建用户真实数据。此处用同一 `Engine`（`new_with_dirs`）显式注入
/// 临时用户目录，其余 FFI 入口（apply_settings / status / session_new / key / free）照旧覆盖。
fn ffi_engine(user_dir: PathBuf) -> *mut Engine {
    Box::into_raw(Box::new(Engine::new_with_dirs(
        host(),
        fixture_dirs(),
        None,
        Some(user_dir),
    )))
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

/// 音反查端到端：设置 → 前缀识别 → 候选/注释 → 预编辑提示 → 空格上屏。
#[test]
fn reverse_lookup_pronunciation_end_to_end() {
    let _guard = serial();
    COMMITS.lock().unwrap().clear();
    UPDATES.lock().unwrap().clear();
    let dirs = vec![
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../goldens/sound_to_char_shape"),
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../data"),
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
}

/// 字反查：默认 `~` 进入组合（**单字符触发键 ⇒ 给默认可上屏候选**）；
/// 上排 = 光标左侧 1 字拼音、下排 = 虎码，步长 1；改成带修饰的触发键时不给默认候选。
#[test]
fn reverse_lookup_character_end_to_end() {
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

/// 字反查夹具目录。
fn reverse_lookup_character_dirs() -> Vec<PathBuf> {
    vec![
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../goldens/sound_to_char_shape"),
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../data"),
    ]
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

/// 设置缺省不得被当成「用户改动」落盘：否则 `options.yaml` 会把设置值钉死，
/// 之后配置页对这些开关永久失效（参照 `M.options.sync` 的 `live.syncing` 抑制语义）。
#[test]
fn setting_defaults_are_not_persisted_as_user_options() {
    let _guard = serial();
    let dir = temp_user_dir("options-seed");
    let mut engine = TestEngine::new(host(), fixture_dirs(), None, Some(dir.clone()));
    // 首个按键会排空会话初始化时入队的选项事件（原缺陷在此落盘 5 个键）。
    engine.key(u32::from(b'a'), 0, false);
    let path = dir.join(hux_cfg::OPTIONS_FILE);
    assert!(
        !path.exists(),
        "设置缺省不应写入 options.yaml：{:?}",
        std::fs::read_to_string(&path).ok()
    );
    // 配置页改动必须生效（不得被 options.yaml 回滚）。
    engine.apply_settings(Settings {
        full_shape: true,
        ..Default::default()
    });
    assert!(
        engine.session().context.get_option("full_shape"),
        "配置页改动应生效"
    );
    // 状态菜单改动仍须落盘（这是 options.yaml 的唯一来源）。
    assert!(engine.set_option_value("tiger_sentence_early_commit", false));
    let text = std::fs::read_to_string(&path).expect("options.yaml");
    assert!(
        text.contains("tiger_sentence_early_commit: false"),
        "{text}"
    );
    std::fs::remove_dir_all(&dir).ok();
}

/// 无会话时状态菜单切换仍须落盘：原先无会话直接 `return true` 而丢弃改动。
#[test]
fn option_change_without_sessions_persists() {
    let _guard = serial();
    let dir = temp_user_dir("option-no-session");
    let mut engine = Engine::new_with_dirs(host(), fixture_dirs(), None, Some(dir.clone()));
    assert!(engine.sessions.is_empty(), "本用例须无会话");
    assert!(engine.set_option_value("tiger_sentence_early_commit", false));
    let text = std::fs::read_to_string(dir.join(hux_cfg::OPTIONS_FILE)).expect("options.yaml");
    assert!(
        text.contains("tiger_sentence_early_commit: false"),
        "无会话切换应落盘：{text}"
    );
    assert_eq!(
        engine.option_value("tiger_sentence_early_commit"),
        Some(false)
    );
    std::fs::remove_dir_all(&dir).ok();
}

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

/// **运行期**学习库写入失败须进状态串。
///
/// 此前 `learning.error` 只在构造期读一次：打开失败可见，`confirm` 里的 `db.put` 失败
/// （磁盘满 / 库被改成只读 / 锁异常）则完全静默，用户只看到「学习不生效」。
/// LevelDB 的写失败无法在测试里稳定构造，故直接注入错误值再走一次按键路径——
/// 守护的是 `finish → observe_learning_error → refresh_status` 这条接线：
/// 去掉那次调用，本用例即失败。
#[test]
fn learning_write_failure_reaches_the_status_string() {
    let _guard = serial();
    let dir = temp_user_dir("learning-error");
    let mut engine = TestEngine::new(host(), fixture_dirs(), None, Some(dir.clone()));
    engine.key(u32::from(b'a'), 0, false);
    let baseline = engine.engine.status.to_str().unwrap_or("").to_string();
    assert!(
        !baseline.contains("learning database write failed"),
        "初始状态串不含写入错误：{baseline}"
    );
    engine.engine.learning.error = Some("learning database write failed".to_string());
    engine.key(u32::from(b'b'), 0, false);
    let status = engine.engine.status.to_str().unwrap_or("").to_string();
    assert!(
        status.contains("learning: learning database write failed"),
        "运行期落库失败应进状态串：{status}"
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

/// **每个角色都必须被方案声明**：装配处按角色解析选项键，缺一即报错。
#[test]
fn every_configured_role_is_declared_by_the_scheme() {
    let _guard = serial();
    let engine = TestEngine::new(host(), fixture_dirs(), None, None);
    let declarations = engine.engine.scheme.option_declarations();
    // 正例：真实方案的声明齐备，装配处不产生诊断（`options:` 前缀只用于装配/保存错误）。
    let (roles, error) = crate::engine::resolve_option_roles(declarations);
    assert_eq!(error, None, "tiger 的声明应覆盖全部角色");
    for role in hux_cfg::roles::SCHEME_OPTION_ROLES {
        assert!(
            roles.key(role).is_some(),
            "角色 {role} 必须被方案声明（否则状态菜单与持久化静默失效）"
        );
    }
    assert!(
        !engine
            .engine
            .status
            .to_str()
            .unwrap_or("")
            .contains("options:"),
        "装配正常时状态串不含角色诊断"
    );

    // 负例：缺角色 → 报错且不接线（不落到角色名字面量上）。
    let incomplete = [
        OptionDecl {
            role: hux_cfg::roles::ROLE_EARLY_COMMIT,
            key: "scheme_early_commit",
        },
        OptionDecl {
            role: "not_a_configured_role",
            key: "scheme_unknown",
        },
    ];
    let (roles, error) = crate::engine::resolve_option_roles(&incomplete);
    let error = error.expect("缺角色应报错");
    assert!(error.contains("方案未声明角色"), "诊断文案：{error}");
    assert!(error.contains(hux_cfg::roles::ROLE_DIGIT_SELECT));
    assert_eq!(roles.key(hux_cfg::roles::ROLE_EARLY_COMMIT), None);
    assert_eq!(roles.key(hux_cfg::roles::ROLE_FULL_SHAPE), None);
}

/// 角色常量值 ↔ 方案读取的角色：两侧**同值同序**（任一侧改名即失败）。
///
/// 这是角色一致性的核心守护：`Config::parse` 对未知角色 `unwrap_or(0/false)` **静默回退**，
/// 故单侧改名过去可让 `min_retained_raw_length`（→0，不再限制保留量）/ `high_freq_limit`
/// （→0，高频过滤全放开）静默失效而全绿。方案不依赖 `hux-cfg`、配置层不依赖方案，
/// 两侧只能在此（装配根）对齐：清单逐项比对 + 真实装配路径无诊断 + 改名必报诊断。
#[test]
fn scheme_config_roles_match_the_scheme() {
    let _guard = serial();
    assert_eq!(
        hux_cfg::roles::SCHEME_CONFIG_ROLES,
        hux_scheme_tiger::scheme::SCHEME_CONFIG_ROLES,
        "cfg 的配置角色清单必须与方案读取的角色同值同序"
    );
    // 正例：真实装配路径（配置层装袋 → 方案读袋）无角色漂移诊断。
    let engine = TestEngine::new(host(), fixture_dirs(), None, Some(temp_user_dir("roles")));
    let status = engine.engine.status.to_str().unwrap_or("").to_string();
    assert!(
        !status.contains("config:"),
        "角色一致时状态串不应有配置诊断：{status}"
    );

    // 负例：把 `high_freq_limit` 单侧改名（等价于改 `ROLE_HIGH_FREQ_LIMIT` 的值）
    // → 方案点名「未识别的角色 / 缺少角色」，用户侧不再是「设置没生效」的哑失败。
    let mut renamed = hux_core::scheme::SchemeConfig::new();
    for role in hux_cfg::roles::SCHEME_CONFIG_ROLES {
        if *role != hux_cfg::roles::ROLE_HIGH_FREQ_LIMIT {
            renamed = renamed.with(role, hux_core::scheme::Value::Count(1));
        }
    }
    renamed = renamed
        .with(
            "high_freq_limit_X",
            hux_core::scheme::Value::Count(hux_cfg::DEFAULT_HIGH_FREQ_LIMIT),
        )
        // 平台追加的运行时选项角色照常装入（漂移项只有 `high_freq_limit`）。
        .with(
            hux_cfg::roles::ROLE_ALLOW_DUPLICATE_SINGLE,
            hux_core::scheme::Value::Bool(true),
        );
    let (_, notes) = hux_scheme_tiger::scheme::TigerScheme::load(&fixture_dirs(), None, &renamed);
    assert!(
        notes
            .iter()
            .any(|note| note == "config: 未识别的角色 high_freq_limit_X"),
        "改名后应报未识别角色：{notes:?}"
    );
    assert!(
        notes
            .iter()
            .any(|note| note == "config: 缺少角色 high_freq_limit"),
        "改名后应报缺少角色：{notes:?}"
    );
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

    // 重新下发完整配置袋：诊断清空（状态串回到基线）。
    let good = crate::engine::scheme_config(&engine.engine.settings).with(
        hux_cfg::roles::ROLE_ALLOW_DUPLICATE_SINGLE,
        hux_core::scheme::Value::Bool(true),
    );
    engine.engine.apply_scheme_config(good);
    let status = engine.engine.status.to_str().unwrap_or("").to_string();
    assert!(
        !status.contains("config:"),
        "恢复完整配置袋后不应残留诊断：{status}"
    );
}

/// 角色表的分组 / 顺序 / 包含关系：手工清单必须钉在角色表上。
///
/// 此前三条不变式（方案开关 ⊆ 运行时角色、存储缺省 ≡ 运行时角色、会话缺省 ⊇ 运行时角色）
/// 只是「今天恰好成立」——新增一个方案开关角色时不会有任何断言失败。
#[test]
fn runtime_role_tables_cover_the_declared_roles() {
    let _guard = serial();
    let engine = TestEngine::new(host(), fixture_dirs(), None, Some(temp_user_dir("roles2")));
    let roles = &engine.engine.option_roles;
    // 方案声明的角色序 = 配置层的方案开关名单（顺序错位会让状态菜单与开关对不上）。
    assert_eq!(
        engine
            .engine
            .scheme
            .option_declarations()
            .iter()
            .map(|decl| decl.role)
            .collect::<Vec<_>>(),
        hux_cfg::roles::SCHEME_OPTION_ROLES.to_vec(),
        "方案声明顺序必须等于 SCHEME_OPTION_ROLES"
    );
    // 运行时角色表与 ABI 角色数同源（空表也不得让本用例空转）。
    let runtime = engine.engine.runtime_options().to_vec();
    assert_eq!(runtime.len(), hux_cfg::roles::RUNTIME_OPTION_ROLES.len());
    let settings = Settings::default();
    let store = settings.store_defaults(roles);
    let defaults = settings.option_defaults(roles);
    assert_eq!(store.len(), hux_cfg::roles::RUNTIME_OPTION_ROLES.len());
    for role in hux_cfg::roles::RUNTIME_OPTION_ROLES {
        let key = roles
            .key(role)
            .unwrap_or_else(|| panic!("角色 {role} 应有键"));
        assert!(
            store.contains_key(key),
            "运行时角色 {role} 必须可持久化（否则菜单能切但不落盘）"
        );
        assert!(
            defaults.iter().any(|(name, _)| *name == key),
            "运行时角色 {role} 必须有会话缺省（否则新会话回退到别处）"
        );
    }
    // 宿主标准项 `ascii_punct` 只作会话初始选项，不入运行时菜单、不落盘。
    assert!(
        defaults
            .iter()
            .any(|(name, _)| *name == hux_cfg::roles::ROLE_ASCII_PUNCT)
    );
    assert!(!store.contains_key(hux_cfg::roles::ROLE_ASCII_PUNCT));
}

/// `hux_abi.h` 的 `HUX_OPTION_*` 枚举序 ↔ `hux_cfg::roles::RUNTIME_OPTION_ROLES`（顺序 / 个数 / 名字）。
///
/// 角色序在三处手工同步（角色表、头文件枚举、C++ 文案表 `kLabels[role]`）：
/// **调序**会让菜单文案与开关静默错位、`HUX_OPTION_DIGIT_SELECT` 取到别的选项键。
/// C++ 侧只能守长度（`static_assert(std::size(kLabels) == HUX_OPTION_COUNT)`，见 `shell/hux.cpp`），
/// 顺序由本用例从**头文件源码**解析后逐项比对——改名 / 加角色 / 调序都在此失败。
#[test]
fn option_role_order_matches_the_abi_header() {
    let header = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../crates/hux-ffi/include/hux_abi.h"
    ))
    .expect("read hux_abi.h");
    // 解析 `enum { HUX_OPTION_X = n, … };` 的成员与被显式写出的下标（含末尾计数哨兵）。
    let body = header
        .split_once("enum {")
        .expect("HUX_OPTION_* 枚举定义")
        .1;
    let body = body.split_once("};").expect("枚举结束").0;
    let members: Vec<(&str, i32)> = body
        .lines()
        .filter_map(|line| {
            let (name, value) = line.trim().split_once('=')?;
            let name = name.trim();
            if !name.starts_with("HUX_OPTION_") {
                return None;
            }
            Some((
                name,
                value
                    .trim()
                    .trim_end_matches(',')
                    .parse::<i32>()
                    .expect("枚举下标应为整数"),
            ))
        })
        .collect();

    let expected: Vec<String> = hux_cfg::roles::RUNTIME_OPTION_ROLES
        .iter()
        .map(|role| format!("HUX_OPTION_{}", role.to_ascii_uppercase()))
        .collect();
    assert_eq!(
        members.len(),
        expected.len() + 1,
        "枚举 = 角色序 + 计数哨兵（实际：{members:?}）"
    );
    assert_eq!(
        members[..expected.len()]
            .iter()
            .map(|(name, _)| *name)
            .collect::<Vec<_>>(),
        expected.iter().map(String::as_str).collect::<Vec<_>>(),
        "hux_abi.h 的角色序（顺序 / 个数 / 名字）必须等于 RUNTIME_OPTION_ROLES"
    );
    // 下标连续 0..=n：重复 / 跳号会让宿主按角色取到错位的键。
    assert_eq!(
        members.iter().map(|(_, value)| *value).collect::<Vec<_>>(),
        (0..=expected.len() as i32).collect::<Vec<_>>()
    );
    let (sentinel, count) = members[expected.len()];
    assert_eq!(sentinel, "HUX_OPTION_COUNT");
    assert_eq!(count as usize, hux_cfg::roles::RUNTIME_OPTION_ROLES.len());
}

/// 设置 → 角色袋必须覆盖 `hux-cfg` 声明的**全部**配置角色（漏一个即失败）。
#[test]
fn scheme_config_covers_every_declared_role() {
    let bag = crate::engine::scheme_config(&Settings::default());
    assert_eq!(
        bag.roles().collect::<Vec<_>>(),
        hux_cfg::roles::SCHEME_CONFIG_ROLES.to_vec(),
        "配置袋的角色与顺序即 `hux-cfg` 的角色全集"
    );
    let settings = Settings::default();
    assert_eq!(
        bag.count(hux_cfg::roles::ROLE_HIGH_FREQ_LIMIT),
        Some(settings.high_freq_limit)
    );
    assert_eq!(
        bag.count(hux_cfg::roles::ROLE_MIN_RETAINED_INPUT_LENGTH),
        Some(settings.min_retained())
    );
    assert_eq!(
        bag.texts(hux_cfg::roles::ROLE_REVERSE_LOOKUP_PRONUNCIATION_KEYS),
        Some(settings.reverse_lookup_pronunciation_keys.as_slice())
    );
    assert_eq!(
        bag.bool(hux_cfg::roles::ROLE_LEARNING_ON_TAB),
        Some(settings.learning_on_tab)
    );
    // 钳制在装配处生效（页大小 / 最短保留码数上限）。
    let clamped = crate::engine::scheme_config(&Settings {
        page_size: 999,
        min_retained_input_length: 999,
        ..Default::default()
    });
    assert_eq!(
        clamped.count(hux_cfg::roles::ROLE_PAGE_SIZE),
        Some(hux_core::host::MAX_PAGE_SIZE)
    );
    assert_eq!(
        clamped.count(hux_cfg::roles::ROLE_MIN_RETAINED_INPUT_LENGTH),
        Some(hux_cfg::MAX_MIN_RETAINED_INPUT_LENGTH)
    );
}

/// 运行时选项值经配置袋下发到方案：单字重码开关决定学习 mode 的 `dup` 位。
#[test]
fn runtime_option_value_reaches_scheme_learning_mode() {
    let _guard = serial();
    let dir = temp_user_dir("duplicate-mode");
    let mut engine = TestEngine::new(host(), fixture_dirs(), None, Some(dir.clone()));
    // mode 串对平台不透明 ⇒ 只断言「随配置变化而变化」与「重启后一致」，
    // 具体格式（`dup=1` / `dup=0` 的映射）由方案自己的用例钉住
    // （`tiger` 的 `learning_mode_follows_config_and_rules`）。
    let default_mode = engine.engine.scheme.learning_mode().to_string();
    assert!(!default_mode.is_empty(), "学习开启时 mode 串非空");
    assert!(engine.set_option_value("tiger_sentence_allow_duplicate_single", false));
    let disabled_mode = engine.engine.scheme.learning_mode().to_string();
    assert_ne!(
        disabled_mode, default_mode,
        "运行时关掉单字重码后方案自算的 mode 串必须随之变化"
    );
    // 持久化值经「存储 → 会话 → 配置袋」在按键路径生效（重启后同样）。
    let mut restarted = TestEngine::new(host(), fixture_dirs(), None, Some(dir.clone()));
    restarted.key(u32::from(b'a'), 0, false);
    assert_eq!(
        restarted.engine.scheme.learning_mode(),
        disabled_mode,
        "options.yaml 的值优先于设置缺省（重启后 mode 与关掉时一致）"
    );
    std::fs::remove_dir_all(&dir).ok();
}

/// 选项角色 → 键：宿主（C++）据此构造状态菜单与面板序号，不再硬编码方案选项名。
#[test]
fn option_role_keys_follow_scheme_declarations() {
    let _guard = serial();
    let engine = TestEngine::new(host(), fixture_dirs(), None, None);
    assert_eq!(
        hux_engine_option_role_count(),
        hux_cfg::roles::RUNTIME_OPTION_ROLES.len() as i32
    );
    // 角色序（ABI 角色序）与**已解析**的键表逐项一致：菜单项与 ABI 索引不会错位
    // （宿主标准项 `full_shape` 由配置层自持，不在方案声明里）。
    assert_eq!(
        hux_cfg::roles::RUNTIME_OPTION_ROLES
            .iter()
            .map(|role| engine.engine.option_roles.key(role))
            .collect::<Vec<_>>(),
        vec![
            Some("tiger_sentence_early_commit"),
            Some("tiger_sentence_early_commit_to_preedit"),
            Some("tiger_sentence_allow_duplicate_single"),
            Some("full_shape"),
            Some("tiger_sentence_digit_select"),
        ],
        "角色序（含宿主标准项 full_shape）↔ 方案声明的键"
    );
    let keys: Vec<String> = (0..5)
        .map(|role| unsafe {
            let key = hux_engine_option_key(&engine.engine, role);
            assert!(!key.is_null(), "角色 {role} 应有选项键");
            std::ffi::CStr::from_ptr(key).to_string_lossy().into_owned()
        })
        .collect();
    assert_eq!(
        keys,
        vec![
            "tiger_sentence_early_commit",
            "tiger_sentence_early_commit_to_preedit",
            "tiger_sentence_allow_duplicate_single",
            "full_shape",
            "tiger_sentence_digit_select",
        ]
    );
    // 角色序与状态菜单白名单同源（改方案时两者一起变）。
    assert_eq!(engine.engine.runtime_options().to_vec(), keys);
    // 越界与空指针安全。
    assert!(unsafe { hux_engine_option_key(&engine.engine, 5) }.is_null());
    assert!(unsafe { hux_engine_option_key(&engine.engine, -1) }.is_null());
    assert!(unsafe { hux_engine_option_key(std::ptr::null(), 0) }.is_null());
}

/// 取 C++ 配置 schema（`shell/hux.cpp`）里 `.path{"<name>"}` 之后的 `.defaultValue` 字面量。
///
/// C++ 侧的默认值不参与 cargo 测试（`hux.cpp` 由 cmake 单独编译），改错了两侧都编译得过；
/// 这里以「解析源码」把它变成可断言的字面量（剥掉行注释；`KeyList` 的默认值跨多行，
/// 按花括号配平补齐）。
fn schema_default(name: &str) -> String {
    let source = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/shell/hux.cpp"))
        .expect("read hux.cpp");
    let lines: Vec<String> = source
        .lines()
        .map(|line| {
            line.trim()
                .split("//")
                .next()
                .unwrap_or("")
                .trim_end()
                .to_string()
        })
        .collect();
    for (index, line) in lines.iter().enumerate() {
        let Some(rest) = line.strip_prefix(".path{") else {
            continue;
        };
        if rest.trim_end_matches("},").trim_matches('"') != name {
            continue;
        }
        for (offset, next) in lines[index + 1..].iter().enumerate() {
            if next.starts_with(".path{") {
                break;
            }
            let Some(rest) = next.strip_prefix(".defaultValue = ") else {
                continue;
            };
            let mut value = rest.trim_end_matches(',').to_string();
            let mut open = value.matches('{').count() as i32 - value.matches('}').count() as i32;
            let mut cursor = index + offset + 2;
            while open > 0 && cursor < lines.len() {
                let more = lines[cursor].trim_end_matches(',');
                value.push_str(more);
                open += more.matches('{').count() as i32 - more.matches('}').count() as i32;
                cursor += 1;
            }
            return value;
        }
        panic!("schema 项 {name} 没有 defaultValue");
    }
    panic!("schema 缺少项 {name}");
}

/// 「候选窗口显示预编辑」是宿主显示项（不进引擎 `Settings`，故
/// `schema_defaults_match_settings_defaults` 明确跳过它）：默认值在这里单独钉住——
/// 改回「关」不会让任何编译或其它测试失败，而用户侧就是「预编辑又没了」。
#[test]
fn host_schema_panel_preedit_defaults_to_on() {
    assert_eq!(schema_default("PanelPreedit"), "true");
}

/// 默认反查触发键：音反查 `` ` ``（`grave`）、字反查 `~`（`asciitilde`），且都**无修饰**。
///
/// `~` 在物理键盘上是 Shift+`` ` ``，但前端上报的是该 level 的 keysym（`asciitilde`+Shift），
/// 而 fcitx5 `Key::normalize()` 会去掉这类「本身就产字符」键的 Shift（旧默认 `Alt+:` 同理：
/// `:` = Shift+`;` 归一化成 `colon`+Alt）⇒ 引擎收到的是 `asciitilde` + 无修饰。
/// 断言按语义给（含 keysym 且无修饰），不钉源码的书写形式。
#[test]
fn host_schema_reverse_lookup_defaults_are_grave_and_asciitilde() {
    let pronunciation = schema_default("SoundToCharShapeKey");
    assert!(
        pronunciation.contains("FcitxKey_grave") && pronunciation.contains("KeyState::NoState"),
        "音反查默认键应为无修饰的 `（grave）：{pronunciation}"
    );
    assert!(
        !pronunciation.contains("KeyState::Alt"),
        "音反查默认键不应带修饰：{pronunciation}"
    );
    let character = schema_default("CharToSoundShapeKey");
    assert!(
        character.contains("FcitxKey_asciitilde") && character.contains("KeyState::NoState"),
        "字反查默认键应为无修饰的 ~（asciitilde）：{character}"
    );
    assert!(
        !character.contains("KeyState::Alt"),
        "字反查默认键不应带修饰：{character}"
    );
}

/// C++ 配置 schema（`shell/hux.cpp`）的默认值必须与 `hux-cfg::Settings::default()` 一致。
///
/// 两边各写一份默认值且此前无任何校验：C++ 构造时即 `applyConfig` 覆盖引擎侧默认，
/// 故 Rust 侧漂移不会被发现。此处以「解析 C++ 源 ↔ 逐项比对」把它变成 CI 不变量。
#[test]
fn schema_defaults_match_settings_defaults() {
    let source = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/shell/hux.cpp"))
        .expect("read hux.cpp");

    // 解析 `.path{"Name"}` 与其后的 `.defaultValue = <value>,`（KeyList 可能跨多行）。
    let mut defaults: Vec<(String, String)> = Vec::new();
    let mut pending: Option<String> = None;
    let mut lines = source.lines().map(str::trim).peekable();
    while let Some(line) = lines.next() {
        if let Some(rest) = line.strip_prefix(".path{") {
            pending = Some(rest.trim_end_matches("},").trim_matches('"').to_string());
            continue;
        }
        let Some(rest) = line.strip_prefix(".defaultValue = ") else {
            continue;
        };
        let Some(name) = pending.take() else { continue };
        let mut value = rest.trim_end_matches(',').to_string();
        // KeyList 跨行：补齐花括号直到配平。
        let mut open = value.matches('{').count() as i32 - value.matches('}').count() as i32;
        while open > 0 {
            let next = lines.next().expect("unterminated defaultValue");
            value.push_str(next.trim_end_matches(','));
            open += next.matches('{').count() as i32 - next.matches('}').count() as i32;
        }
        defaults.push((name, value));
    }

    // C++ 用 keysym 常量声明默认键：`fcitx::Key(FcitxKey_grave, fcitx::KeyState::NoState)`
    // → rime 键名 `grave`（`FcitxKey_<name>` 即 X11 键名，与 librime 键名表同名）；
    // 带 `KeyState::Alt` 的项加 `Alt+` 前缀（旧默认形态）。
    let keys = |raw: &str| -> Vec<String> {
        raw.split("fcitx::Key(FcitxKey_")
            .skip(1)
            .filter_map(|part| {
                let name: String = part
                    .chars()
                    .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                    .collect();
                if name.is_empty() {
                    return None;
                }
                Some(if part.contains("KeyState::Alt") {
                    format!("Alt+{name}")
                } else {
                    name
                })
            })
            .collect()
    };
    let enum_tail = |raw: &str| raw.rsplit("::").next().unwrap_or(raw).to_string();

    let settings = Settings::default();
    let mut checked = 0usize;
    for (name, raw) in &defaults {
        let expected: String = match name.as_str() {
            "EarlyCommit" => settings.early_commit.to_string(),
            "EarlyCommitToPreedit" => settings.early_commit_to_preedit.to_string(),
            "AllowDuplicateSingle" => settings.allow_duplicate_single.to_string(),
            "FullShape" => settings.full_shape.to_string(),
            "AsciiPunct" => settings.ascii_punct.to_string(),
            "TabLearning" => settings.learning_on_tab.to_string(),
            "DigitSelect" => settings.digit_select.to_string(),
            "PageCycle" => settings.page_cycle.to_string(),
            "HighFreqLimit" => settings.high_freq_limit.to_string(),
            "PageSize" => settings.page_size.to_string(),
            "MinRetainedRawLength" => settings.min_retained_input_length.to_string(),
            "CandidateLayout" => format!("{:?}", settings.candidate_layout),
            "PreeditMode" => format!("{:?}", settings.preedit_mode),
            "PageUpKey" => settings.page_up_keys.join(","),
            "PageDownKey" => settings.page_down_keys.join(","),
            "SoundToCharShapeKey" => settings.reverse_lookup_pronunciation_keys.join(","),
            "CharToSoundShapeKey" => settings.reverse_lookup_character_keys.join(","),
            _ => continue, // PanelPreedit 等宿主显示项不经引擎
        };
        let actual = if raw.contains("fcitx::KeyList") {
            keys(raw).join(",")
        } else if expected.chars().all(|c| c.is_ascii_digit())
            || matches!(expected.as_str(), "true" | "false")
        {
            raw.clone()
        } else {
            enum_tail(raw)
        };
        assert_eq!(actual, expected, "schema 默认值与 Settings 不一致：{name}");
        checked += 1;
    }
    assert_eq!(checked, 17, "应逐项核对 17 个引擎设置");
}

/// 反向守护：上面那条测试只保证「schema 里出现的项与 `Settings` 一致」，
/// 是**单向**的——新增一个 `Settings` 字段而不写进 `shell/hux.cpp` 的 schema 不会失败。
/// 本测试补上另一向：`Settings` 的每个字段都必须在 schema 中声明，反之 schema 里除
/// `HOST_ONLY_PATHS`（只服务宿主显示、不经引擎的项）外不得出现引擎不认识的路径。
///
/// 表内每项都用 `offset_of!` 引用真实字段名 ⇒ **改名字段即编译失败**；`FIELDS.len()` 被钉住
/// ⇒ 新增字段必须同步本表与配置页（否则此测试先红）。这正是本表要堵的漂移入口。
#[test]
fn every_settings_field_is_declared_in_the_schema() {
    // （字段名，schema 路径名，偏移）。顺序 = `Settings` 声明序。
    const FIELDS: &[(&str, &str, usize)] = &[
        (
            "early_commit",
            "EarlyCommit",
            std::mem::offset_of!(Settings, early_commit),
        ),
        (
            "early_commit_to_preedit",
            "EarlyCommitToPreedit",
            std::mem::offset_of!(Settings, early_commit_to_preedit),
        ),
        (
            "allow_duplicate_single",
            "AllowDuplicateSingle",
            std::mem::offset_of!(Settings, allow_duplicate_single),
        ),
        (
            "full_shape",
            "FullShape",
            std::mem::offset_of!(Settings, full_shape),
        ),
        (
            "ascii_punct",
            "AsciiPunct",
            std::mem::offset_of!(Settings, ascii_punct),
        ),
        (
            "learning_on_tab",
            "TabLearning",
            std::mem::offset_of!(Settings, learning_on_tab),
        ),
        (
            "digit_select",
            "DigitSelect",
            std::mem::offset_of!(Settings, digit_select),
        ),
        (
            "page_cycle",
            "PageCycle",
            std::mem::offset_of!(Settings, page_cycle),
        ),
        (
            "high_freq_limit",
            "HighFreqLimit",
            std::mem::offset_of!(Settings, high_freq_limit),
        ),
        (
            "page_size",
            "PageSize",
            std::mem::offset_of!(Settings, page_size),
        ),
        (
            "min_retained_input_length",
            "MinRetainedRawLength",
            std::mem::offset_of!(Settings, min_retained_input_length),
        ),
        (
            "candidate_layout",
            "CandidateLayout",
            std::mem::offset_of!(Settings, candidate_layout),
        ),
        (
            "preedit_mode",
            "PreeditMode",
            std::mem::offset_of!(Settings, preedit_mode),
        ),
        (
            "page_up_keys",
            "PageUpKey",
            std::mem::offset_of!(Settings, page_up_keys),
        ),
        (
            "page_down_keys",
            "PageDownKey",
            std::mem::offset_of!(Settings, page_down_keys),
        ),
        (
            "reverse_lookup_pronunciation_keys",
            "SoundToCharShapeKey",
            std::mem::offset_of!(Settings, reverse_lookup_pronunciation_keys),
        ),
        (
            "reverse_lookup_character_keys",
            "CharToSoundShapeKey",
            std::mem::offset_of!(Settings, reverse_lookup_character_keys),
        ),
    ];

    // 只服务宿主显示、不经引擎的 schema 项（与上一条测试的 `_ => continue` 一致）。
    const HOST_ONLY_PATHS: &[&str] = &["PanelPreedit"];

    assert_eq!(
        FIELDS.len(),
        17,
        "Settings 字段数变化：新增/删除字段必须同步本表与 shell/hux.cpp 的 schema（或将新增项登记为宿主显示项）"
    );
    // 字段顺序由 `repr(Rust)` 决定（编译器会重排），故**不假设**「声明序 == 偏移序」；
    // 只要求偏移互异且落在结构体内——重复登记或张冠李戴都会被抓住。
    let mut offsets: Vec<usize> = FIELDS.iter().map(|(_, _, offset)| *offset).collect();
    let size = std::mem::size_of::<Settings>();
    assert!(
        offsets.iter().all(|offset| *offset < size),
        "FIELDS 中有偏移越界项（size_of::<Settings>() = {size}）"
    );
    offsets.sort_unstable();
    offsets.dedup();
    assert_eq!(offsets.len(), FIELDS.len(), "FIELDS 中两个字段指向同一偏移");

    let source = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/shell/hux.cpp"))
        .expect("read hux.cpp");
    let mut declared: Vec<&str> = source
        .split(".path{\"")
        .skip(1)
        .filter_map(|part| part.split('"').next())
        .collect();
    declared.sort_unstable();
    declared.dedup();

    for (field, path, _) in FIELDS {
        assert!(
            declared.contains(path),
            "Settings::{field} 未在 shell/hux.cpp 的 schema 中声明（新增字段须同步配置页，\
             或在 HOST_ONLY_PATHS 登记为宿主显示项）"
        );
    }
    let mut engine_paths: Vec<&str> = declared
        .iter()
        .copied()
        .filter(|path| !HOST_ONLY_PATHS.contains(path))
        .collect();
    engine_paths.sort_unstable();
    let mut expected: Vec<&str> = FIELDS.iter().map(|(_, path, _)| *path).collect();
    expected.sort_unstable();
    assert_eq!(
        engine_paths, expected,
        "schema 路径集合与 Settings 字段表不一致（双向守护：两侧都必须有对方）"
    );
}

/// 模型摘要（`hux_engine_model_info`）的三种状态 + 空指针：已装载（三阶夹具）/
/// 未找到 / 装载失败（非模型文件）；摘要由方案侧结构化产出，平台只搬运。
#[test]
fn model_info_reports_file_format_and_state() {
    let _guard = serial();
    let goldens = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../goldens");
    let read = |engine: *const Engine| -> String {
        let info = unsafe { hux_engine_model_info(engine) };
        assert!(!info.is_null(), "引擎存活期内摘要指针不应为空");
        unsafe { std::ffi::CStr::from_ptr(info) }
            .to_string_lossy()
            .into_owned()
    };

    // 已装载：三阶夹具（文件名 + 格式标签都来自模型自身）。
    let engine = Box::into_raw(Box::new(Engine::new_with_dirs(
        host(),
        fixture_dirs(),
        Some(goldens.join("ngram_fixture.bin")),
        Some(temp_user_dir("model-info-loaded")),
    )));
    assert_eq!(read(engine), "ngram_fixture.bin — 已加载（三阶 TCSKNM02）");
    unsafe { hux_engine_free(engine) };

    // 未找到：数据目录里没有模型资产。
    let empty_dir = hux_test_support::temp_dir("model-info-empty");
    let engine = Box::into_raw(Box::new(Engine::new_with_dirs(
        host(),
        vec![empty_dir.clone()],
        None,
        Some(temp_user_dir("model-info-none")),
    )));
    assert_eq!(read(engine), "未找到模型（整句排序退化为码表名次）");
    unsafe { hux_engine_free(engine) };
    std::fs::remove_dir_all(&empty_dir).ok();

    // 装载失败：错误原文来自装载器（平台不拼、不解析）。
    let engine = Box::into_raw(Box::new(Engine::new_with_dirs(
        host(),
        fixture_dirs(),
        Some(goldens.join("lexicon/tiger_sentence.codes.txt")),
        Some(temp_user_dir("model-info-failed")),
    )));
    let failed = read(engine);
    assert!(
        failed.starts_with("tiger_sentence.codes.txt — 装载失败："),
        "{failed}"
    );
    assert!(
        failed.contains("TCSKNM02"),
        "失败原因应说明期望的模型格式：{failed}"
    );
    unsafe { hux_engine_free(engine) };

    // 空指针 ⇒ NULL（宿主据此早退）。
    assert!(unsafe { hux_engine_model_info(std::ptr::null()) }.is_null());
}

/// 模型路径来源：默认查找（`Auto`）按数据目录解析，「重新部署」据此拿到新装入的模型；
/// 显式路径（`Fixed`）不受目录内容影响。
#[test]
fn model_source_resolves_by_source() {
    let dir = hux_test_support::temp_dir("model-source-auto");
    let auto = crate::engine::ModelSource::Auto;
    assert_eq!(
        auto.resolve(std::slice::from_ref(&dir)),
        None,
        "空目录里没有模型资产"
    );
    let model = dir.join("models/sentence-ngram-mobile.bin");
    std::fs::create_dir_all(model.parent().expect("parent")).expect("mkdir");
    std::fs::write(&model, b"TCSKNM02").expect("write");
    assert_eq!(
        auto.resolve(std::slice::from_ref(&dir)),
        Some(model.clone())
    );
    let fixed = crate::engine::ModelSource::Fixed(dir.join("fixed.bin"));
    assert_eq!(
        fixed.resolve(std::slice::from_ref(&dir)),
        Some(dir.join("fixed.bin"))
    );
    std::fs::remove_dir_all(&dir).ok();
}

/// 重新部署（`hux_engine_redeploy`）：返回 1、既有会话 id 继续可用但状态被重置、
/// 模型摘要随重新装载刷新；引擎为空指针返回 0。
#[test]
fn redeploy_refreshes_model_info_and_resets_sessions() {
    let _guard = serial();
    let goldens = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../goldens");
    let dir = hux_test_support::temp_dir("redeploy-model");
    std::fs::create_dir_all(&dir).expect("mkdir");
    // 指向一个尚不存在的模型：先「装载失败」，装入文件后再重新部署应变成「已加载」。
    let model = dir.join("sentence-ngram-mobile.bin");
    let engine = Box::into_raw(Box::new(Engine::new_with_dirs(
        host(),
        fixture_dirs(),
        Some(model.clone()),
        Some(temp_user_dir("redeploy-user")),
    )));
    let read = || {
        let info = unsafe { hux_engine_model_info(engine) };
        assert!(!info.is_null());
        unsafe { std::ffi::CStr::from_ptr(info) }
            .to_string_lossy()
            .into_owned()
    };
    let failed = read();
    assert!(
        failed.starts_with("sentence-ngram-mobile.bin — 装载失败："),
        "{failed}"
    );

    // 建一个会话并留下组合状态：重新部署后 id 必须仍然有效、组合必须被清空。
    let session = unsafe { hux_engine_session_new(engine) };
    assert!(session > 0);
    assert_ne!(
        unsafe { hux_engine_key(engine, session, u32::from(b'a'), 0, 0) } & HUX_KEY_CONSUMED,
        0,
        "夹具码表里 a 应被消费（组合已开始）"
    );
    assert_eq!(unsafe { &*engine }.sessions[&session].context.input(), b"a");

    // 「装好数据再重新部署」：模型文件就位 → 摘要刷新成已加载。
    std::fs::copy(goldens.join("ngram_fixture.bin"), &model).expect("copy");
    assert_eq!(unsafe { hux_engine_redeploy(engine) }, 1);
    assert_eq!(
        read(),
        "sentence-ngram-mobile.bin — 已加载（三阶 TCSKNM02）"
    );

    // 会话 id 仍可用（重置而非释放）；未知 id 仍被忽略。
    let state = unsafe { &*engine };
    assert!(state.sessions.contains_key(&session));
    assert_eq!(
        state.sessions[&session].context.input(),
        b"",
        "重新部署应清空组合"
    );
    assert_ne!(
        unsafe { hux_engine_key(engine, session, u32::from(b'a'), 0, 0) } & HUX_KEY_CONSUMED,
        0
    );
    assert_eq!(
        unsafe { hux_engine_key(engine, session + 100, u32::from(b'a'), 0, 0) },
        0
    );
    unsafe { hux_engine_session_free(engine, session) };
    unsafe { hux_engine_free(engine) };
    std::fs::remove_dir_all(&dir).ok();

    // 空指针：返回 0（宿主据此报错，而不是假装成功）。
    assert_eq!(unsafe { hux_engine_redeploy(std::ptr::null_mut()) }, 0);
}
