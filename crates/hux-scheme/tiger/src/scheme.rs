// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! 虎句方案对 [`hux_core::scheme::Scheme`] 的实现（P4c）。
//!
//! 共享资源（解码器 / 标点表）与每会话状态（组合、学习暂存、锁、早提交、附件态）
//! 都集中在本类型内，平台只持有 `Box<dyn Scheme>` 与不透明的 [`SessionId`]；
//! 平台原先直接编排的处理器 + 宿主链 + 重建 + 证据流程，收在这里。

use hashbrown::HashMap;
use std::path::{Path, PathBuf};

use hux_core::host::{self, HostOptions, MAX_PAGE_SIZE};
use hux_core::key::KeyEvent;
use hux_core::learning::{Event, LearningIndex};
use hux_core::punct::PunctTable;
use hux_core::scheme::{
    Asset, AssetKind, KeyOutcome, OptionIds, Scheme, SchemeConfig, SessionId, asset_paths,
    find_asset,
};
use hux_core::session::Context;

use crate::char_to_sound_shape;
use crate::decode::Decoder;
use crate::interaction::{
    CompositionBuilder, HostCommitObserver, K_CHAR_TO_SOUND_SHAPE_KEY, K_SOUND_TO_CHAR_SHAPE_KEY,
    LiveLearning, OPTION_ALLOW_DUPLICATE_SINGLE, OPTION_DIGIT_SELECT, OPTION_EARLY_COMMIT,
    OPTION_EARLY_COMMIT_TO_PREEDIT, ProcessorEnv, ProcessorResult, SentenceState, buffered_text,
    processor, reset_early_evidence, select_candidate_at, set_property_if_changed, update_notifier,
};
use crate::lexical;
use crate::lexicon::{LEXICAL_FILE, Lexicon, MODEL_PATH, Supplement};
use crate::ngram::MobileModel;

/// 方案标识（与上游数据互通；学习库命名沿用）。
pub const SCHEME_ID: &str = "tiger_sentence";
/// 码表 / 字频 / 白名单 / 补充 / 反查索引 / 标点表的文件名（平台记日志与打包用）。
const CODES_FILE: &str = "tiger_sentence.codes.txt";
const RANKS_FILE: &str = "tiger_sentence.char_ranks.txt";
const WHITELIST_FILE: &str = "tiger_sentence.full_code_whitelist.txt";
const SUPPLEMENT_FILE: &str = "tiger_sentence.supplement.txt";
const PINYIN_FILE: &str = "tiger_sentence.pinyin.bin.gz";
const SYMBOLS_FILE: &str = "symbols.yaml";

/// 数据资产清单（相对数据目录）。
pub const ASSETS: &[Asset] = &[
    Asset {
        kind: AssetKind::Model,
        file: MODEL_PATH,
    },
    Asset {
        kind: AssetKind::Data,
        file: CODES_FILE,
    },
    Asset {
        kind: AssetKind::Data,
        file: RANKS_FILE,
    },
    Asset {
        kind: AssetKind::Data,
        file: WHITELIST_FILE,
    },
    Asset {
        kind: AssetKind::Data,
        file: SUPPLEMENT_FILE,
    },
    Asset {
        kind: AssetKind::Data,
        file: LEXICAL_FILE,
    },
    Asset {
        kind: AssetKind::Data,
        file: PINYIN_FILE,
    },
    Asset {
        kind: AssetKind::Data,
        file: SYMBOLS_FILE,
    },
];

/// 每会话状态（原先由平台 `Session` 持有）。
struct TigerSession {
    state: SentenceState,
    live: LiveLearning,
    dot_armed: bool,
    min_retained: Option<i64>,
    builder: CompositionBuilder,
}

/// 虎句方案：引擎级共享资源 + 会话集合。
pub struct TigerScheme {
    decoder: Decoder,
    punct: Option<PunctTable>,
    config: SchemeConfig,
    host_options: HostOptions,
    learning_rules: String,
    learning_mode: String,
    store_ready: bool,
    /// 已应用的索引版本（变化时重设解码器学习并重置证据）。
    applied_learning: Option<u64>,
    sessions: HashMap<u64, TigerSession>,
    next_session: u64,
}

impl TigerScheme {
    /// 加载方案数据（词库 / 补充 / 模型 / 词先验 / 标点表）；`notes` 汇总加载状态。
    ///
    /// 模型路径由平台解析后传入（`HUX_MODEL` 覆盖与默认查找都在平台侧）。
    pub fn load(
        dirs: &[PathBuf],
        model_path: Option<PathBuf>,
        config: SchemeConfig,
    ) -> (Self, Vec<String>) {
        let mut notes = Vec::new();
        let lexicon = Lexicon::load(dirs, config.high_freq_limit);
        notes.push(format!("lexicon: {}", lexicon.data_status().canonical()));
        let learning_rules = lexicon.learning_rules.clone();
        let supplement = Supplement::load_default(supplement_dir(dirs).as_deref());
        let model = model_path.and_then(|path| match MobileModel::load(&path, None) {
            Ok(model) => Some(model),
            Err(error) => {
                notes.push(format!("model: {error}"));
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
        let host_options = host_options_from(&config);
        let scheme = Self {
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
        };
        (scheme, notes)
    }

    fn session_mut(&mut self, session: SessionId) -> Option<&mut TigerSession> {
        self.sessions.get_mut(&session.0)
    }
}

/// 页大小归一（契约调用方可能传任意值；与配置层 `Settings::host_options` 同钳制）。
fn page_size_of(config: &SchemeConfig) -> usize {
    config.page_size.clamp(1, MAX_PAGE_SIZE)
}

/// 补充短语所在目录：在**全部**数据目录中取首个存在该文件者。
///
/// 与 lexical / symbols 一致走资产查找——若只读 `dirs.first()`（用户目录恒排第一），
/// 一键安装把数据装到系统级目录时该文件会永不生效。
fn supplement_dir(dirs: &[PathBuf]) -> Option<PathBuf> {
    find_asset(dirs, SUPPLEMENT_FILE).and_then(|path| path.parent().map(Path::to_path_buf))
}

/// 由方案配置派生宿主链选项（键名解析失败项忽略）。
fn host_options_from(config: &SchemeConfig) -> HostOptions {
    let parse = |reprs: &[String]| -> Vec<KeyEvent> {
        reprs
            .iter()
            .filter_map(|repr| KeyEvent::from_repr(repr))
            .collect()
    };
    let mut options = HostOptions {
        page_size: page_size_of(config),
        page_cycle: config.page_cycle,
        ..HostOptions::default()
    };
    if !config.page_up_keys.is_empty() {
        options.page_up_keys = parse(&config.page_up_keys);
    }
    if !config.page_down_keys.is_empty() {
        options.page_down_keys = parse(&config.page_down_keys);
    }
    options
}

/// 触发键（属性）：把两项触发键的 rime 键名列表（逗号分隔）交给 core 解析 / 匹配。
/// 配置变更后在下一次按键 / 重建时惰性同步（属性仅在按键处理中读取）。
fn sync_trigger_keys(context: &mut Context, config: &SchemeConfig) {
    for (property, value) in [
        (
            K_SOUND_TO_CHAR_SHAPE_KEY,
            config.sound_to_char_shape_keys.join(","),
        ),
        (
            K_CHAR_TO_SOUND_SHAPE_KEY,
            config.char_to_sound_shape_keys.join(","),
        ),
    ] {
        set_property_if_changed(context, property, &value);
    }
}

/// 当前组合末段是否为字反查段。
fn char_to_sound_shape_tagged(context: &Context) -> bool {
    context
        .composition
        .back()
        .is_some_and(|segment| segment.has_tag(char_to_sound_shape::TAG))
}

impl Scheme for TigerScheme {
    fn id(&self) -> &'static str {
        SCHEME_ID
    }

    fn learning_rules(&self) -> &str {
        &self.learning_rules
    }

    fn option_ids(&self) -> OptionIds {
        OptionIds {
            early_commit: OPTION_EARLY_COMMIT,
            early_commit_to_preedit: OPTION_EARLY_COMMIT_TO_PREEDIT,
            allow_duplicate_single: OPTION_ALLOW_DUPLICATE_SINGLE,
            digit_select: OPTION_DIGIT_SELECT,
        }
    }

    fn learning_mode(&self, rules: &str, duplicate: bool, high_freq_limit: usize) -> String {
        // 参照 `prepare_learning`：关闭 Tab 学习 → 空串 = 不记录。
        // 模式串自带版本号（`c69c1a8` 起 v1→v2）：事件与索引按 mode 分区，
        // 旧版记录仍留在库中但不再命中。
        if !self.config.tab_learning {
            return String::new();
        }
        format!(
            "sentence-v2|rules={rules}|optimal={high_freq_limit}|dup={}",
            u8::from(duplicate)
        )
    }

    fn apply_config(&mut self, config: &SchemeConfig) {
        self.config = config.clone();
        self.host_options = host_options_from(config);
        let min_retained = Some(config.min_retained_raw_length as i64);
        for session in self.sessions.values_mut() {
            session.min_retained = min_retained;
        }
    }

    fn host_options(&self) -> &HostOptions {
        &self.host_options
    }

    fn set_learning_mode(&mut self, mode: &str) {
        if mode == self.learning_mode {
            return;
        }
        self.learning_mode = mode.to_string();
        for session in self.sessions.values_mut() {
            session.live.mode = mode.to_string();
        }
        self.applied_learning = None;
    }

    fn set_store_ready(&mut self, ready: bool) {
        self.store_ready = ready;
        for session in self.sessions.values_mut() {
            session.live.store_ready = ready;
        }
    }

    fn apply_learning_index(
        &mut self,
        session: SessionId,
        version: u64,
        index: &LearningIndex,
        mode: &str,
    ) {
        if self.applied_learning == Some(version) {
            return;
        }
        self.applied_learning = Some(version);
        self.decoder.set_learning(index.clone(), mode);
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
                min_retained: Some(self.config.min_retained_raw_length as i64),
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
        if !key.release()
            && char_to_sound_shape_tagged(context)
            && matches!(key.repr().as_str(), "Left" | "Right" | "Up" | "Down")
        {
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
            page_size: page_size_of(config),
        };
        let result = processor(
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
            // 参照链：处理器未消费的键交宿主等价物（selector/navigator/express_editor 等）。
            ProcessorResult::Forward => {
                let mut observer = HostCommitObserver {
                    decoder,
                    live: &mut state.live,
                    state: &state.state,
                    now,
                };
                // 契约结果类型即宿主链结果类型（`KeyOutcome` ≡ `HostResult`），无需转换。
                Ok(host::process_key(
                    key,
                    context,
                    punct.as_ref(),
                    host_options,
                    Some(&mut observer),
                ))
            }
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
        char_to_sound_shape_tagged(context)
    }

    fn auxiliary_rows(&mut self, text: &str, cursor_chars: usize) -> (String, String) {
        self.decoder
            .char_to_sound_shape_rows(text, cursor_chars)
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hux_core::learning::LearningIndex;

    fn fixture_dirs() -> Vec<PathBuf> {
        vec![PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../goldens/lexicon")]
    }

    fn fixture_scheme() -> TigerScheme {
        let config = SchemeConfig {
            high_freq_limit: 0,
            page_size: 5,
            tab_learning: true,
            ..SchemeConfig::default()
        };
        TigerScheme::load(&fixture_dirs(), None, config).0
    }

    #[test]
    fn option_ids_are_stable_persisted_keys() {
        // 这些字符串是**持久化契约**（`options.yaml` / legacy `user.yaml` 的键；
        // 也是状态菜单与设置页的对接键）：改名会让老用户的设置静默失效。
        let ids = fixture_scheme().option_ids();
        assert_eq!(ids.early_commit, "tiger_sentence_early_commit");
        assert_eq!(
            ids.early_commit_to_preedit,
            "tiger_sentence_early_commit_to_preedit"
        );
        assert_eq!(
            ids.allow_duplicate_single,
            "tiger_sentence_allow_duplicate_single"
        );
        assert_eq!(ids.digit_select, "tiger_sentence_digit_select");
    }

    #[test]
    fn supplement_dir_searches_all_data_dirs() {
        // 一键安装把数据装在系统级目录（用户目录在前但为空）时，补充短语仍须被找到。
        let user_dir = hux_test_support::temp_dir("supplement-user");
        let system_dir = hux_test_support::temp_dir("supplement-system");
        assert_eq!(
            supplement_dir(&[user_dir.clone(), system_dir.clone()]),
            None
        );
        std::fs::write(system_dir.join(SUPPLEMENT_FILE), "甲 乙 2\n").expect("write");
        assert_eq!(
            supplement_dir(&[user_dir.clone(), system_dir.clone()]),
            Some(system_dir.clone())
        );
        std::fs::write(user_dir.join(SUPPLEMENT_FILE), "甲 乙 2\n").expect("write");
        assert_eq!(
            supplement_dir(&[user_dir.clone(), system_dir]),
            Some(user_dir)
        );
    }

    #[test]
    fn learning_mode_follows_config_and_rules() {
        let scheme = fixture_scheme();
        assert_eq!(
            scheme.learning_mode("abc", true, 1500),
            "sentence-v2|rules=abc|optimal=1500|dup=1"
        );
        let mut off = fixture_scheme();
        off.apply_config(&SchemeConfig {
            tab_learning: false,
            ..SchemeConfig::default()
        });
        assert_eq!(off.learning_mode("abc", true, 1500), "");
    }

    #[test]
    fn apply_learning_index_records_version_once() {
        // 对应平台原先的 `engine_applies_learning_after_key`：已应用版本属方案状态。
        let mut scheme = fixture_scheme();
        let mut context = Context::new();
        let session = scheme.new_session(&mut context);
        let index = LearningIndex::build(&[], 0.0);
        scheme.apply_learning_index(session, 7, &index, "sentence-v2");
        assert_eq!(scheme.applied_learning, Some(7));
        scheme.apply_learning_index(session, 7, &index, "sentence-v2");
        assert_eq!(scheme.applied_learning, Some(7), "同版本不重复应用");
        scheme.apply_learning_index(session, 8, &index, "sentence-v2");
        assert_eq!(scheme.applied_learning, Some(8));
    }

    #[test]
    fn config_reaches_sessions_and_host_options() {
        // 对应平台原先断言的 `session.min_retained` / `host_options` / 触发键。
        let mut scheme = fixture_scheme();
        let mut context = Context::new();
        let session = scheme.new_session(&mut context);
        let config = SchemeConfig {
            min_retained_raw_length: 4,
            page_size: 7,
            page_cycle: true,
            page_up_keys: vec!["comma".to_string()],
            char_to_sound_shape_keys: vec!["quotedbl".to_string()],
            ..SchemeConfig::default()
        };
        scheme.apply_config(&config);
        assert_eq!(scheme.sessions[&session.0].min_retained, Some(4));
        assert_eq!(scheme.host_options.page_size, 7);
        assert!(scheme.host_options.page_cycle);
        sync_trigger_keys(&mut context, &config);
        assert_eq!(
            context.get_property(K_CHAR_TO_SOUND_SHAPE_KEY),
            Some("quotedbl")
        );
        assert_eq!(
            scheme.host_options.page_up_keys,
            vec![KeyEvent::from_repr("comma").expect("comma")]
        );
    }

    #[test]
    fn session_lifecycle_and_learning_events() {
        let mut scheme = fixture_scheme();
        let mut context = Context::new();
        let session = scheme.new_session(&mut context);
        assert!(scheme.take_learning_events(session).is_empty());
        scheme.set_learning_mode("sentence-v2");
        scheme.set_store_ready(true);
        scheme.reset_session(session, &mut context);
        scheme.free_session(session);
        assert!(scheme.sessions.is_empty());
        // 未知会话：按键/点击/重建均安全转发或忽略。
        assert_eq!(
            scheme
                .process_key(session, &mut context, &KeyEvent::new(0x61, 0), 0.0)
                .expect("process"),
            KeyOutcome::Forward
        );
        assert!(
            !scheme
                .select_candidate(session, &mut context, 0, 0.0)
                .expect("select")
        );
        assert!(scheme.rebuild(session, &mut context, false).is_ok());
    }
}
