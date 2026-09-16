//! 虎整句 fcitx5 addon 的 Rust 侧（K3）：C ABI、数据加载与会话装配。
//!
//! 分工：`shell/tigerclaw.cpp` 只做 fcitx5 接口适配（按键 → 本层；提交/preedit/候选 ← 本层回调），
//! 逻辑在 Rust（本层 → `tigerclaw-core`）。组合重建照 2c 重放桩同构规则：
//! 提交（翻译失效）或输入变化时重建，否则保留段状态（含菜单高亮）。
//!
//! K3b 范围：每引擎单会话（`activate/reset` 清空）；数据目录见 [`data_dirs`]。

use std::ffi::{CString, c_char, c_void};
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use tigerclaw_core::decode::Decoder;
use tigerclaw_core::interaction::{
    CompositionBuilder, LiveLearning, ProcessorEnv, ProcessorResult, SentenceState, buffered_text,
    option_defaults, processor,
};
use tigerclaw_core::key::{
    K_ALT_MASK, K_CONTROL_MASK, K_LOCK_MASK, K_RELEASE_MASK, K_SHIFT_MASK, K_SUPER_MASK, KeyEvent,
};
use tigerclaw_core::lexical;
use tigerclaw_core::lexicon::{
    LEXICAL_FILE, Lexicon, MODEL_PATH, Supplement, candidate_paths, data_directories,
};
use tigerclaw_core::ngram::MobileModel;
use tigerclaw_core::session::{Context, Event};

// fcitx5 `KeyState` 位（`fcitx-utils/keysym.h`）。
const FCITX_SHIFT: u32 = 1 << 0;
const FCITX_CAPS_LOCK: u32 = 1 << 1;
const FCITX_CTRL: u32 = 1 << 2;
const FCITX_ALT: u32 = 1 << 3;
const FCITX_SUPER: u32 = 1 << 6;

/// fcitx5 `KeyState` → core（Rime）掩码。
fn core_modifiers(states: u32, release: bool) -> i32 {
    let mut modifiers = 0;
    if states & FCITX_SHIFT != 0 {
        modifiers |= K_SHIFT_MASK;
    }
    if states & FCITX_CAPS_LOCK != 0 {
        modifiers |= K_LOCK_MASK;
    }
    if states & FCITX_CTRL != 0 {
        modifiers |= K_CONTROL_MASK;
    }
    if states & FCITX_ALT != 0 {
        modifiers |= K_ALT_MASK;
    }
    if states & FCITX_SUPER != 0 {
        modifiers |= K_SUPER_MASK;
    }
    if release {
        modifiers |= K_RELEASE_MASK;
    }
    modifiers
}

/// 宿主回调表（由 C++ 薄壳提供；函数指针可为空，便于测试）。
#[derive(Clone, Copy)]
#[repr(C)]
pub struct HostCallback {
    pub user: *mut c_void,
    pub commit: Option<unsafe extern "C" fn(*mut c_void, *const c_char)>,
    #[allow(clippy::type_complexity)]
    pub update: Option<
        unsafe extern "C" fn(
            *mut c_void,
            *const c_char,
            i32,
            *const *const c_char,
            *const *const c_char,
            i32,
            i32,
        ),
    >,
}

/// 数据目录：`TIGERCLAW_DATA_DIRS`（冒号分隔，开发用）或用户/共享标准目录。
pub fn data_dirs() -> Vec<PathBuf> {
    if let Ok(value) = std::env::var("TIGERCLAW_DATA_DIRS") {
        let dirs: Vec<PathBuf> = value
            .split(':')
            .filter(|part| !part.is_empty())
            .map(PathBuf::from)
            .collect();
        if !dirs.is_empty() {
            return dirs;
        }
    }
    data_directories()
}

fn default_model_path(dirs: &[PathBuf]) -> Option<PathBuf> {
    candidate_paths(dirs, MODEL_PATH)
        .into_iter()
        .find(|path| path.is_file())
}

fn wall_clock() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|value| value.as_secs_f64())
        .unwrap_or(0.0)
}

/// 引擎：数据 + 单会话（`Context`/`SentenceState`/学习暂存）。
pub struct Engine {
    host: Option<HostCallback>,
    decoder: Decoder,
    context: Context,
    state: SentenceState,
    live: LiveLearning,
    dot_armed: bool,
    min_retained: Option<i64>,
    /// 组合重建（提交或输入变化时重建，保留段状态含菜单高亮）。
    builder: CompositionBuilder,
    status: CString,
}

impl Engine {
    fn new(host: Option<HostCallback>) -> Self {
        let dirs = data_dirs();
        let model = std::env::var_os("TIGERCLAW_MODEL")
            .map(PathBuf::from)
            .or_else(|| default_model_path(&dirs));
        Self::new_with_dirs(host, dirs, model)
    }

    fn new_with_dirs(
        host: Option<HostCallback>,
        dirs: Vec<PathBuf>,
        model_path: Option<PathBuf>,
    ) -> Self {
        let mut notes = vec![format!(
            "dirs: {}",
            dirs.iter()
                .map(|dir| dir.display().to_string())
                .collect::<Vec<_>>()
                .join(":")
        )];
        let lexicon = Lexicon::load(&dirs, 1500);
        notes.push(format!("lexicon: {}", lexicon.data_status().canonical()));
        let supplement = Supplement::load_default(dirs.first().map(PathBuf::as_path));
        let model = model_path.and_then(|path| match MobileModel::load(&path, None) {
            Ok(model) => Some(model),
            Err(error) => {
                notes.push(format!("model: {error}"));
                None
            }
        });
        let mut decoder = Decoder::new(lexicon, supplement, model);
        let lexical_paths = candidate_paths(&dirs, LEXICAL_FILE);
        let (lexical_model, lexical_error) = lexical::load_first(&lexical_paths);
        decoder.set_lexical_model(lexical_model);
        if let Some(error) = lexical_error {
            notes.push(format!("lexical: {error}"));
        }
        let mut context = Context::new();
        // 宿主缺省：`_auto_commit`（librime `express_editor` 默认 true）+ 核心选项缺省。
        context.set_option("_auto_commit", true);
        for (name, value) in option_defaults() {
            context.set_option(&name, value);
        }
        let engine = Self {
            host,
            decoder,
            context,
            state: SentenceState::fresh(1),
            live: LiveLearning::default(),
            dot_armed: false,
            min_retained: None,
            builder: CompositionBuilder::default(),
            status: CString::new(notes.join("; ")).unwrap_or_default(),
        };
        engine.push_update();
        engine
    }

    /// 处理一次按键：返回是否消费；副作用（提交/preedit/候选）经宿主回调送出。
    fn key(&mut self, keysym: u32, states: u32, release: bool) -> bool {
        let key = KeyEvent::new(keysym as i32, core_modifiers(states, release));
        let mut env = ProcessorEnv {
            now: wall_clock(),
            dot_armed: &mut self.dot_armed,
            min_retained: self.min_retained,
        };
        let result = processor(
            &key,
            &mut self.context,
            &mut self.state,
            &mut self.decoder,
            &mut self.live,
            &mut env,
        );
        let consumed = match result {
            Ok(result) => matches!(result, ProcessorResult::Consume),
            Err(error) => {
                eprintln!("tigerclaw: processor error: {error}");
                false
            }
        };
        let mut commits = Vec::new();
        let mut invalidated = false;
        for event in self.context.drain_events() {
            if let Event::Commit(text) = event {
                invalidated = true;
                commits.push(text);
            }
        }
        for text in commits {
            self.host_commit(&text);
        }
        if let Err(error) = self.builder.rebuild(
            &mut self.decoder,
            &mut self.context,
            &self.state,
            invalidated,
        ) {
            eprintln!("tigerclaw: rebuild error: {error}");
        }
        self.push_update();
        consumed
    }

    /// 重置会话（`activate`/`deactivate`/`reset`）。
    fn reset(&mut self) {
        self.context.clear();
        self.state.reset(&mut self.context, false);
        self.live.pending.clear();
        self.live.baseline = None;
        self.live.submitted_raw = None;
        self.dot_armed = false;
        self.builder.reset();
        self.push_update();
    }

    /// 提交回调（`engine:commit_text`）。
    fn host_commit(&self, text: &str) {
        let Some(host) = &self.host else {
            return;
        };
        let Some(commit) = host.commit else {
            return;
        };
        if let Ok(text) = CString::new(text) {
            // SAFETY: 函数指针与 `user` 由宿主提供且在本调用期间有效。
            unsafe { commit(host.user, text.as_ptr()) };
        }
    }

    /// UI 更新回调：preedit（缓冲文本 + 实况输入）+ 字节光标 + 候选（文本/注释/高亮）。
    fn push_update(&self) {
        let Some(host) = &self.host else {
            return;
        };
        let Some(update) = host.update else {
            return;
        };
        let buffered = buffered_text(&self.context);
        let live = String::from_utf8_lossy(self.context.live_input()).into_owned();
        let mut preedit = String::new();
        preedit.push_str(&buffered);
        if !buffered.is_empty() && !live.is_empty() {
            preedit.push(' ');
        }
        preedit.push_str(&live);
        let prefix_length = if buffered.is_empty() {
            0
        } else {
            buffered.len() + usize::from(!live.is_empty())
        };
        let cursor = (prefix_length + self.context.live_caret()).min(preedit.len());
        let (texts, comments, selected) = match self.context.composition.back() {
            Some(segment) => (
                segment
                    .candidates
                    .iter()
                    .map(|candidate| candidate.text.clone())
                    .collect::<Vec<_>>(),
                segment
                    .candidates
                    .iter()
                    .map(|candidate| candidate.comment.clone())
                    .collect::<Vec<_>>(),
                segment.selected_index as i32,
            ),
            None => (Vec::new(), Vec::new(), 0),
        };
        let preedit = CString::new(preedit).unwrap_or_default();
        let texts: Vec<CString> = texts
            .iter()
            .map(|text| CString::new(text.as_str()).unwrap_or_default())
            .collect();
        let comments: Vec<CString> = comments
            .iter()
            .map(|comment| CString::new(comment.as_str()).unwrap_or_default())
            .collect();
        let text_pointers: Vec<*const c_char> = texts.iter().map(|text| text.as_ptr()).collect();
        let comment_pointers: Vec<*const c_char> =
            comments.iter().map(|comment| comment.as_ptr()).collect();
        // SAFETY: 指针数组与 C 串在本调用期间有效；计数与数组长度一致。
        unsafe {
            update(
                host.user,
                preedit.as_ptr(),
                cursor as i32,
                text_pointers.as_ptr(),
                comment_pointers.as_ptr(),
                text_pointers.len() as i32,
                selected,
            );
        }
    }
}

/// 创建引擎实例（`host` 可为空指针）。
///
/// # Safety
/// `host` 须为空或指向有效 `tigerclaw_host`（只做浅拷贝）。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tigerclaw_engine_new(host: *const HostCallback) -> *mut Engine {
    let host = if host.is_null() {
        None
    } else {
        Some(unsafe { *host })
    };
    Box::into_raw(Box::new(Engine::new(host)))
}

/// 释放引擎实例（`engine` 可为空指针）。
///
/// # Safety
/// `engine` 须为 [`tigerclaw_engine_new`] 的返回值且尚未释放。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tigerclaw_engine_free(engine: *mut Engine) {
    if engine.is_null() {
        return;
    }
    drop(unsafe { Box::from_raw(engine) });
}

/// 重置会话（对应 fcitx5 `InputMethodEngine::activate/deactivate/reset`）。
///
/// # Safety
/// 同 [`tigerclaw_engine_free`]。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tigerclaw_engine_reset(engine: *mut Engine) {
    if let Some(engine) = unsafe { engine.as_mut() } {
        engine.reset();
    }
}

/// 数据加载状态（诊断；NUL 结尾，随引擎存活）。
///
/// # Safety
/// `engine` 须有效（可为空指针）。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tigerclaw_engine_status(engine: *const Engine) -> *const c_char {
    match unsafe { engine.as_ref() } {
        Some(engine) => engine.status.as_ptr(),
        None => std::ptr::null(),
    }
}

/// 处理一次按键：返回 1 = 已消费。
///
/// # Safety
/// `engine` 须有效（可为空指针）。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tigerclaw_engine_key(
    engine: *mut Engine,
    keysym: u32,
    states: u32,
    release: i32,
) -> i32 {
    let Some(engine) = (unsafe { engine.as_mut() }) else {
        return 0;
    };
    i32::from(engine.key(keysym, states, release != 0))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    static COMMITS: Mutex<Vec<String>> = Mutex::new(Vec::new());
    type UpdateSnapshot = (String, i32, Vec<String>, i32);
    static UPDATES: Mutex<Vec<UpdateSnapshot>> = Mutex::new(Vec::new());

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
        UPDATES
            .lock()
            .unwrap()
            .push((preedit, cursor, candidates, selected));
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

    #[test]
    fn types_composition_and_commits_with_fixture() {
        COMMITS.lock().unwrap().clear();
        UPDATES.lock().unwrap().clear();
        let mut engine = Engine::new_with_dirs(host(), fixture_dirs(), None);
        // 「甲/乙」共用码 ab：输入两个键后出现候选，space 确认并提交。
        assert!(engine.key(u32::from(b'a'), 0, false));
        assert!(engine.key(u32::from(b'b'), 0, false));
        assert_eq!(engine.context.input(), b"ab");
        let (preedit, cursor, candidates, selected) =
            UPDATES.lock().unwrap().last().cloned().expect("update");
        assert_eq!(preedit, "ab");
        assert_eq!(cursor, 2);
        assert_eq!(candidates, vec!["甲".to_string(), "乙".to_string()]);
        assert_eq!(selected, 0);
        assert!(engine.key(0x20, 0, false)); // space
        assert_eq!(COMMITS.lock().unwrap().last().unwrap(), "甲");
        assert!(engine.context.input().is_empty());
    }

    #[test]
    fn modifiers_and_release_pass_through() {
        let mut engine = Engine::new_with_dirs(host(), fixture_dirs(), None);
        assert!(!engine.key(u32::from(b'a'), FCITX_CTRL, false)); // Ctrl+a 交宿主
        assert!(!engine.key(u32::from(b'a'), 0, true)); // release 交宿主
        assert!(!engine.key(0xff0d, 0, false)); // Return 空闲交宿主
    }

    #[test]
    fn idle_editing_keys_pass_through() {
        let mut engine = Engine::new_with_dirs(host(), fixture_dirs(), None);
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
    fn reset_clears_panel() {
        UPDATES.lock().unwrap().clear();
        let mut engine = Engine::new_with_dirs(host(), fixture_dirs(), None);
        engine.key(u32::from(b'a'), 0, false);
        engine.reset();
        let (preedit, _, candidates, _) = UPDATES.lock().unwrap().last().cloned().expect("update");
        assert!(preedit.is_empty());
        assert!(candidates.is_empty());
        assert!(engine.context.input().is_empty());
    }

    #[test]
    fn ffi_roundtrip() {
        let engine = unsafe { tigerclaw_engine_new(std::ptr::null()) };
        assert!(!engine.is_null());
        let status = unsafe { tigerclaw_engine_status(engine) };
        assert!(!status.is_null());
        let consumed = unsafe { tigerclaw_engine_key(engine, u32::from(b'a'), 0, 0) };
        assert_eq!(consumed, 1);
        unsafe { tigerclaw_engine_free(engine) };
    }
}
