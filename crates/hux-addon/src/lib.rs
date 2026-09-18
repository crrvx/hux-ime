// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! hux-ime（虎虚）fcitx5 addon 的 Rust 侧（K3）：C ABI、数据加载与会话装配。
//!
//! 分工：`shell/hux.cpp` 只做 fcitx5 接口适配（按键 → 本层；提交/preedit/候选 ← 本层回调），
//! 逻辑在 Rust（本层 → `hux-core`）。组合重建照 2c 重放桩同构规则：
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

use hux_core::char_to_sound_shape;
use hux_core::decode::Decoder;
use hux_core::host::{self, HostOptions, HostResult};
use hux_core::interaction::{
    CompositionBuilder, K_CHAR_TO_SOUND_SHAPE_KEY, K_SOUND_TO_CHAR_SHAPE_KEY, LiveLearning,
    ProcessorEnv, ProcessorResult, SentenceState, buffered_text, processor, reset_early_evidence,
    set_allow_duplicate_single, update_notifier,
};
use hux_core::key::{
    K_ALT_MASK, K_CONTROL_MASK, K_LOCK_MASK, K_RELEASE_MASK, K_SHIFT_MASK, K_SUPER_MASK, KeyEvent,
};
use hux_core::lexical;
use hux_core::lexicon::{
    LEXICAL_FILE, Lexicon, MODEL_PATH, Supplement, candidate_paths, data_directories,
};
use hux_core::ngram::MobileModel;
use hux_core::punct::PunctTable;
use hux_core::session::{Context, Event};

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
            *const c_char,
            *const c_char,
        ),
    >,
}

/// 数据目录：`HUX_DATA_DIRS`（冒号分隔，开发用）或用户/共享标准目录。
pub fn data_dirs() -> Vec<PathBuf> {
    if let Ok(value) = std::env::var("HUX_DATA_DIRS") {
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

/// 参照 `os.time()`：整秒墙钟（学习事件时间戳）。
pub(crate) fn wall_clock() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|value| value.as_secs() as f64)
        .unwrap_or(0.0)
}

/// 字反查（⑧-2）会话态：周边文本（字符制光标）+ 窗口起点 + 已算好的提示。
#[derive(Default)]
struct CharToSoundShapeState {
    valid: bool,
    text: String,
    cursor: usize,
    aux_up: String,
    aux_down: String,
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
    /// 宿主选项（翻页键/页大小；由 `settings` 派生，避免每次按键解析键名）。
    host_options: HostOptions,
    /// 标点表（`symbols.yaml`；缺失时标点交宿主）。
    punct: Option<PunctTable>,
    /// 学习库（用户目录不可用时为禁用占位）。
    learning: LearningStore,
    /// 字反查（⑧-2）会话态。
    char_to_sound_shape: CharToSoundShapeState,
    /// 学习规则串（来自码表；用于拼 mode）。
    learning_rules: String,
    /// 当前学习 mode 串与已应用的索引版本。
    learning_mode: String,
    applied_learning: Option<u64>,
    /// 本次按键「已提交且未消费」：宿主层应消费该键并以 `forwardKey` 重发，
    /// 保证「提交 → 按键」送达顺序（对齐 fcitx5 核心 `KeyEventOrderFix` 修法）。
    pub forward_after_commit: bool,
    status: CString,
}

impl Engine {
    fn new(host: Option<HostCallback>) -> Self {
        let dirs = data_dirs();
        let model = std::env::var_os("HUX_MODEL")
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
        let host_options = settings.host_options();
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
            host_options,
            punct,
            learning,
            char_to_sound_shape: CharToSoundShapeState::default(),
            learning_rules,
            learning_mode,
            applied_learning: None,
            forward_after_commit: false,
            status: CString::new(notes.join("; ")).unwrap_or_default(),
        };
        engine.sync_sound_to_char_shape_prefix();
        engine.push_update();
        engine
    }

    /// 处理一次按键：返回是否消费；副作用（提交/preedit/候选）经宿主回调送出。
    ///
    /// 提交且未消费（`express_editor` 的 `char_handler = DirectCommit`）时置
    /// [`Engine::forward_after_commit`]：宿主层据此消费该键并以 `forwardKey` 重发，
    /// 保证客户端先收到提交、后收到按键。
    fn key(&mut self, keysym: u32, states: u32, release: bool) -> bool {
        self.forward_after_commit = false;
        let key = KeyEvent::new(keysym as i32, core_modifiers(states, release));
        // 字反查段：←/→/↑/↓ **交应用处理**（应用光标随动），本层不消费也不改动输入；
        // 两排在应用回传周边文本后的下一次按键（含 release）时刷新。
        if !release
            && self.char_to_sound_shape_tagged()
            && matches!(key.repr().as_str(), "Left" | "Right" | "Up" | "Down")
        {
            return false;
        }
        let now = wall_clock();
        let result = {
            let mut env = ProcessorEnv {
                now,
                dot_armed: &mut self.dot_armed,
                min_retained: self.min_retained,
                page_size: self.host_options.page_size,
                digit_select: self.settings.digit_select,
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
                host::process_key(
                    &key,
                    &mut self.context,
                    self.punct.as_mut(),
                    &self.host_options,
                ) == HostResult::Consumed
            }
            Err(error) => {
                eprintln!("hux: processor error: {error}");
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
        let committed = !commits.is_empty();
        for text in commits {
            self.host_commit(&text);
        }
        self.forward_after_commit = !consumed && committed;
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
            eprintln!("hux: rebuild error: {error}");
        }
        update_notifier(&mut self.context, &mut self.state, &mut self.live);
        self.refresh_char_to_sound_shape_aux();
        self.push_update();
        consumed
    }

    /// 字反查（⑧-2）：查码段内 ←/→ 以 2 字符步长移动锚点（返回 `Some(true)` 消费）。
    /// 进入/退出查码段由 core 处理器负责（触发字符推入/清空组合）。
    /// 当前组合末段是否为字反查段。
    fn char_to_sound_shape_tagged(&self) -> bool {
        self.context
            .composition
            .back()
            .is_some_and(|segment| segment.has_tag(char_to_sound_shape::TAG))
    }

    /// 重算两排提示（上排 = 光标左侧拼音、下排 = 虎码）；不在查码段则清空。
    fn refresh_char_to_sound_shape_aux(&mut self) {
        let tagged = self.char_to_sound_shape_tagged();
        let state = &mut self.char_to_sound_shape;
        if !tagged {
            state.aux_up.clear();
            state.aux_down.clear();
            return;
        }
        if !state.valid {
            // 周边文本不可用（如终端）：不显示提示——提示能否呈现取决于前端，
            // 统一清空两排。
            state.aux_up.clear();
            state.aux_down.clear();
            return;
        }
        let (text, cursor) = (state.text.clone(), state.cursor);
        let (up, down) = self
            .decoder
            .char_to_sound_shape_rows(&text, cursor)
            .unwrap_or_default();
        state.aux_up = up;
        state.aux_down = down;
    }

    /// 宿主送入应用侧周边文本（字符制光标；`None` = 应用不支持/不可用）。
    pub fn set_surrounding(&mut self, text: Option<&str>, cursor_chars: usize) {
        let state = &mut self.char_to_sound_shape;
        match text {
            Some(text) => {
                state.valid = true;
                state.text = text.to_string();
                state.cursor = cursor_chars.min(state.text.chars().count());
            }
            None => {
                state.valid = false;
                state.text.clear();
                state.cursor = 0;
            }
        }
        if self.char_to_sound_shape_tagged() {
            self.refresh_char_to_sound_shape_aux();
        }
    }

    /// 重置会话（`activate`/`deactivate`/`reset`）。
    fn reset(&mut self) {
        self.char_to_sound_shape = CharToSoundShapeState::default();
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
        self.host_options = self.settings.host_options();
        let defaults = self.settings.option_defaults();
        for (name, value) in defaults {
            if self.context.get_option(name) != value {
                self.context.set_option(name, value);
            }
        }
        self.sync_sound_to_char_shape_prefix();
        self.refresh_learning_mode();
    }

    /// 触发键（属性）：把两项触发键的 rime 键名列表（逗号分隔）交给 core（解析/匹配均在 core 内）。
    fn sync_sound_to_char_shape_prefix(&mut self) {
        for (property, value) in [
            (
                K_SOUND_TO_CHAR_SHAPE_KEY,
                self.settings.sound_to_char_shape_keys.join(","),
            ),
            (
                K_CHAR_TO_SOUND_SHAPE_KEY,
                self.settings.char_to_sound_shape_keys.join(","),
            ),
        ] {
            if self.context.get_property(property).unwrap_or("") != value {
                self.context.set_property(property, &value);
            }
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
        let live_bytes = self.context.live_input();
        let live = String::from_utf8_lossy(live_bytes).into_owned();
        // 参照 librime `Composition::GetPreedit` + 参照 Lua 的候选 preedit：
        // 高亮候选的 preedit（「按字分码」，含缓冲前缀与音反查前缀）始终优先；
        // 组合（光标）之后的原始输入原样接在其后——左/右移动时保持按字分码，
        // 光标落在分码文本末尾、原始尾部之前。
        let highlighted = self
            .context
            .composition
            .back()
            .and_then(|segment| segment.selected_candidate())
            .map(|candidate| candidate.preedit.clone())
            .unwrap_or_default();
        let (mut preedit, cursor) = if !highlighted.is_empty() {
            let cursor = highlighted.len();
            // 末段 `end` 为组合输入（含缓冲 `~` 标记）的字节位；换算到实况输入。
            let marker =
                usize::from(!buffered.is_empty() && self.context.input().first() == Some(&b'~'));
            let composed_end = self
                .context
                .composition
                .back()
                .map(|segment| segment.end)
                .unwrap_or(0)
                .saturating_sub(marker)
                .min(live_bytes.len());
            let mut text = highlighted;
            text.push_str(&String::from_utf8_lossy(&live_bytes[composed_end..]));
            (text, cursor)
        } else {
            // 无高亮候选（如未翻译段）：回退「缓冲 + 实况输入」，光标按字节对应。
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
        // 参照 `Composition::GetPreedit`：段提示插在光标处（如音反查段的「〔拼音〕」）。
        let prompt = self
            .context
            .composition
            .back()
            .map(|segment| segment.prompt.clone())
            .unwrap_or_default();
        if !prompt.is_empty() {
            preedit.insert_str(cursor.min(preedit.len()), &prompt);
        }
        // 字反查段不下发预编辑：避免应用端 marked text 锁住光标（←/→ 无法移动）。
        let mut cursor = cursor;
        if self.char_to_sound_shape_tagged() {
            preedit.clear();
            cursor = 0;
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
        let aux_up = CString::new(self.char_to_sound_shape.aux_up.as_str()).unwrap_or_default();
        let aux_down = CString::new(self.char_to_sound_shape.aux_down.as_str()).unwrap_or_default();
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
                aux_up.as_ptr(),
                aux_down.as_ptr(),
            );
        }
    }
}

/// 创建引擎实例（`host` 可为空指针）。
///
/// # Safety
/// `host` 须为空或指向有效 `hux_host`（只做浅拷贝）。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn hux_engine_new(host: *const HostCallback) -> *mut Engine {
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
/// `engine` 须为 [`hux_engine_new`] 的返回值且尚未释放。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn hux_engine_free(engine: *mut Engine) {
    if engine.is_null() {
        return;
    }
    drop(unsafe { Box::from_raw(engine) });
}

/// 重置会话（对应 fcitx5 `InputMethodEngine::activate/deactivate/reset`）。
///
/// # Safety
/// 同 [`hux_engine_free`]。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn hux_engine_reset(engine: *mut Engine) {
    if let Some(engine) = unsafe { engine.as_mut() } {
        engine.reset();
    }
}

/// 数据加载状态（诊断；NUL 结尾，随引擎存活）。
///
/// # Safety
/// `engine` 须有效（可为空指针）。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn hux_engine_status(engine: *const Engine) -> *const c_char {
    match unsafe { engine.as_ref() } {
        Some(engine) => engine.status.as_ptr(),
        None => std::ptr::null(),
    }
}

/// 键位列表上限（与 `shell/hux_abi.h` 的 `HUX_MAX_KEYS` 一致）。
pub const HUX_MAX_KEYS: usize = 8;

/// 键位列表（fcitx5 `KeyList` → C ABI；`sym == 0` 的项忽略）。
#[derive(Clone, Copy, Debug, Default)]
#[repr(C)]
pub struct HuxKeyList {
    pub count: i32,
    pub sym: [i32; HUX_MAX_KEYS],
    pub states: [i32; HUX_MAX_KEYS],
}

/// 外部配置（C ABI 布局；与 `shell/hux_abi.h` 一致）。
#[derive(Clone, Copy, Debug, Default)]
#[repr(C)]
pub struct HuxOptions {
    pub early_commit: i32,
    pub early_commit_to_preedit: i32,
    pub allow_duplicate_single: i32,
    pub full_shape: i32,
    pub ascii_punct: i32,
    pub tab_learning: i32,
    pub high_freq_limit: i32,
    /// 音反查触发键（rime 键名，可多项）。
    pub sound_to_char_shape: HuxKeyList,
    /// 字反查触发键（rime 键名，可多项）。
    pub char_to_sound_shape: HuxKeyList,
    /// 每页候选个数（1..=10）。
    pub page_size: i32,
    /// 上/下翻页键（rime 键名，可多项）。
    pub page_up: HuxKeyList,
    pub page_down: HuxKeyList,
    /// 数字直选（1–9；0=10）。
    pub digit_select: i32,
}

/// 应用外部配置（fcitx5 配置界面 → C++ 壳 → 本入口）。返回 1 = 已应用。
///
/// # Safety
/// `engine` 须有效；`options` 须为空或指向有效 `HuxOptions`。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn hux_engine_apply_settings(
    engine: *mut Engine,
    options: *const HuxOptions,
) -> i32 {
    let Some(engine) = (unsafe { engine.as_mut() }) else {
        return 0;
    };
    let Some(options) = (unsafe { options.as_ref() }) else {
        return 0;
    };
    // fcitx5 按键列表（keysym + 状态位）→ rime 键名列表；`sym=0` 项忽略。
    let key_reprs = |list: &HuxKeyList| -> Vec<String> {
        (0..HUX_MAX_KEYS)
            .take(list.count.clamp(0, HUX_MAX_KEYS as i32) as usize)
            .filter_map(|index| {
                let sym = list.sym[index];
                (sym != 0).then(|| {
                    KeyEvent::new(sym, core_modifiers(list.states[index] as u32, false)).repr()
                })
            })
            .collect()
    };
    engine.apply_settings(Settings {
        early_commit: options.early_commit != 0,
        early_commit_to_preedit: options.early_commit_to_preedit != 0,
        allow_duplicate_single: options.allow_duplicate_single != 0,
        full_shape: options.full_shape != 0,
        ascii_punct: options.ascii_punct != 0,
        tab_learning: options.tab_learning != 0,
        high_freq_limit: options.high_freq_limit.max(0) as usize,
        sound_to_char_shape_keys: key_reprs(&options.sound_to_char_shape),
        char_to_sound_shape_keys: key_reprs(&options.char_to_sound_shape),
        page_size: options.page_size.max(1) as usize,
        page_up_keys: key_reprs(&options.page_up),
        page_down_keys: key_reprs(&options.page_down),
        digit_select: options.digit_select != 0,
    });
    1
}

/// 送入应用侧周边文本（字符制光标；`valid=0` 表示不可用/应用不支持）。返回 1 = 已受理。
///
/// # Safety
/// `engine` 须有效（可为空指针）；`text` 须为空或指向 NUL 结尾字符串。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn hux_engine_set_surrounding(
    engine: *mut Engine,
    text: *const c_char,
    cursor_chars: i32,
    valid: i32,
) -> i32 {
    let Some(engine) = (unsafe { engine.as_mut() }) else {
        return 0;
    };
    if valid == 0 || text.is_null() {
        engine.set_surrounding(None, 0);
    } else {
        let text = unsafe { std::ffi::CStr::from_ptr(text) };
        engine.set_surrounding(
            Some(text.to_string_lossy().as_ref()),
            cursor_chars.max(0) as usize,
        );
    }
    1
}

/// `hux_engine_key` 返回值位掩码：已消费（宿主不应再处理该键）。
pub const HUX_KEY_CONSUMED: i32 = 0x1;
/// `hux_engine_key` 返回值位掩码：已提交且未消费——宿主应消费该键并以 `forwardKey`
/// 重发（保证客户端先收到提交、后收到按键；对齐 fcitx5 核心 `KeyEventOrderFix` 修法）。
pub const HUX_KEY_FORWARD_AFTER_COMMIT: i32 = 0x2;

/// 处理一次按键：返回位掩码 [`HUX_KEY_CONSUMED`] / [`HUX_KEY_FORWARD_AFTER_COMMIT`]。
///
/// # Safety
/// `engine` 须有效（可为空指针）。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn hux_engine_key(
    engine: *mut Engine,
    keysym: u32,
    states: u32,
    release: i32,
) -> i32 {
    let Some(engine) = (unsafe { engine.as_mut() }) else {
        return 0;
    };
    let consumed = engine.key(keysym, states, release != 0);
    let mut disposition = 0;
    if engine.forward_after_commit {
        disposition |= HUX_KEY_FORWARD_AFTER_COMMIT;
    }
    if consumed {
        disposition |= HUX_KEY_CONSUMED;
    }
    disposition
}

#[cfg(test)]
mod tests {
    use super::*;
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
        let dir = std::env::temp_dir().join(format!("hux-user-{}-{tag}", std::process::id()));
        std::fs::remove_dir_all(&dir).ok();
        std::fs::create_dir_all(&dir).expect("temp user dir");
        dir
    }

    /// 最近一次 UI 快照（preedit、字节光标、候选、高亮）。
    fn last_update() -> UpdateSnapshot {
        UPDATES.lock().unwrap().last().cloned().expect("update")
    }

    #[test]
    fn engine_enables_learning_store() {
        let _guard = serial();
        let dir = temp_user_dir("learning");
        let engine = Engine::new_with_dirs(host(), fixture_dirs(), None, Some(dir.clone()));
        assert!(engine.live.store_ready, "用户目录可用时学习库应就绪");
        assert!(engine.live.mode.starts_with("sentence-v1|rules="));
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
    fn engine_applies_learning_after_key() {
        let _guard = serial();
        let dir = temp_user_dir("learning-apply");
        let mut engine = Engine::new_with_dirs(host(), fixture_dirs(), None, Some(dir.clone()));
        engine.key(u32::from(b'a'), 0, false);
        assert!(engine.applied_learning.is_some(), "按键后应已应用学习索引");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn typing_shows_preedit_and_candidates() {
        let _guard = serial();
        UPDATES.lock().unwrap().clear();
        let mut engine = Engine::new_with_dirs(host(), fixture_dirs(), None, None);
        // 「甲/乙」共用码 ab：输入两个键后出现候选。
        assert!(engine.key(u32::from(b'a'), 0, false));
        assert!(engine.key(u32::from(b'b'), 0, false));
        assert_eq!(engine.context.input(), b"ab");
        let (preedit, cursor, candidates, selected, _, _) = last_update();
        assert_eq!(preedit, "ab");
        assert_eq!(cursor, 2);
        assert_eq!(candidates, vec!["甲".to_string(), "乙".to_string()]);
        assert_eq!(selected, 0);
    }

    #[test]
    fn space_commits_highlighted_candidate() {
        let _guard = serial();
        COMMITS.lock().unwrap().clear();
        let mut engine = Engine::new_with_dirs(host(), fixture_dirs(), None, None);
        assert!(engine.key(u32::from(b'a'), 0, false));
        assert!(engine.key(u32::from(b'b'), 0, false));
        assert!(engine.key(0x20, 0, false)); // space
        assert_eq!(COMMITS.lock().unwrap().last().unwrap(), "甲");
        assert!(engine.context.input().is_empty());
    }

    /// 上翻页键：候选菜单可见即消费（首屏也不落作标点/输入）。
    #[test]
    fn page_up_is_consumed_with_menu() {
        let _guard = serial();
        COMMITS.lock().unwrap().clear();
        UPDATES.lock().unwrap().clear();
        let mut engine = Engine::new_with_dirs(host(), fixture_dirs(), None, None);
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
        let mut engine = Engine::new_with_dirs(host(), fixture_dirs(), None, None);
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
        let mut engine = Engine::new_with_dirs(host(), fixture_dirs(), None, None);
        engine.apply_settings(Settings {
            sound_to_char_shape_keys: vec!["grave".to_string(), "semicolon".to_string()],
            ..Default::default()
        });
        // 第二绑定（`;`）触发，入段字符为 `;`。
        assert!(engine.key(0x3b, 0, false), "; 应被消费");
        assert_eq!(engine.context.input(), b";");
        engine.reset();
        // 第一绑定（`` ` ``）触发，入段字符为 `` ` ``。
        assert!(engine.key(0x60, 0, false), "` 应被消费");
        assert_eq!(engine.context.input(), b"`");
    }
    /// 数字直选默认关：数字仍是编码字符（选重后缀），不直接上屏。
    #[test]
    fn digit_select_off_keeps_rank_suffix() {
        let _guard = serial();
        COMMITS.lock().unwrap().clear();
        let mut engine = Engine::new_with_dirs(host(), fixture_dirs(), None, None);
        for code in *b"ja" {
            engine.key(u32::from(code), 0, false);
        }
        assert!(engine.key(u32::from(b'2'), 0, false));
        assert!(
            COMMITS.lock().unwrap().is_empty(),
            "默认关：数字不应直接上屏"
        );
        assert!(engine.context.input().ends_with(b"2"));
    }

    /// 数字直选：页大小 5 时 `0`（第 10 个）不在页内，按普通数字输入处理。
    #[test]
    fn digit_select_out_of_page_falls_through() {
        let _guard = serial();
        COMMITS.lock().unwrap().clear();
        let mut engine = Engine::new_with_dirs(host(), fixture_dirs(), None, None);
        engine.apply_settings(Settings {
            digit_select: true,
            ..Default::default()
        });
        for code in *b"ja" {
            engine.key(u32::from(code), 0, false);
        }
        assert!(engine.key(u32::from(b'0'), 0, false));
        assert!(COMMITS.lock().unwrap().is_empty(), "页外数字不应直接上屏");
        assert!(engine.context.input().ends_with(b"0"));
    }

    #[test]
    fn modified_keys_pass_through() {
        let _guard = serial();
        let mut engine = Engine::new_with_dirs(host(), fixture_dirs(), None, None);
        assert!(!engine.key(u32::from(b'a'), FCITX_CTRL, false)); // Ctrl+a 交宿主
    }

    #[test]
    fn key_releases_pass_through() {
        let _guard = serial();
        let mut engine = Engine::new_with_dirs(host(), fixture_dirs(), None, None);
        assert!(!engine.key(u32::from(b'a'), 0, true));
    }

    #[test]
    fn idle_return_passes_through() {
        let _guard = serial();
        let mut engine = Engine::new_with_dirs(host(), fixture_dirs(), None, None);
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
    fn composing_left_right_move_caret_and_toggle_candidates() {
        let _guard = serial();
        UPDATES.lock().unwrap().clear();
        let mut engine = Engine::new_with_dirs(host(), fixture_dirs(), None, None);
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
        let mut engine = Engine::new_with_dirs(host(), fixture_dirs(), None, None);
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
        let mut engine = Engine::new_with_dirs(host(), fixture_dirs(), None, None);
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
        let mut engine = Engine::new_with_dirs(host(), fixture_dirs(), None, None);
        // symbols.yaml half_shape："." → 。
        assert!(engine.key(0x2e, 0, false), "period 应被消费");
        assert_eq!(COMMITS.lock().unwrap().last().unwrap(), "。");
    }

    #[test]
    fn punctuation_appends_to_composition() {
        let _guard = serial();
        COMMITS.lock().unwrap().clear();
        let mut engine = Engine::new_with_dirs(host(), fixture_dirs(), None, None);
        assert!(engine.key(u32::from(b'a'), 0, false));
        assert!(engine.key(u32::from(b'b'), 0, false));
        assert!(engine.key(0x2c, 0, false), "comma 应被消费");
        assert_eq!(COMMITS.lock().unwrap().last().unwrap(), "甲，");
        assert!(engine.context.input().is_empty());
    }

    #[test]
    fn punctuation_pair_alternates() {
        let _guard = serial();
        COMMITS.lock().unwrap().clear();
        let mut engine = Engine::new_with_dirs(host(), fixture_dirs(), None, None);
        // apostrophe：'‘' / '’'
        for text in ["‘", "’"] {
            assert!(engine.key(0x27, 0, false));
            assert_eq!(COMMITS.lock().unwrap().last().unwrap(), text);
        }
    }

    #[test]
    fn punctuation_passes_unmapped_space() {
        let _guard = serial();
        let mut engine = Engine::new_with_dirs(host(), fixture_dirs(), None, None);
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
        let mut engine = Engine::new_with_dirs(host(), fixture_dirs(), None, None);
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
        assert!(engine.context.input().is_empty(), "组合已提交并清空");
    }

    #[test]
    fn idle_uppercase_does_not_request_forward() {
        let _guard = serial();
        let mut engine = Engine::new_with_dirs(host(), fixture_dirs(), None, None);
        assert!(!engine.key(0x41, FCITX_SHIFT, false));
        assert!(!engine.forward_after_commit);
    }

    #[test]
    fn apply_settings_switches_context_options() {
        let _guard = serial();
        let mut engine = Engine::new_with_dirs(host(), fixture_dirs(), None, None);
        engine.apply_settings(Settings {
            full_shape: true,
            ascii_punct: true,
            ..Default::default()
        });
        assert!(engine.context.get_option("full_shape"));
        assert!(engine.context.get_option("ascii_punct"));
    }

    #[test]
    fn apply_settings_disables_learning_mode() {
        let _guard = serial();
        let mut engine = Engine::new_with_dirs(host(), fixture_dirs(), None, None);
        engine.apply_settings(Settings {
            tab_learning: false,
            ..Default::default()
        });
        assert!(
            engine.live.mode.is_empty(),
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
        }
    }

    #[test]
    fn ffi_apply_settings_maps_engine_options() {
        let _guard = serial();
        let engine = unsafe { hux_engine_new(std::ptr::null()) };
        assert!(!engine.is_null());
        let options = ffi_options();
        assert_eq!(unsafe { hux_engine_apply_settings(engine, &options) }, 1);
        let state = unsafe { &mut *engine };
        assert!(!state.settings.early_commit);
        assert!(state.context.get_option("full_shape"));
        assert!(state.context.get_option("ascii_punct"));
        assert!(
            state.live.mode.is_empty(),
            "tab_learning=0 → 学习 mode 为空"
        );
        assert_eq!(state.settings.high_freq_limit, 800);
        unsafe { hux_engine_free(engine) };
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
        assert_eq!(state.host_options.page_size, 7);
        assert_eq!(
            state.host_options.page_up_keys,
            vec![KeyEvent::from_repr("comma").unwrap()]
        );
        assert_eq!(
            state.host_options.page_down_keys,
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
        let mut engine = Engine::new_with_dirs(host(), fixture_dirs(), None, None);
        engine.key(u32::from(b'a'), 0, false);
        engine.reset();
        let (preedit, _, candidates, _, _, _) =
            UPDATES.lock().unwrap().last().cloned().expect("update");
        assert!(preedit.is_empty());
        assert!(candidates.is_empty());
        assert!(engine.context.input().is_empty());
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
        let mut engine = Engine::new_with_dirs(host(), dirs, None, None);
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
        let mut engine = Engine::new_with_dirs(host(), dirs, None, None);
        // 应用侧周边文本「中欧中兴」，光标在第 2 个字符后（锚点 = 2）。
        engine.set_surrounding(Some("中欧中兴"), 2);
        // 默认 Alt+"（带修饰）→ 组合无默认候选；上排「咅」、下排「虍」。
        assert!(engine.key(0x22, FCITX_ALT, false), "Alt+\" 应被消费");
        let (preedit, _, candidates, _, up, down) = last_update();
        assert_eq!(engine.context.input(), b"\"");
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
        assert_eq!(engine.context.input(), b"a");
        // 音反查：带修饰键（默认 Alt+:）**不给**默认候选；单字符键（;）才给。
        engine.reset();
        assert!(engine.key(0x3a, FCITX_ALT, false), "Alt+: 应被消费");
        let (_, _, candidates, _, _, _) = last_update();
        assert!(
            candidates.is_empty(),
            "带修饰触发键不给默认候选：{candidates:?}"
        );
        engine.reset();
        engine.apply_settings(settings::Settings {
            sound_to_char_shape_keys: vec!["semicolon".to_string()],
            ..settings::Settings::default()
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
        engine.apply_settings(settings::Settings {
            sound_to_char_shape_keys: vec!["grave".to_string()],
            ..settings::Settings::default()
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
        engine.apply_settings(settings::Settings {
            char_to_sound_shape_keys: vec!["asciitilde".to_string()],
            ..settings::Settings::default()
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
        let mut engine = Engine::new_with_dirs(host(), char_to_sound_shape_dirs(), None, None);
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
        let mut engine = Engine::new_with_dirs(host(), char_to_sound_shape_dirs(), None, None);
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
        let mut engine = Engine::new_with_dirs(host(), fixture_dirs(), None, None);
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
        let mut engine = Engine::new_with_dirs(host(), fixture_dirs(), None, None);
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
        let mut engine = Engine::new_with_dirs(host(), fixture_dirs(), None, None);
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
        let consumed = unsafe { hux_engine_key(engine, u32::from(b'a'), 0, 0) };
        assert_eq!(consumed & HUX_KEY_CONSUMED, HUX_KEY_CONSUMED);
        unsafe { hux_engine_free(engine) };
    }
}
