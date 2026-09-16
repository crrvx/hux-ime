//! 虎爪（虎句方案）fcitx5 addon 的 Rust 侧（K3）：C ABI、数据加载与会话装配。
//!
//! 分工：`shell/tigerclaw.cpp` 只做 fcitx5 接口适配（按键 → 本层；提交/preedit/候选 ← 本层回调），
//! 逻辑在 Rust（本层 → `tigerclaw-core`）。组合重建照 2c 重放桩同构规则：
//! 提交（翻译失效）或输入变化时重建，否则保留段状态（含菜单高亮）。
//!
//! K3b 范围：每引擎单会话（`activate/reset` 清空）；数据目录见 [`data_dirs`]。

use std::ffi::{CString, c_char, c_void};
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

mod learning_store;
mod options;
mod settings;

use learning_store::LearningStore;
use options::OptionsStore;
use settings::Settings;

use tigerclaw_core::decode::Decoder;
use tigerclaw_core::host::{self, HostResult};
use tigerclaw_core::interaction::{
    CompositionBuilder, K_REVERSE_PREFIX, LiveLearning, ProcessorEnv, ProcessorResult,
    SentenceState, buffered_text, processor, reset_early_evidence, set_allow_duplicate_single,
    update_notifier,
};
use tigerclaw_core::key::{
    K_ALT_MASK, K_CONTROL_MASK, K_LOCK_MASK, K_RELEASE_MASK, K_SHIFT_MASK, K_SUPER_MASK, KeyEvent,
};
use tigerclaw_core::lexical;
use tigerclaw_core::lexicon::{
    LEXICAL_FILE, Lexicon, MODEL_PATH, Supplement, candidate_paths, data_directories,
};
use tigerclaw_core::ngram::MobileModel;
use tigerclaw_core::punct::PunctTable;
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

/// 反查前缀字符：可打印 ASCII 且无修饰键时启用（参照 `recognizer` 的 `ch > 0x20 && ch < 0x80`）。
fn reverse_prefix_char(key_repr: &str) -> Option<char> {
    let event = KeyEvent::from_repr(key_repr)?;
    let code = event.keycode;
    if (0x20..0x7f).contains(&code)
        && !event.shift()
        && !event.ctrl()
        && !event.alt()
        && !event.super_modifier()
    {
        char::from_u32(code as u32)
    } else {
        None
    }
}

/// 参照 `os.time()`：整秒墙钟（学习事件时间戳）。
pub(crate) fn wall_clock() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|value| value.as_secs() as f64)
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
    /// 选项存储（用户目录不可用时为 `None`，此时仅用内建缺省）。
    options: Option<OptionsStore>,
    /// 外部配置（fcitx5 配置界面 / 测试；默认 = 内建缺省）。
    settings: Settings,
    /// 标点表（`symbols.yaml`；缺失时标点交宿主）。
    punct: Option<PunctTable>,
    /// 学习库（用户目录不可用时为禁用占位）。
    learning: LearningStore,
    /// 学习规则串（来自码表；用于拼 mode）。
    learning_rules: String,
    /// 当前学习 mode 串与已应用的索引版本。
    learning_mode: String,
    applied_learning: Option<u64>,
    status: CString,
}

impl Engine {
    fn new(host: Option<HostCallback>) -> Self {
        let dirs = data_dirs();
        let model = std::env::var_os("TIGERCLAW_MODEL")
            .map(PathBuf::from)
            .or_else(|| default_model_path(&dirs));
        // 选项存于标准用户目录（与数据目录的开发覆盖解耦）。
        let options_dir = data_directories().into_iter().next();
        Self::new_with_dirs(host, dirs, model, options_dir)
    }

    fn new_with_dirs(
        host: Option<HostCallback>,
        dirs: Vec<PathBuf>,
        model_path: Option<PathBuf>,
        options_dir: Option<PathBuf>,
    ) -> Self {
        let mut notes = vec![format!(
            "dirs: {}",
            dirs.iter()
                .map(|dir| dir.display().to_string())
                .collect::<Vec<_>>()
                .join(":")
        )];
        let settings = Settings::default();
        let lexicon = Lexicon::load(&dirs, settings.high_freq_limit);
        notes.push(format!("lexicon: {}", lexicon.data_status().canonical()));
        let learning_rules = lexicon.learning_rules.clone();
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
        let (punct, punct_error) = PunctTable::load_first(&candidate_paths(&dirs, "symbols.yaml"));
        if punct.is_none()
            && let Some(error) = punct_error
        {
            notes.push(format!("punct: {error}"));
        }
        let mut context = Context::new();
        // 宿主缺省：`_auto_commit`（librime `express_editor` 默认 true）。
        context.set_option("_auto_commit", true);
        // 选项：有存储则同步（参照 `M.options.sync`，同步写入由核心抑制观察）；
        // 无存储时直接用内建缺省。
        let mut options = options_dir
            .as_deref()
            .map(|dir| OptionsStore::load_with_defaults(dir, settings.store_defaults()));
        if let Some(options) = options.as_mut() {
            options.sync(&mut context);
        } else {
            for (name, value) in settings.option_defaults() {
                context.set_option(name, value);
            }
        }
        // 学习库：`<user dir>/tiger_sentence_learning_<hash>.userdb/`（用户目录不可用则禁用）。
        let learning = match options_dir.as_deref() {
            Some(dir) => LearningStore::open(
                dir,
                &learning_store::store_name(learning_store::DEFAULT_SCHEMA_ID),
                wall_clock(),
            ),
            None => LearningStore::disabled("user data directory unavailable"),
        };
        if let Some(error) = &learning.error {
            notes.push(format!("learning: {error}"));
        } else {
            notes.push(format!("learning: {}", learning.name));
        }
        let learning_mode = settings.learning_mode(
            &learning_rules,
            u8::from(set_allow_duplicate_single(&context)),
        );
        let live = LiveLearning {
            mode: learning_mode.clone(),
            store_ready: learning.store_ready(),
            ..LiveLearning::default()
        };
        let mut engine = Self {
            host,
            decoder,
            context,
            state: SentenceState::fresh(1),
            live,
            dot_armed: false,
            min_retained: None,
            builder: CompositionBuilder::default(),
            options,
            settings,
            punct,
            learning,
            learning_rules,
            learning_mode,
            applied_learning: None,
            status: CString::new(notes.join("; ")).unwrap_or_default(),
        };
        engine.sync_reverse_prefix();
        engine.push_update();
        engine
    }

    /// 处理一次按键：返回是否消费；副作用（提交/preedit/候选）经宿主回调送出。
    fn key(&mut self, keysym: u32, states: u32, release: bool) -> bool {
        let key = KeyEvent::new(keysym as i32, core_modifiers(states, release));
        let now = wall_clock();
        let result = {
            let mut env = ProcessorEnv {
                now,
                dot_armed: &mut self.dot_armed,
                min_retained: self.min_retained,
            };
            processor(
                &key,
                &mut self.context,
                &mut self.state,
                &mut self.decoder,
                &mut self.live,
                &mut env,
            )
        };
        let consumed = match result {
            Ok(ProcessorResult::Consume) => true,
            // 参照链：处理器未消费的键交宿主等价物（selector/navigator/express_editor 等）。
            Ok(ProcessorResult::Forward) => {
                host::process_key(&key, &mut self.context, self.punct.as_mut())
                    == HostResult::Consumed
            }
            Err(error) => {
                eprintln!("tigerclaw: processor error: {error}");
                false
            }
        };
        let mut commits = Vec::new();
        let mut invalidated = false;
        // 事件泵：选项事件可能触发确认（进而产生提交），循环至排空（有界）。
        for _ in 0..4 {
            let events = self.context.drain_events();
            if events.is_empty() {
                break;
            }
            for event in events {
                match event {
                    Event::Commit(text) => {
                        invalidated = true;
                        commits.push(text);
                    }
                    Event::Option(name) => {
                        self.observe_option(&name);
                    }
                    Event::Update => {}
                }
            }
        }
        for text in commits {
            self.host_commit(&text);
        }
        // 学习：核心暂存 → 落库；刷新打分（未组合时，60 秒节流）；应用索引。
        let submitted = std::mem::take(&mut self.live.submitted);
        if !submitted.is_empty() {
            self.learning.confirm(&submitted);
        }
        self.refresh_learning_mode();
        if !self.context.is_composing() {
            self.learning.refresh_scores(now);
        }
        self.apply_learning();
        // 组合重建（参照 `ConcreteEngine::Compose`，先于通知器）与 update 通知器
        // （非组合清暂存；缓冲且实况输入为空时隐藏候选）。
        if let Err(error) = self.builder.rebuild(
            &mut self.decoder,
            &mut self.context,
            &self.state,
            invalidated,
            self.punct.as_mut(),
        ) {
            eprintln!("tigerclaw: rebuild error: {error}");
        }
        update_notifier(&mut self.context, &mut self.state, &mut self.live);
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
        if let Some(options) = self.options.as_mut() {
            options.sync(&mut self.context);
        }
        self.push_update();
    }

    /// 选项事件 → 记录/持久化（参照 `M.options` 的选项通知器）。
    fn observe_option(&mut self, name: &str) {
        if let Some(options) = self.options.as_mut() {
            options.observe(&mut self.context, name);
        }
    }

    /// 应用外部配置（fcitx5 配置界面 / 测试）：选项类即时生效；`high_freq_limit` 需重启。
    pub fn apply_settings(&mut self, settings: Settings) {
        self.settings = settings;
        let defaults = self.settings.option_defaults();
        for (name, value) in defaults {
            if self.context.get_option(name) != value {
                self.context.set_option(name, value);
            }
        }
        self.sync_reverse_prefix();
        self.refresh_learning_mode();
    }

    /// 反查前缀（`_reverse_prefix`）：配置键为可打印 ASCII 且无修饰时启用，否则关闭反查。
    fn sync_reverse_prefix(&mut self) {
        let value = reverse_prefix_char(&self.settings.reverse_pinyin_key)
            .map(|ch| ch.to_string())
            .unwrap_or_default();
        if self.context.get_property(K_REVERSE_PREFIX).unwrap_or("") != value {
            self.context.set_property(K_REVERSE_PREFIX, &value);
        }
    }

    /// 按当前规则/选项刷新学习 mode（变化时强制重设 decoder 学习）。
    fn refresh_learning_mode(&mut self) {
        let mode = self.settings.learning_mode(
            &self.learning_rules,
            u8::from(set_allow_duplicate_single(&self.context)),
        );
        if mode != self.learning_mode {
            self.learning_mode = mode.clone();
            self.live.mode = mode;
            self.applied_learning = None;
        }
    }

    /// 索引变化时重设 decoder 学习（参照 `active_index` 变化：重置早证据与空码态）。
    fn apply_learning(&mut self) {
        let version = self.learning.index_version();
        if self.applied_learning == Some(version) {
            return;
        }
        self.applied_learning = Some(version);
        self.decoder
            .set_learning(self.learning.index().clone(), &self.learning_mode);
        reset_early_evidence(&mut self.state);
        self.state.empty_code_pending = None;
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
        // 参照 librime `Composition::GetPreedit` + 参照 Lua 的候选 preedit：
        // 高亮候选的 preedit（「按词分码」，含缓冲前缀与反查前缀）优先；
        // 光标不在实况输入末尾时回退「缓冲 + 实况输入」，保证字节光标与字符串一致。
        let highlighted = self
            .context
            .composition
            .back()
            .and_then(|segment| segment.selected_candidate())
            .map(|candidate| candidate.preedit.clone())
            .unwrap_or_default();
        let caret_at_end = self.context.live_caret() >= self.context.live_input().len();
        let (mut preedit, cursor) = if !highlighted.is_empty() && caret_at_end {
            let cursor = highlighted.len();
            (highlighted, cursor)
        } else {
            let mut text = String::new();
            text.push_str(&buffered);
            if !buffered.is_empty() && !live.is_empty() {
                text.push(' ');
            }
            let prefix_length = if buffered.is_empty() {
                0
            } else {
                buffered.len() + usize::from(!live.is_empty())
            };
            text.push_str(&live);
            let cursor = (prefix_length + self.context.live_caret()).min(text.len());
            (text, cursor)
        };
        // 参照 `Composition::GetPreedit`：段提示插在光标处（如反查段的「〔拼音〕」）。
        let prompt = self
            .context
            .composition
            .back()
            .map(|segment| segment.prompt.clone())
            .unwrap_or_default();
        if !prompt.is_empty() {
            preedit.insert_str(cursor.min(preedit.len()), &prompt);
        }
        let (mut texts, mut comments, selected) = match self.context.composition.back() {
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
        // 参照 `_hide_candidate`：缓冲且实况输入为空时隐藏候选。
        if self.context.get_option("_hide_candidate") {
            texts.clear();
            comments.clear();
        }
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

/// 外部配置（C ABI 布局；与 `shell/tigerclaw_abi.h` 一致）。
#[derive(Clone, Copy, Debug, Default)]
#[repr(C)]
pub struct TigerclawOptions {
    pub early_commit: i32,
    pub early_commit_to_preedit: i32,
    pub allow_duplicate_single: i32,
    pub full_shape: i32,
    pub ascii_punct: i32,
    pub tab_learning: i32,
    pub high_freq_limit: i32,
    pub reverse_pinyin_sym: i32,
    pub reverse_pinyin_states: i32,
    pub reverse_hanzi_sym: i32,
    pub reverse_hanzi_states: i32,
    pub quick_input_sym: i32,
    pub quick_input_states: i32,
}

/// 应用外部配置（fcitx5 配置界面 → C++ 壳 → 本入口）。返回 1 = 已应用。
///
/// # Safety
/// `engine` 须有效；`options` 须为空或指向有效 `TigerclawOptions`。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tigerclaw_engine_apply_settings(
    engine: *mut Engine,
    options: *const TigerclawOptions,
) -> i32 {
    let Some(engine) = (unsafe { engine.as_mut() }) else {
        return 0;
    };
    let Some(options) = (unsafe { options.as_ref() }) else {
        return 0;
    };
    // fcitx5 按键（keysym + 状态位）→ rime 键名；未设（sym=0）为空串。
    let key_repr = |sym: i32, states: i32| -> String {
        if sym == 0 {
            return String::new();
        }
        KeyEvent::new(sym, core_modifiers(states as u32, false)).repr()
    };
    engine.apply_settings(Settings {
        early_commit: options.early_commit != 0,
        early_commit_to_preedit: options.early_commit_to_preedit != 0,
        allow_duplicate_single: options.allow_duplicate_single != 0,
        full_shape: options.full_shape != 0,
        ascii_punct: options.ascii_punct != 0,
        tab_learning: options.tab_learning != 0,
        high_freq_limit: options.high_freq_limit.max(0) as usize,
        reverse_pinyin_key: key_repr(options.reverse_pinyin_sym, options.reverse_pinyin_states),
        reverse_hanzi_key: key_repr(options.reverse_hanzi_sym, options.reverse_hanzi_states),
        quick_input_key: key_repr(options.quick_input_sym, options.quick_input_states),
    });
    1
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

    fn temp_user_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("tigerclaw-user-{}-{tag}", std::process::id()));
        std::fs::remove_dir_all(&dir).ok();
        std::fs::create_dir_all(&dir).expect("temp user dir");
        dir
    }

    /// 最近一次 UI 快照（preedit、字节光标、候选、高亮）。
    fn last_update() -> UpdateSnapshot {
        UPDATES.lock().unwrap().last().cloned().expect("update")
    }

    #[test]
    fn engine_wires_learning_store() {
        let _guard = serial();
        let dir = temp_user_dir("learning");
        let mut engine = Engine::new_with_dirs(host(), fixture_dirs(), None, Some(dir.clone()));
        assert!(engine.live.store_ready, "用户目录可用时学习库应就绪");
        assert!(engine.live.mode.starts_with("sentence-v1|rules="));
        engine.key(u32::from(b'a'), 0, false);
        assert!(engine.applied_learning.is_some(), "按键后应已应用学习索引");
        assert!(
            dir.join(format!(
                "{}.userdb",
                learning_store::store_name(learning_store::DEFAULT_SCHEMA_ID)
            ))
            .is_dir()
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn types_composition_and_commits_with_fixture() {
        let _guard = serial();
        COMMITS.lock().unwrap().clear();
        UPDATES.lock().unwrap().clear();
        let mut engine = Engine::new_with_dirs(host(), fixture_dirs(), None, None);
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
        let _guard = serial();
        let mut engine = Engine::new_with_dirs(host(), fixture_dirs(), None, None);
        assert!(!engine.key(u32::from(b'a'), FCITX_CTRL, false)); // Ctrl+a 交宿主
        assert!(!engine.key(u32::from(b'a'), 0, true)); // release 交宿主
        assert!(!engine.key(0xff0d, 0, false)); // Return 空闲交宿主
    }

    #[test]
    fn idle_editing_keys_pass_through() {
        let _guard = serial();
        let mut engine = Engine::new_with_dirs(host(), fixture_dirs(), None, None);
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
    fn composing_editing_keys_update_panel_and_are_consumed() {
        let _guard = serial();
        COMMITS.lock().unwrap().clear();
        UPDATES.lock().unwrap().clear();
        let mut engine = Engine::new_with_dirs(host(), fixture_dirs(), None, None);
        assert!(engine.key(u32::from(b'a'), 0, false));
        assert!(engine.key(u32::from(b'b'), 0, false));
        let (preedit, cursor, candidates, _) = last_update();
        assert_eq!(preedit, "ab");
        assert_eq!(cursor, 2);
        assert_eq!(candidates, vec!["甲".to_string(), "乙".to_string()]);
        // ←：光标左移；组合按 caret 前缀重建（候选清空）
        assert!(engine.key(0xff51, 0, false), "组合中 Left 应被消费");
        let (preedit, cursor, candidates, _) = last_update();
        assert_eq!(preedit, "ab");
        assert_eq!(cursor, 1);
        assert!(candidates.is_empty(), "光标在输入中间时无候选");
        // →：回到末尾，候选恢复
        assert!(engine.key(0xff53, 0, false));
        let (_, cursor, candidates, _) = last_update();
        assert_eq!(cursor, 2);
        assert_eq!(candidates, vec!["甲".to_string(), "乙".to_string()]);
        // ↓：高亮下移；↑ 到首项
        assert!(engine.key(0xff54, 0, false));
        let (_, _, _, selected) = last_update();
        assert_eq!(selected, 1);
        assert!(engine.key(0xff52, 0, false));
        let (_, _, _, selected) = last_update();
        assert_eq!(selected, 0);
        // 退格：删除输入
        assert!(engine.key(0xff08, 0, false));
        let (preedit, cursor, candidates, _) = last_update();
        assert_eq!(preedit, "a");
        assert_eq!(cursor, 1);
        assert!(candidates.is_empty());
        // 再退格清空组合；此后交宿主
        assert!(engine.key(0xff08, 0, false));
        let (preedit, _, candidates, _) = last_update();
        assert!(preedit.is_empty());
        assert!(candidates.is_empty());
        assert!(!engine.key(0xff08, 0, false), "空闲 BackSpace 交宿主");
    }

    #[test]
    fn punctuation_commits_via_table() {
        let _guard = serial();
        COMMITS.lock().unwrap().clear();
        UPDATES.lock().unwrap().clear();
        let mut engine = Engine::new_with_dirs(host(), fixture_dirs(), None, None);
        // 空闲：标点直提交（symbols.yaml half_shape："." → 。）
        assert!(engine.key(0x2e, 0, false), "period 应被消费");
        assert_eq!(COMMITS.lock().unwrap().last().unwrap(), "。");
        // 组合中：当前候选 + 标点一并提交并清空
        assert!(engine.key(u32::from(b'a'), 0, false));
        assert!(engine.key(u32::from(b'b'), 0, false));
        assert!(engine.key(0x2c, 0, false), "comma 应被消费");
        assert_eq!(COMMITS.lock().unwrap().last().unwrap(), "甲，");
        assert!(engine.context.input().is_empty());
        // pair 交替（apostrophe：'‘' / '’'）
        assert!(engine.key(0x27, 0, false));
        assert_eq!(COMMITS.lock().unwrap().last().unwrap(), "‘");
        assert!(engine.key(0x27, 0, false));
        assert_eq!(COMMITS.lock().unwrap().last().unwrap(), "’");
        // 半角空格未映射：交宿主
        assert!(!engine.key(0x20, 0, false), "空闲空格交宿主");
    }

    #[test]
    fn uppercase_letter_commits_composition_first() {
        let _guard = serial();
        // 用户报告：组合中收到大写字母时，应先上屏当前候选（而非把字母插到预编辑之前）。
        COMMITS.lock().unwrap().clear();
        UPDATES.lock().unwrap().clear();
        let mut engine = Engine::new_with_dirs(host(), fixture_dirs(), None, None);
        assert!(engine.key(u32::from(b'a'), 0, false));
        assert!(engine.key(u32::from(b'b'), 0, false));
        assert!(
            !engine.key(0x41, FCITX_SHIFT, false),
            "大写字母应交宿主（不消费）"
        );
        assert_eq!(COMMITS.lock().unwrap().last().unwrap(), "甲");
        assert!(engine.context.input().is_empty(), "组合已提交并清空");
    }

    #[test]
    fn apply_settings_switches_options_and_learning() {
        let _guard = serial();
        let mut engine = Engine::new_with_dirs(host(), fixture_dirs(), None, None);
        let settings = Settings {
            full_shape: true,
            ascii_punct: true,
            tab_learning: false,
            high_freq_limit: 100,
            ..Default::default()
        };
        engine.apply_settings(settings);
        assert!(engine.context.get_option("full_shape"));
        assert!(engine.context.get_option("ascii_punct"));
        assert!(
            engine.live.mode.is_empty(),
            "关闭 Tab 学习 → 学习 mode 为空"
        );
    }

    #[test]
    fn ffi_apply_settings_roundtrip() {
        let _guard = serial();
        let engine = unsafe { tigerclaw_engine_new(std::ptr::null()) };
        assert!(!engine.is_null());
        let options = TigerclawOptions {
            early_commit: 0,
            early_commit_to_preedit: 1,
            allow_duplicate_single: 1,
            full_shape: 1,
            ascii_punct: 1,
            tab_learning: 0,
            high_freq_limit: 800,
            reverse_pinyin_sym: 0x60,
            reverse_pinyin_states: 0,
            reverse_hanzi_sym: 0x60,
            reverse_hanzi_states: 1,
            quick_input_sym: 0x3b,
            quick_input_states: 0,
        };
        let applied = unsafe { tigerclaw_engine_apply_settings(engine, &options) };
        assert_eq!(applied, 1);
        let state = unsafe { &mut *engine };
        assert!(!state.settings.early_commit);
        assert!(state.context.get_option("full_shape"));
        assert!(state.context.get_option("ascii_punct"));
        assert!(
            state.live.mode.is_empty(),
            "tab_learning=0 → 学习 mode 为空"
        );
        assert_eq!(state.settings.high_freq_limit, 800);
        assert_eq!(state.settings.reverse_pinyin_key, "grave");
        assert_eq!(state.settings.reverse_hanzi_key, "Shift+grave");
        assert_eq!(state.settings.quick_input_key, "semicolon");
        unsafe { tigerclaw_engine_free(engine) };
    }

    #[test]
    fn reset_clears_panel() {
        let _guard = serial();
        UPDATES.lock().unwrap().clear();
        let mut engine = Engine::new_with_dirs(host(), fixture_dirs(), None, None);
        engine.key(u32::from(b'a'), 0, false);
        engine.reset();
        let (preedit, _, candidates, _) = UPDATES.lock().unwrap().last().cloned().expect("update");
        assert!(preedit.is_empty());
        assert!(candidates.is_empty());
        assert!(engine.context.input().is_empty());
    }

    /// 反查（⑧-1）端到端：设置 → 前缀识别 → 候选/注释 → 预编辑提示 → 空格上屏。
    #[test]
    fn reverse_lookup_end_to_end() {
        let _guard = serial();
        COMMITS.lock().unwrap().clear();
        UPDATES.lock().unwrap().clear();
        let dirs = vec![
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../goldens/reverse"),
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../data"),
        ];
        let mut engine = Engine::new_with_dirs(host(), dirs, None, None);
        assert!(engine.key(u32::from(b'`'), 0, false), "反查前缀应被消费");
        for code in *b"zho" {
            assert!(engine.key(u32::from(code), 0, false), "反查输入应被消费");
        }
        let (preedit, _, candidates, _) = last_update();
        assert_eq!(preedit, "`zho〔拼音〕");
        assert_eq!(
            candidates,
            vec!["中哦", "中龘", "中欧", "找哦", "兆欧", "找欧"]
        );
        assert!(engine.key(0x20, 0, false));
        assert_eq!(COMMITS.lock().unwrap().last().unwrap(), "中哦");
        // 反查预编辑「按音节分码」：全拼音节之间插空格。
        engine.reset();
        for code in *b"`zhongguo" {
            assert!(engine.key(u32::from(code), 0, false));
        }
        let (preedit, _, candidates, _) = last_update();
        assert_eq!(candidates.first().map(String::as_str), Some("中国"));
        assert_eq!(preedit, "`zhong guo〔拼音〕");
    }

    /// 预编辑「按词分码」：使用高亮候选的 preedit（`ab cd`），单字不分段（`ab`）。
    #[test]
    fn preedit_uses_segmented_codes() {
        let _guard = serial();
        UPDATES.lock().unwrap().clear();
        let mut engine = Engine::new_with_dirs(host(), fixture_dirs(), None, None);
        for code in *b"abcd" {
            engine.key(u32::from(code), 0, false);
        }
        let (preedit, cursor, candidates, _) = last_update();
        assert!(!candidates.is_empty(), "abcd 应有候选");
        assert_eq!(preedit, "ab cd");
        assert_eq!(cursor, 5);
        engine.reset();
        for code in *b"ab" {
            engine.key(u32::from(code), 0, false);
        }
        let (preedit, cursor, _, _) = last_update();
        assert_eq!(preedit, "ab");
        assert_eq!(cursor, 2);
    }

    #[test]
    fn ffi_roundtrip() {
        let _guard = serial();
        let engine = unsafe { tigerclaw_engine_new(std::ptr::null()) };
        assert!(!engine.is_null());
        let status = unsafe { tigerclaw_engine_status(engine) };
        assert!(!status.is_null());
        let consumed = unsafe { tigerclaw_engine_key(engine, u32::from(b'a'), 0, 0) };
        assert_eq!(consumed, 1);
        unsafe { tigerclaw_engine_free(engine) };
    }
}
