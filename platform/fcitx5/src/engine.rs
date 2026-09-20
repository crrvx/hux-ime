// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! 引擎：共享数据（词库/解码/选项/学习库）+ 按输入上下文隔离的多会话。

use std::collections::HashMap;
use std::ffi::CString;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::abi::{HostCallback, core_modifiers};
use crate::learning_store::{self, LearningStore};
use crate::paths::{data_dirs, default_model_path, user_data_dir};
use crate::session::{CharToSoundShapeState, Session};
use hux_cfg::{CandidateLayout, OptionsStore, Settings};

use hux_core::char_to_sound_shape;
use hux_core::decode::Decoder;
use hux_core::host::{self, HostOptions, HostResult};
use hux_core::interaction::{
    CompositionBuilder, K_CHAR_TO_SOUND_SHAPE_KEY, K_SOUND_TO_CHAR_SHAPE_KEY, LearningCommit,
    LiveLearning, OPTION_ALLOW_DUPLICATE_SINGLE, OPTION_DIGIT_SELECT, OPTION_EARLY_COMMIT,
    OPTION_EARLY_COMMIT_TO_PREEDIT, ProcessorEnv, ProcessorResult, SentenceState, processor,
    reset_early_evidence, select_candidate_at, update_notifier,
};
use hux_core::key::KeyEvent;
use hux_core::lexical;
use hux_core::lexicon::{LEXICAL_FILE, Lexicon, Supplement, candidate_paths};
use hux_core::ngram::MobileModel;
use hux_core::punct::PunctTable;
use hux_core::session::{Context, Event};

pub(crate) fn wall_clock() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|value| value.as_secs() as f64)
        .unwrap_or(0.0)
}

pub(crate) const RUNTIME_OPTIONS: [&str; 5] = [
    OPTION_EARLY_COMMIT,
    OPTION_EARLY_COMMIT_TO_PREEDIT,
    OPTION_ALLOW_DUPLICATE_SINGLE,
    "full_shape",
    OPTION_DIGIT_SELECT,
];

pub struct Engine {
    pub(crate) host: Option<HostCallback>,
    pub(crate) decoder: Decoder,
    pub(crate) sessions: HashMap<u64, Session>,
    pub(crate) next_session: u64,
    /// 选项存储（用户目录不可用时为 `None`，此时仅用内建缺省）。
    pub(crate) options: Option<OptionsStore>,
    /// 外部配置（fcitx5 配置界面 / 测试；默认 = 内建缺省）。
    pub(crate) settings: Settings,
    /// 宿主选项（翻页键/页大小；由 `settings` 派生，避免每次按键解析键名）。
    pub(crate) host_options: HostOptions,
    /// 标点表（`symbols.yaml`；缺失时标点交宿主）。
    pub(crate) punct: Option<PunctTable>,
    /// 学习库（用户目录不可用时为禁用占位）。
    pub(crate) learning: LearningStore,
    /// 学习规则串（来自码表；用于拼 mode）。
    pub(crate) learning_rules: String,
    /// 当前学习 mode 串与已应用的索引版本。
    pub(crate) learning_mode: String,
    pub(crate) applied_learning: Option<u64>,
    /// 本次按键「已提交且未消费」：宿主层应消费该键并以 `forwardKey` 重发，
    /// 保证「提交 → 按键」送达顺序（对齐 fcitx5 核心 `KeyEventOrderFix` 修法）。
    pub forward_after_commit: bool,
    pub(crate) status: CString,
}
impl Engine {
    pub(crate) fn new(host: Option<HostCallback>) -> Self {
        let dirs = data_dirs();
        let model = std::env::var_os("HUX_MODEL")
            .map(PathBuf::from)
            .or_else(|| default_model_path(&dirs));
        // 选项存于标准用户目录（与数据目录的开发覆盖解耦）。
        let options_dir = user_data_dir();
        Self::new_with_dirs(host, dirs, model, options_dir)
    }

    pub(crate) fn new_with_dirs(
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
        // 选项：有存储则同步（参照 `M.options.sync`，同步写入由核心抑制观察）；
        // 无存储时直接用内建缺省。会话创建时逐个同步（见 `session_new`）。
        let options = options_dir
            .as_deref()
            .map(|dir| OptionsStore::load_with_defaults(dir, settings.store_defaults()));
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
        let learning_mode =
            settings.learning_mode(&learning_rules, u8::from(settings.allow_duplicate_single));
        Self {
            host,
            decoder,
            sessions: HashMap::new(),
            next_session: 1,
            options,
            settings,
            host_options,
            punct,
            learning,
            learning_rules,
            learning_mode,
            applied_learning: None,
            forward_after_commit: false,
            status: CString::new(notes.join("; ")).unwrap_or_default(),
        }
    }

    /// 新建会话（一个输入上下文注册/激活时创建）；选项/触发键/学习模式按共享状态初始化。
    pub fn session_new(&mut self) -> u64 {
        let mut context = Context::new();
        // 宿主缺省：`_auto_commit`（librime `express_editor` 默认 true）。
        context.set_option("_auto_commit", true);
        // 先写设置缺省（含不持久化的 `ascii_punct`），再由 `options.yaml` 持久化值覆盖。
        for (name, value) in self.settings.option_defaults() {
            context.set_option(name, value);
        }
        if let Some(options) = self.options.as_mut() {
            options.sync(&mut context);
        }
        // 无存储时运行时开关（状态菜单）以现存会话为模板，避免新会话回退到设置缺省。
        if let Some(existing) = self.sessions.values().next() {
            for name in RUNTIME_OPTIONS {
                let value = existing.context.get_option(name);
                if context.get_option(name) != value {
                    context.set_option(name, value);
                }
            }
        }
        let mut session = Session {
            context,
            state: SentenceState::fresh(1),
            live: LiveLearning {
                mode: self.learning_mode.clone(),
                store_ready: self.learning.store_ready(),
                ..LiveLearning::default()
            },
            dot_armed: false,
            min_retained: Some(self.settings.min_retained() as i64),
            builder: CompositionBuilder::default(),
            char_to_sound_shape: CharToSoundShapeState::default(),
        };
        self.apply_trigger_keys(&mut session);
        self.apply_layout_options(&mut session);
        let id = self.next_session;
        self.next_session += 1;
        self.sessions.insert(id, session);
        id
    }

    /// 释放会话（输入上下文销毁时）。
    pub fn session_free(&mut self, session_id: u64) {
        self.sessions.remove(&session_id);
    }

    /// 取会话执行闭包后放回（借用拆分：会话与共享状态互不重叠）。
    pub(crate) fn with_session<R>(
        &mut self,
        session_id: u64,
        f: impl FnOnce(&mut Self, &mut Session) -> R,
    ) -> Option<R> {
        let mut session = self.sessions.remove(&session_id)?;
        let result = f(self, &mut session);
        self.sessions.insert(session_id, session);
        Some(result)
    }

    /// 处理一次按键：返回是否消费；副作用（提交/preedit/候选）经宿主回调送出。
    ///
    /// 提交且未消费（`express_editor` 的 `char_handler = DirectCommit`）时置
    /// [`Engine::forward_after_commit`]：宿主层据此消费该键并以 `forwardKey` 重发，
    /// 保证客户端先收到提交、后收到按键。
    pub fn key(&mut self, session_id: u64, keysym: u32, states: u32, release: bool) -> bool {
        self.with_session(session_id, |engine, session| {
            engine.key_in(session, keysym, states, release)
        })
        .unwrap_or(false)
    }

    pub(crate) fn key_in(
        &mut self,
        session: &mut Session,
        keysym: u32,
        states: u32,
        release: bool,
    ) -> bool {
        self.forward_after_commit = false;
        let key = KeyEvent::new(keysym as i32, core_modifiers(states, release));
        // 字反查段：←/→/↑/↓ **交应用处理**（应用光标随动），本层不消费也不改动输入；
        // 两排在应用回传周边文本后的下一次按键（含 release）时刷新。
        if !release
            && self.char_to_sound_shape_tagged(session)
            && matches!(key.repr().as_str(), "Left" | "Right" | "Up" | "Down")
        {
            return false;
        }
        let now = wall_clock();
        let result = {
            let mut env = ProcessorEnv {
                now,
                dot_armed: &mut session.dot_armed,
                min_retained: session.min_retained,
                page_size: self.host_options.page_size,
            };
            processor(
                &key,
                &mut session.context,
                &mut session.state,
                &mut self.decoder,
                &mut session.live,
                &mut env,
            )
        };
        let consumed = match result {
            Ok(ProcessorResult::Consume) => true,
            // 参照链：处理器未消费的键交宿主等价物（selector/navigator/express_editor 等）。
            Ok(ProcessorResult::Forward) => {
                let mut learning = LearningCommit {
                    decoder: &mut self.decoder,
                    live: &mut session.live,
                    now,
                };
                host::process_key(
                    &key,
                    &mut session.context,
                    &session.state,
                    self.punct.as_mut(),
                    &self.host_options,
                    Some(&mut learning),
                ) == HostResult::Consumed
            }
            Err(error) => {
                eprintln!("hux: processor error: {error}");
                false
            }
        };
        self.finish(session, now, Some(consumed));
        consumed
    }

    /// 候选点击（面板候选 `CandidateWord::select`，按会话）：按全局索引选中并上屏。
    /// 走与 `space` 相同的确认/学习链；越界/无可选段返回 `false`。
    pub fn select_candidate(&mut self, session_id: u64, index: usize) -> bool {
        self.with_session(session_id, |engine, session| {
            engine.select_candidate_in(session, index)
        })
        .unwrap_or(false)
    }

    pub(crate) fn select_candidate_in(&mut self, session: &mut Session, index: usize) -> bool {
        self.forward_after_commit = false;
        let now = wall_clock();
        match select_candidate_at(
            &mut self.decoder,
            &mut session.context,
            &mut session.state,
            &mut session.live,
            now,
            index,
        ) {
            Ok(true) => {}
            Ok(false) => return false,
            Err(error) => {
                eprintln!("hux: select candidate error: {error}");
                return false;
            }
        }
        self.finish(session, now, None);
        true
    }

    /// 提交泵 + 学习落库 + 组合重建 + UI 刷新（按键与候选点击共用）。
    ///
    /// `key_forward`：按键路径传入消费结果（据提交计算 `forward_after_commit`）；
    /// 候选点击传 `None`（非按键路径，恒不转发）。
    pub(crate) fn finish(&mut self, session: &mut Session, now: f64, key_forward: Option<bool>) {
        let mut commits = Vec::new();
        let mut invalidated = false;
        // 事件泵：选项事件可能触发确认（进而产生提交），循环至排空（有界）。
        for _ in 0..4 {
            let events = session.context.drain_events();
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
                        self.observe_option(&mut session.context, &name);
                    }
                    Event::Update => {}
                }
            }
        }
        let committed = !commits.is_empty();
        for text in commits {
            self.host_commit(&text);
        }
        self.forward_after_commit = match key_forward {
            Some(consumed) => !consumed && committed,
            None => false,
        };
        // 学习：核心暂存 → 落库；刷新打分（未组合时，60 秒节流）；应用索引。
        let submitted = std::mem::take(&mut session.live.submitted);
        if !submitted.is_empty() {
            self.learning.confirm(&submitted);
        }
        self.refresh_learning_mode();
        if !session.context.is_composing() {
            self.learning.refresh_scores(now);
        }
        self.apply_learning(session);
        // 组合重建（参照 `ConcreteEngine::Compose`，先于通知器）与 update 通知器
        // （非组合清暂存；缓冲且实况输入为空时隐藏候选）。
        if let Err(error) = session.builder.rebuild(
            &mut self.decoder,
            &mut session.context,
            &session.state,
            invalidated,
            self.punct.as_mut(),
        ) {
            eprintln!("hux: rebuild error: {error}");
        }
        update_notifier(&mut session.context, &mut session.state, &mut session.live);
        self.refresh_char_to_sound_shape_aux(session);
        self.push_update(session);
    }

    /// 字反查（⑧-2）：查码段内 ←/→ 以 2 字符步长移动锚点（返回 `Some(true)` 消费）。
    /// 进入/退出查码段由 core 处理器负责（触发字符推入/清空组合）。
    /// 当前组合末段是否为字反查段。
    pub(crate) fn char_to_sound_shape_tagged(&self, session: &Session) -> bool {
        session
            .context
            .composition
            .back()
            .is_some_and(|segment| segment.has_tag(char_to_sound_shape::TAG))
    }

    /// 重算两排提示（上排 = 光标左侧拼音、下排 = 虎码）；不在查码段则清空。
    pub(crate) fn refresh_char_to_sound_shape_aux(&mut self, session: &mut Session) {
        let tagged = self.char_to_sound_shape_tagged(session);
        let state = &mut session.char_to_sound_shape;
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
    pub fn set_surrounding(&mut self, session_id: u64, text: Option<&str>, cursor_chars: usize) {
        self.with_session(session_id, |engine, session| {
            engine.set_surrounding_in(session, text, cursor_chars);
        });
    }

    pub(crate) fn set_surrounding_in(
        &mut self,
        session: &mut Session,
        text: Option<&str>,
        cursor_chars: usize,
    ) {
        let state = &mut session.char_to_sound_shape;
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
        if self.char_to_sound_shape_tagged(session) {
            self.refresh_char_to_sound_shape_aux(session);
        }
    }

    /// 重置会话（`deactivate`/`reset`；组合不跨输入上下文保留）。
    pub fn reset(&mut self, session_id: u64) {
        self.with_session(session_id, |engine, session| engine.reset_in(session));
    }

    pub(crate) fn reset_in(&mut self, session: &mut Session) {
        // 契约：重置即丢弃——先清掉未派发的事件（含可能的提交），避免下次按键补上屏。
        session.context.drain_events();
        session.char_to_sound_shape = CharToSoundShapeState::default();
        session.context.clear();
        session.state.reset(&mut session.context, false);
        session.live.pending.clear();
        session.live.baseline = None;
        session.live.submitted_raw = None;
        session.dot_armed = false;
        session.builder.reset();
        if let Some(options) = self.options.as_mut() {
            options.sync(&mut session.context);
        }
        self.push_update(session);
    }

    /// 选项事件 → 记录/持久化（参照 `M.options` 的选项通知器）。
    pub(crate) fn observe_option(&mut self, context: &mut Context, name: &str) {
        if let Some(options) = self.options.as_mut() {
            options.observe(context, name);
        }
    }

    /// 运行时开关当前值（状态菜单；全局）：任一会话的生效值，无会话时回退存储/设置缺省。
    pub fn option_value(&self, name: &str) -> Option<bool> {
        if !RUNTIME_OPTIONS.contains(&name) {
            return None;
        }
        if let Some(session) = self.sessions.values().next() {
            return Some(session.context.get_option(name));
        }
        self.options
            .as_ref()
            .and_then(|store| store.value(name))
            .or_else(|| self.settings.option_default(name))
    }

    /// 设置运行时开关（状态菜单）：白名单校验 → 写入全部会话 → 持久化（`options.yaml`）。
    pub fn set_option_value(&mut self, name: &str, value: bool) -> bool {
        if !RUNTIME_OPTIONS.contains(&name) {
            return false;
        }
        let ids: Vec<u64> = self.sessions.keys().copied().collect();
        for id in ids {
            self.with_session(id, |engine, session| {
                if session.context.get_option(name) != value {
                    session.context.set_option(name, value);
                }
                engine.observe_option(&mut session.context, name);
            });
        }
        self.refresh_learning_mode();
        true
    }

    /// 应用外部配置（fcitx5 配置界面 / 测试）：选项类即时生效；`high_freq_limit` 需重启。
    /// 顺序：设置写入缺省 → `options.yaml` 持久化值覆盖（含状态菜单开关）→ 触发键/学习模式刷新。
    /// 作用于全部会话（选项为引擎级）。
    pub fn apply_settings(&mut self, settings: Settings) {
        self.settings = settings;
        self.host_options = self.settings.host_options();
        let defaults = self.settings.option_defaults();
        if let Some(store) = self.options.as_mut() {
            store.set_defaults(self.settings.store_defaults());
        }
        let ids: Vec<u64> = self.sessions.keys().copied().collect();
        for id in ids {
            self.with_session(id, |engine, session| {
                for (name, value) in &defaults {
                    if session.context.get_option(name) != *value {
                        session.context.set_option(name, *value);
                    }
                }
                if let Some(store) = engine.options.as_mut() {
                    store.sync(&mut session.context);
                }
                engine.apply_trigger_keys(session);
                engine.apply_layout_options(session);
                session.min_retained = Some(engine.settings.min_retained() as i64);
            });
        }
        self.refresh_learning_mode();
    }

    /// 候选布局（context 选项）：host `selector` 读取 `_vertical` 决定 ←→/↑↓ 语义。
    /// 仅「竖排」置位；「横排/跟随全局」维持横排键语义（面板排列见 C++ `LayoutHint`）。
    pub(crate) fn apply_layout_options(&self, session: &mut Session) {
        let vertical = self.settings.candidate_layout == CandidateLayout::Vertical;
        if session.context.get_option("_vertical") != vertical {
            session.context.set_option("_vertical", vertical);
        }
    }

    /// 触发键（属性）：把两项触发键的 rime 键名列表（逗号分隔）交给 core（解析/匹配均在 core 内）。
    pub(crate) fn apply_trigger_keys(&self, session: &mut Session) {
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
            if session.context.get_property(property).unwrap_or("") != value {
                session.context.set_property(property, &value);
            }
        }
    }

    /// 按当前规则/选项刷新学习 mode（变化时强制重设 decoder 学习；同步到全部会话）。
    pub(crate) fn refresh_learning_mode(&mut self) {
        let mode = self.settings.learning_mode(
            &self.learning_rules,
            u8::from(
                self.option_value(OPTION_ALLOW_DUPLICATE_SINGLE)
                    .unwrap_or(true),
            ),
        );
        if mode != self.learning_mode {
            self.learning_mode = mode.clone();
            for session in self.sessions.values_mut() {
                session.live.mode = mode.clone();
            }
            self.applied_learning = None;
        }
    }

    /// 索引变化时重设 decoder 学习（参照 `active_index` 变化：重置早证据与空码态）。
    pub(crate) fn apply_learning(&mut self, session: &mut Session) {
        let version = self.learning.index_version();
        if self.applied_learning == Some(version) {
            return;
        }
        self.applied_learning = Some(version);
        self.decoder
            .set_learning(self.learning.index().clone(), &self.learning_mode);
        reset_early_evidence(&mut session.state);
        session.state.empty_code_pending = None;
    }

    /// 提交回调（`engine:commit_text`）。
    pub(crate) fn host_commit(&self, text: &str) {
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
}
