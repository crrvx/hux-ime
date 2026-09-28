// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! 虎句方案对 [`hux_core::scheme::Scheme`] 的实现。
//!
//! 共享资源（解码器 / 标点表）与每会话状态（组合、学习暂存、锁、早提交、附件态）
//! 都集中在本类型内，平台只持有 `Box<dyn Scheme>` 与不透明的 [`SessionId`]；
//! 平台原先直接编排的处理器 + 宿主链 + 重建 + 证据流程，收在这里。

use hashbrown::HashMap;
use std::path::PathBuf;

use hux_core::host::HostOptions;
use hux_core::key::KeyEvent;
use hux_core::learning::{Event, LearningIndex};
use hux_core::punct::PunctTable;
use hux_core::scheme::{
    ConfigError, KeyOutcome, OptionDecl, Scheme, SchemeConfig, SessionId, asset_paths,
};
use hux_core::session::Context;

use crate::char_to_sound_shape;
use crate::decode::Decoder;
use crate::interaction::{
    CompositionBuilder, LiveLearning, ProcessorEnv, ProcessorResult, SentenceState, buffered_text,
    process_key_event, reset_early_evidence, select_candidate_at, update_notifier,
};
use crate::lexical;
use crate::lexicon::{LEXICAL_FILE, Lexicon, Supplement};
use crate::ngram::MobileModel;

mod assets;
mod config;
mod wiring;

pub use assets::{ASSETS, SCHEME_CONFIG_ROLES, SCHEME_ID};
use assets::{OPTION_DECLARATIONS, SYMBOLS_FILE};
use config::{Config, config_diagnostics};
use wiring::{forward_to_host, supplement_dir, sync_trigger_keys};

/// 每会话状态（原先由平台 `Session` 持有）。
struct TigerSession {
    state: SentenceState,
    live: LiveLearning,
    dot_armed: bool,
    min_retained: usize,
    builder: CompositionBuilder,
}

/// 虎句方案：引擎级共享资源 + 会话集合。
pub struct TigerScheme {
    decoder: Decoder,
    punct: Option<PunctTable>,
    config: Config,
    host_options: HostOptions,
    learning_rules: String,
    learning_mode: String,
    store_ready: bool,
    /// 已应用的索引版本（变化时重设解码器学习并重置证据）。
    applied_learning: Option<u64>,
    sessions: HashMap<u64, TigerSession>,
    next_session: u64,
    /// 模型装载的**菜单短名**（宿主首项「虎虚：」后接的那段；见 [`crate::model_status`]）。
    model_info: String,
    /// 模型装载的**详细摘要**（格式标签 / 失败原因；随状态串落日志，不进菜单）。
    model_detail: String,
}

impl TigerScheme {
    /// 加载方案数据（词库 / 补充 / 模型 / 词先验 / 标点表）；`notes` 汇总加载状态。
    ///
    /// 模型路径由平台解析后传入（`HUX_MODEL` 覆盖与默认查找都在平台侧）。
    pub fn load(
        dirs: &[PathBuf],
        model_path: Option<PathBuf>,
        config: &SchemeConfig,
    ) -> (Self, Vec<String>) {
        // 角色漂移 / 类型诊断先于解析：装袋方与读袋方的角色名必须同值，
        // 且每个角色都必须能按声明类型读出（见 `config_diagnostics`）。
        let (parsed, config_errors) = Config::parse(config);
        let mut notes = config_diagnostics(config, &config_errors);
        let config = parsed;
        let lexicon = Lexicon::load_with(dirs, config.high_freq_limit, config.lexicon_options());
        notes.push(format!("lexicon: {}", lexicon.data_status().canonical()));
        let learning_rules = lexicon.learning_rules.clone();
        let supplement = Supplement::load_default(supplement_dir(dirs).as_deref());
        // 模型状态（宿主状态菜单「模型」项）：文件名 / 格式标签 / 装载结果，见 `model_status`。
        let mut model_status = crate::model_status::ModelStatus::not_found();
        let model = model_path.and_then(|path| match MobileModel::load(&path, None) {
            Ok(model) => {
                model_status.record_loaded(&path);
                Some(model)
            }
            Err(error) => {
                notes.push(format!("model: {error}"));
                model_status.record_failed(&path, error.to_string());
                None
            }
        });
        let mut decoder = Decoder::new(lexicon, supplement, model);
        let (lexical_model, lexical_error) = lexical::load_first(&asset_paths(dirs, LEXICAL_FILE));
        decoder.set_lexical_model(lexical_model);
        if let Some(error) = lexical_error {
            notes.push(format!("lexical: {error}"));
        }
        let (punct, punct_error) = PunctTable::load_first(&asset_paths(dirs, SYMBOLS_FILE));
        if punct.is_none()
            && let Some(error) = punct_error
        {
            notes.push(format!("punct: {error}"));
        }
        let host_options = config.host_options();
        let mut scheme = Self {
            decoder,
            punct,
            config,
            host_options,
            learning_rules,
            learning_mode: String::new(),
            store_ready: false,
            applied_learning: None,
            sessions: HashMap::new(),
            next_session: 1,
            model_info: model_status.short_summary(),
            model_detail: model_status.summary(),
        };
        // 构造即自算学习 mode（平台随后下发的配置袋与之一致，不会造成 mode 抖动）。
        scheme.learning_mode = scheme.mode_from_config();
        (scheme, notes)
    }

    fn session_mut(&mut self, session: SessionId) -> Option<&mut TigerSession> {
        self.sessions.get_mut(&session.0)
    }
}

impl Scheme for TigerScheme {
    fn id(&self) -> &'static str {
        SCHEME_ID
    }

    fn option_declarations(&self) -> &'static [OptionDecl] {
        OPTION_DECLARATIONS
    }

    fn learning_mode(&self) -> &str {
        &self.learning_mode
    }

    fn model_info(&self) -> &str {
        &self.model_info
    }

    fn model_detail(&self) -> &str {
        &self.model_detail
    }

    fn data_info(&self) -> String {
        self.decoder.lexicon().data_info()
    }

    fn apply_config(&mut self, config: &SchemeConfig) -> Result<(), Vec<ConfigError>> {
        let (parsed, errors) = Config::parse(config);
        self.config = parsed;
        self.host_options = self.config.host_options();
        // 高频字上限 / 字集开关改变 ⇒ 重建词库索引（参照 `M.apply_high_freq_limit`）。
        // 平台在装配方案**之后**才下发配置页设置，故这里必须能重建；只在真的变化时重建
        // （每次按键路径都会经 `push_scheme_config` 走到本函数）。
        let limit = self.config.high_freq_limit;
        let options = self.config.lexicon_options();
        let lexicon = self.decoder.lexicon();
        if lexicon.high_freq_limit != limit || lexicon.options() != options {
            self.decoder.apply_lexicon_options(limit, options);
        }
        // 学习 mode 的输入都在配置袋里（Tab 学习 / 高频上限 / 单字重码选项值），
        // 由方案自算：变化时同步全部会话并重置解码器的学习索引（旧 mode 的记录不再命中）。
        let mode = self.mode_from_config();
        let changed = mode != self.learning_mode;
        if changed {
            self.learning_mode = mode;
            self.applied_learning = None;
        }
        let min_retained = self.config.min_retained_input_length;
        for session in self.sessions.values_mut() {
            session.min_retained = min_retained;
            if changed {
                session.live.mode = self.learning_mode.clone();
            }
        }
        // 诊断回给平台（进状态串）：装袋侧改名 / 类型不符不再静默回退。
        // 「未识别角色」由装配期的 `config_diagnostics` 点名（袋的角色集合归装配方），
        // 运行期重新下发时逐角色诊断即可覆盖同一类漂移。
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }

    fn host_options(&self) -> &HostOptions {
        &self.host_options
    }

    fn set_store_ready(&mut self, ready: bool) {
        self.store_ready = ready;
        for session in self.sessions.values_mut() {
            session.live.store_ready = ready;
        }
    }

    fn apply_learning_index(&mut self, session: SessionId, version: u64, index: &LearningIndex) {
        if self.applied_learning == Some(version) {
            return;
        }
        self.applied_learning = Some(version);
        let mode = self.learning_mode.clone();
        self.decoder.set_learning(index.clone(), &mode);
        if let Some(state) = self.session_mut(session) {
            reset_early_evidence(&mut state.state);
            state.state.empty_code_pending = None;
        }
    }

    fn new_session(&mut self, context: &mut Context) -> SessionId {
        sync_trigger_keys(context, &self.config);
        let id = self.next_session;
        self.next_session += 1;
        self.sessions.insert(
            id,
            TigerSession {
                state: SentenceState::fresh(1),
                live: LiveLearning {
                    mode: self.learning_mode.clone(),
                    store_ready: self.store_ready,
                    ..LiveLearning::default()
                },
                dot_armed: false,
                min_retained: self.config.min_retained_input_length,
                builder: CompositionBuilder::default(),
            },
        );
        SessionId(id)
    }

    fn free_session(&mut self, session: SessionId) {
        self.sessions.remove(&session.0);
    }

    fn reset_session(&mut self, session: SessionId, context: &mut Context) {
        let Some(state) = self.session_mut(session) else {
            return;
        };
        state.state.reset(context, false);
        state.live.pending.clear();
        state.live.baseline = None;
        state.live.submitted_raw = None;
        state.dot_armed = false;
        state.builder.reset();
    }

    fn process_key(
        &mut self,
        session: SessionId,
        context: &mut Context,
        key: &KeyEvent,
        now: f64,
    ) -> Result<KeyOutcome, String> {
        // 字反查段：←/→/↑/↓ **交应用处理**（应用光标随动），本层不消费也不改动输入。
        if char_to_sound_shape::navigation_forwarded(key, context) {
            return Ok(KeyOutcome::Forward);
        }
        sync_trigger_keys(context, &self.config);
        let Self {
            decoder,
            punct,
            config,
            host_options,
            sessions,
            ..
        } = self;
        let Some(state) = sessions.get_mut(&session.0) else {
            return Ok(KeyOutcome::Forward);
        };
        let mut env = ProcessorEnv {
            now,
            dot_armed: &mut state.dot_armed,
            min_retained: state.min_retained,
            page_size: config.page_size(),
            // 与紧随其后的宿主链共用同一份翻页键绑定（有意偏离上游，见 `ProcessorEnv`）。
            host_options: &*host_options,
        };
        let result = process_key_event(
            key,
            context,
            &mut state.state,
            decoder,
            &mut state.live,
            &mut env,
        )
        .map_err(|error| error.to_string())?;
        match result {
            ProcessorResult::Consume => Ok(KeyOutcome::Consumed),
            ProcessorResult::Forward => Ok(forward_to_host(
                key,
                context,
                punct.as_ref(),
                host_options,
                decoder,
                state,
                now,
            )),
        }
    }

    fn select_candidate(
        &mut self,
        session: SessionId,
        context: &mut Context,
        index: usize,
        now: f64,
    ) -> Result<bool, String> {
        let Self {
            decoder, sessions, ..
        } = self;
        let Some(state) = sessions.get_mut(&session.0) else {
            return Ok(false);
        };
        select_candidate_at(
            decoder,
            context,
            &mut state.state,
            &mut state.live,
            now,
            index,
        )
        .map_err(|error| error.to_string())
    }

    fn rebuild(
        &mut self,
        session: SessionId,
        context: &mut Context,
        invalidated: bool,
    ) -> Result<(), String> {
        sync_trigger_keys(context, &self.config);
        let Self {
            decoder,
            punct,
            sessions,
            ..
        } = self;
        let Some(state) = sessions.get_mut(&session.0) else {
            return Ok(());
        };
        state
            .builder
            .rebuild(decoder, context, &state.state, invalidated, punct.as_ref())
            .map_err(|error| error.to_string())?;
        update_notifier(context, &mut state.state, &mut state.live);
        Ok(())
    }

    fn take_learning_events(&mut self, session: SessionId) -> Vec<Event> {
        self.session_mut(session)
            .map(|state| std::mem::take(&mut state.live.submitted))
            .unwrap_or_default()
    }

    fn buffered_text(&self, context: &Context) -> String {
        buffered_text(context)
    }

    fn auxiliary_lookup_active(&self, context: &Context) -> bool {
        char_to_sound_shape::tagged(context)
    }

    fn auxiliary_rows(&mut self, text: &str, cursor_chars: usize) -> (String, String) {
        self.decoder
            .char_to_sound_shape_rows(text, cursor_chars)
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests;
