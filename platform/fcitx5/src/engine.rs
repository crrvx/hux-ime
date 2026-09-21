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
use hux_cfg::{CandidateLayout, OptionIds, OptionsStore, Settings};

use hux_core::key::KeyEvent;
use hux_core::scheme::{KeyOutcome, Scheme, SchemeConfig};
use hux_core::session::{Context, Event};
use hux_scheme_tiger::scheme::{ASSETS, TigerScheme};

pub(crate) fn wall_clock() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|value| value.as_secs() as f64)
        .unwrap_or(0.0)
}

impl Engine {
    /// 状态菜单可切换的运行时开关（顺序即菜单顺序）：方案声明的 4 项 + rime 标准的 `full_shape`。
    pub(crate) fn runtime_options(&self) -> [&'static str; 5] {
        [
            self.option_ids.early_commit,
            self.option_ids.early_commit_to_preedit,
            self.option_ids.allow_duplicate_single,
            "full_shape",
            self.option_ids.digit_select,
        ]
    }
}

/// hux 自身设置 → 方案配置（平台是装配根：只有这里知道两边字段的对应关系）。
pub(crate) fn scheme_config(settings: &Settings) -> SchemeConfig {
    let host = settings.host_options();
    SchemeConfig {
        high_freq_limit: settings.high_freq_limit,
        min_retained_raw_length: settings.min_retained(),
        page_size: host.page_size,
        page_cycle: host.page_cycle,
        page_up_keys: settings.page_up_keys.clone(),
        page_down_keys: settings.page_down_keys.clone(),
        sound_to_char_shape_keys: settings.sound_to_char_shape_keys.clone(),
        char_to_sound_shape_keys: settings.char_to_sound_shape_keys.clone(),
        tab_learning: settings.tab_learning,
    }
}

pub struct Engine {
    pub(crate) host: Option<HostCallback>,
    /// 方案（P4c：平台经 `dyn Scheme` 驱动，不直接引用方案模块；共享资源与会话态都在方案内）。
    pub(crate) scheme: Box<dyn Scheme>,
    pub(crate) sessions: HashMap<u64, Session>,
    pub(crate) next_session: u64,
    /// 选项存储（用户目录不可用时为 `None`，此时仅用内建缺省）。
    pub(crate) options: Option<OptionsStore>,
    /// 外部配置（fcitx5 配置界面 / 测试；默认 = 内建缺省）。
    pub(crate) settings: Settings,
    /// 学习库（用户目录不可用时为禁用占位）。
    pub(crate) learning: LearningStore,
    /// 方案声明的选项 id（P4 收尾：单一来源 = 方案；配置层与状态菜单白名单据此工作）。
    pub(crate) option_ids: OptionIds,
    /// 学习规则串（来自方案数据；用于拼 mode）。
    pub(crate) learning_rules: String,
    /// 当前学习 mode 串。
    pub(crate) learning_mode: String,
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
            .or_else(|| default_model_path(&dirs, ASSETS));
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
        let (mut scheme, scheme_notes) =
            TigerScheme::load(&dirs, model_path, scheme_config(&settings));
        notes.extend(scheme_notes);
        let learning_rules = scheme.learning_rules().to_string();
        let option_ids = scheme.option_ids();
        // 选项：有存储则同步（参照 `M.options.sync`，同步写入由核心抑制观察）；
        // 无存储时直接用内建缺省。会话创建时逐个同步（见 `session_new`）。
        let options = options_dir
            .as_deref()
            .map(|dir| OptionsStore::load_with_defaults(dir, settings.store_defaults(&option_ids)));
        // 学习库：`<user dir>/<方案 id 哈希>.userdb/`（用户目录不可用则禁用）。
        let learning = match options_dir.as_deref() {
            Some(dir) => {
                LearningStore::open(dir, &learning_store::store_name(scheme.id()), wall_clock())
            }
            None => LearningStore::disabled("user data directory unavailable"),
        };
        if let Some(error) = &learning.error {
            notes.push(format!("learning: {error}"));
        } else {
            notes.push(format!("learning: {}", learning.name));
        }
        scheme.set_store_ready(learning.store_ready());
        let learning_mode = scheme.learning_mode(
            &learning_rules,
            settings.allow_duplicate_single,
            settings.high_freq_limit,
        );
        scheme.set_learning_mode(&learning_mode);
        Self {
            host,
            scheme: Box::new(scheme),
            sessions: HashMap::new(),
            next_session: 1,
            options,
            settings,
            learning,
            option_ids,
            learning_rules,
            learning_mode,
            forward_after_commit: false,
            status: CString::new(notes.join("; ")).unwrap_or_default(),
        }
    }

    /// 新建会话（一个输入上下文注册/激活时创建）；选项/触发键/学习模式按共享状态初始化。
    pub fn session_new(&mut self) -> u64 {
        let mut context = Context::new();
        // 宿主缺省：`_auto_commit`（librime `express_editor` 默认 true）。
        context.set_option("_auto_commit", true);
        // 先写设置缺省，再由 `options.yaml` 持久化值覆盖。
        // 分流：**可持久化项**（存储有声明的缺省）只经 `store.sync` 写入——其写入带抑制名单，
        // 不会被随后的选项事件当成用户改动写进 `options.yaml`；其余（如 `ascii_punct`）直接写。
        // 参照实现同此：缺省经 `M.options.sync` 写入并由 `live.syncing` 抑制。
        for (name, value) in self.settings.option_defaults(&self.option_ids) {
            if self
                .options
                .as_ref()
                .is_some_and(|store| store.covers(name))
            {
                continue;
            }
            context.set_option(name, value);
        }
        if let Some(options) = self.options.as_mut() {
            options.sync(&mut context);
        }
        // 无存储时运行时开关（状态菜单）以现存会话为模板，避免新会话回退到设置缺省。
        if let Some(existing) = self.sessions.values().next() {
            for name in self.runtime_options() {
                let value = existing.context.get_option(name);
                if context.get_option(name) != value {
                    context.set_option(name, value);
                }
            }
        }
        let scheme_session = self.scheme.new_session(&mut context);
        let mut session = Session {
            context,
            scheme_session,
            char_to_sound_shape: CharToSoundShapeState::default(),
        };
        self.apply_layout_options(&mut session);
        let id = self.next_session;
        self.next_session += 1;
        self.sessions.insert(id, session);
        id
    }

    /// 释放会话（输入上下文销毁时）。
    pub fn session_free(&mut self, session_id: u64) {
        if let Some(session) = self.sessions.remove(&session_id) {
            self.scheme.free_session(session.scheme_session);
        }
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
            && self.scheme.auxiliary_lookup_active(&session.context)
            && matches!(key.repr().as_str(), "Left" | "Right" | "Up" | "Down")
        {
            return false;
        }
        let now = wall_clock();
        // 方案内部完成：处理器（翻译/锁/早提交）→ 未消费则宿主链（含提交点学习）。
        let consumed =
            match self
                .scheme
                .process_key(session.scheme_session, &mut session.context, &key, now)
            {
                Ok(KeyOutcome::Consumed) => true,
                Ok(KeyOutcome::Forward) => false,
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
        match self
            .scheme
            .select_candidate(session.scheme_session, &mut session.context, index, now)
        {
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
        let submitted = self.scheme.take_learning_events(session.scheme_session);
        if !submitted.is_empty() {
            self.learning.confirm(&submitted);
        }
        self.refresh_learning_mode();
        if !session.context.is_composing() {
            self.learning.refresh_scores(now);
        }
        let version = self.learning.index_version();
        let mode = self.learning_mode.clone();
        self.scheme.apply_learning_index(
            session.scheme_session,
            version,
            self.learning.index(),
            &mode,
        );
        // 组合重建（参照 `ConcreteEngine::Compose`，含 update 通知器）
        // （非组合清暂存；缓冲且实况输入为空时隐藏候选）。
        if let Err(error) =
            self.scheme
                .rebuild(session.scheme_session, &mut session.context, invalidated)
        {
            eprintln!("hux: rebuild error: {error}");
        }
        self.refresh_char_to_sound_shape_aux(session);
        self.push_update(session);
    }

    /// 当前组合末段是否为字反查段（进入/退出由方案处理器负责：触发字符推入/清空组合）；
    /// 查码段内方向键交应用处理（见 `key_in` 开头的早退）。
    /// 当前组合末段是否为字反查段。
    pub(crate) fn char_to_sound_shape_tagged(&self, session: &Session) -> bool {
        self.scheme.auxiliary_lookup_active(&session.context)
    }

    /// 重算两排提示（上排 = 光标左侧拼音、下排 = 虎码）；不在查码段则清空。
    pub(crate) fn refresh_char_to_sound_shape_aux(&mut self, session: &mut Session) {
        let tagged = self.char_to_sound_shape_tagged(session);
        let state = &mut session.char_to_sound_shape;
        if !tagged || !state.valid {
            // 不在查码段，或周边文本不可用（如终端）：提示能否呈现取决于前端，
            // 统一清空两排。
            state.aux_up.clear();
            state.aux_down.clear();
            return;
        }
        let (up, down) = self.scheme.auxiliary_rows(&state.text, state.cursor);
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
        self.scheme
            .reset_session(session.scheme_session, &mut session.context);
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
        if !self.runtime_options().contains(&name) {
            return None;
        }
        if let Some(session) = self.sessions.values().next() {
            return Some(session.context.get_option(name));
        }
        self.options
            .as_ref()
            .and_then(|store| store.value(name))
            .or_else(|| self.settings.option_default(&self.option_ids, name))
    }

    /// 设置运行时开关（状态菜单）：白名单校验 → 写入全部会话 → 持久化（`options.yaml`）。
    pub fn set_option_value(&mut self, name: &str, value: bool) -> bool {
        if !self.runtime_options().contains(&name) {
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
        self.scheme.apply_config(&scheme_config(&self.settings));
        let defaults = self.settings.option_defaults(&self.option_ids);
        if let Some(store) = self.options.as_mut() {
            store.set_defaults(self.settings.store_defaults(&self.option_ids));
        }
        let ids: Vec<u64> = self.sessions.keys().copied().collect();
        for id in ids {
            self.with_session(id, |engine, session| {
                // 同 `session_new`：可持久化项交由 `store.sync`（带抑制），其余直接写。
                for (name, value) in &defaults {
                    if engine
                        .options
                        .as_ref()
                        .is_some_and(|store| store.covers(name))
                    {
                        continue;
                    }
                    if session.context.get_option(name) != *value {
                        session.context.set_option(name, *value);
                    }
                }
                if let Some(store) = engine.options.as_mut() {
                    store.sync(&mut session.context);
                }
                engine.apply_layout_options(session);
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

    /// 按当前规则/选项刷新学习 mode（变化时方案会强制重设解码器学习并同步全部会话）。
    pub(crate) fn refresh_learning_mode(&mut self) {
        let duplicate = self
            .option_value(self.option_ids.allow_duplicate_single)
            .unwrap_or(true);
        let rules = self.learning_rules.clone();
        let mode = self
            .scheme
            .learning_mode(&rules, duplicate, self.settings.high_freq_limit);
        self.learning_mode = mode.clone();
        self.scheme.set_learning_mode(&mode);
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
