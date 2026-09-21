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
    Asset, AssetKind, KeyOutcome, OptionDecl, Scheme, SchemeConfig, SessionId, asset_paths,
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

/// 角色名：与 `hux-cfg` 的角色常量**同值**。
///
/// 方案不依赖 `hux-cfg`（依赖方向 `hux-scheme/* → hux-core`），故以字面量声明；
/// 一致性由平台装配处的测试守护（每个角色都必须被方案声明）。
mod role {
    pub const EARLY_COMMIT: &str = "early_commit";
    pub const EARLY_COMMIT_TO_PREEDIT: &str = "early_commit_to_preedit";
    pub const ALLOW_DUPLICATE_SINGLE: &str = "allow_duplicate_single";
    pub const DIGIT_SELECT: &str = "digit_select";
    pub const HIGH_FREQ_LIMIT: &str = "high_freq_limit";
    pub const MIN_RETAINED_INPUT_LENGTH: &str = "min_retained_raw_length";
    pub const PAGE_SIZE: &str = "page_size";
    pub const PAGE_CYCLE: &str = "page_cycle";
    pub const PAGE_UP_KEYS: &str = "page_up_keys";
    pub const PAGE_DOWN_KEYS: &str = "page_down_keys";
    pub const REVERSE_LOOKUP_PRONUNCIATION_KEYS: &str = "sound_to_char_shape_keys";
    pub const REVERSE_LOOKUP_CHARACTER_KEYS: &str = "char_to_sound_shape_keys";
    pub const LEARNING_ON_TAB: &str = "tab_learning";
}

/// 本方案的运行时选项声明（角色 → 键；键即 `interaction::OPTION_*` 的持久化键）。
const OPTION_DECLARATIONS: &[OptionDecl] = &[
    OptionDecl {
        role: role::EARLY_COMMIT,
        key: OPTION_EARLY_COMMIT,
    },
    OptionDecl {
        role: role::EARLY_COMMIT_TO_PREEDIT,
        key: OPTION_EARLY_COMMIT_TO_PREEDIT,
    },
    OptionDecl {
        role: role::ALLOW_DUPLICATE_SINGLE,
        key: OPTION_ALLOW_DUPLICATE_SINGLE,
    },
    OptionDecl {
        role: role::DIGIT_SELECT,
        key: OPTION_DIGIT_SELECT,
    },
];

/// 本方案从配置袋读取的**引擎设置角色**（顺序即 [`Config`] 的字段序）。
///
/// 与 `hux_cfg::roles::SCHEME_CONFIG_ROLES` **同值同序**：方案不依赖 `hux-cfg`
/// （依赖方向 `hux-scheme/* → hux-core`），故以同名字面量声明；一致性由平台用例
/// `scheme_config_roles_match_the_scheme` 逐项比对，任一侧改名即失败。
pub const SCHEME_CONFIG_ROLES: &[&str] = &[
    role::HIGH_FREQ_LIMIT,
    role::MIN_RETAINED_INPUT_LENGTH,
    role::PAGE_SIZE,
    role::PAGE_CYCLE,
    role::PAGE_UP_KEYS,
    role::PAGE_DOWN_KEYS,
    role::REVERSE_LOOKUP_PRONUNCIATION_KEYS,
    role::REVERSE_LOOKUP_CHARACTER_KEYS,
    role::LEARNING_ON_TAB,
];

/// 配置袋中本方案还会读取的**运行时选项角色**（平台在装配处追加；键由本方案声明）。
const RUNTIME_CONFIG_ROLES: &[&str] = &[role::ALLOW_DUPLICATE_SINGLE];

/// 配置袋角色诊断：装配处（平台）按 `hux-cfg` 的角色名装袋，本方案按自己的角色名读袋。
///
/// 两侧漂移（单侧改名 / 漏装）在此暴露，而不是让 [`Config::parse`] 静默回退默认值——
/// `min_retained_raw_length` → 0（不再限制保留量）、`tab_learning` → false（不学习）
/// 这类降级在用户侧只表现为「设置没生效」（审计 F1）。
fn config_role_notes(bag: &SchemeConfig) -> Vec<String> {
    let recognized =
        |role: &str| SCHEME_CONFIG_ROLES.contains(&role) || RUNTIME_CONFIG_ROLES.contains(&role);
    let mut notes = Vec::new();
    let unknown: Vec<&str> = bag.roles().filter(|role| !recognized(role)).collect();
    if !unknown.is_empty() {
        notes.push(format!("config: 未识别的角色 {}", unknown.join(", ")));
    }
    let missing: Vec<&str> = SCHEME_CONFIG_ROLES
        .iter()
        .chain(RUNTIME_CONFIG_ROLES)
        .copied()
        .filter(|role| bag.get(role).is_none())
        .collect();
    if !missing.is_empty() {
        notes.push(format!("config: 缺少角色 {}", missing.join(", ")));
    }
    notes
}

/// 虎码口径的方案配置：由契约的键值袋 [`SchemeConfig`] 解析而来。
///
/// 键值袋是**通用容器**（内核不认识角色）；虎码语义（早提交最短保留、反查键、Tab 学习…）
/// 全在本类型与其消费方。缺角色的回退与迁移前的逐字段默认值一致
/// （`0` / `false` / 空列表，`allow_duplicate_single` 同为 `false`）——可观测行为不变。
#[derive(Clone, Debug, Default)]
struct Config {
    high_freq_limit: usize,
    min_retained_input_length: usize,
    page_size: usize,
    page_cycle: bool,
    page_up_keys: Vec<String>,
    page_down_keys: Vec<String>,
    reverse_lookup_pronunciation_keys: Vec<String>,
    reverse_lookup_character_keys: Vec<String>,
    learning_on_tab: bool,
    allow_duplicate_single: bool,
}

impl Config {
    fn parse(config: &SchemeConfig) -> Self {
        Self {
            high_freq_limit: config.count(role::HIGH_FREQ_LIMIT).unwrap_or(0),
            min_retained_input_length: config.count(role::MIN_RETAINED_INPUT_LENGTH).unwrap_or(0),
            page_size: config.count(role::PAGE_SIZE).unwrap_or(0),
            page_cycle: config.bool(role::PAGE_CYCLE).unwrap_or(false),
            page_up_keys: config.texts(role::PAGE_UP_KEYS).to_vec(),
            page_down_keys: config.texts(role::PAGE_DOWN_KEYS).to_vec(),
            reverse_lookup_pronunciation_keys: config
                .texts(role::REVERSE_LOOKUP_PRONUNCIATION_KEYS)
                .to_vec(),
            reverse_lookup_character_keys: config
                .texts(role::REVERSE_LOOKUP_CHARACTER_KEYS)
                .to_vec(),
            learning_on_tab: config.bool(role::LEARNING_ON_TAB).unwrap_or(false),
            allow_duplicate_single: config.bool(role::ALLOW_DUPLICATE_SINGLE).unwrap_or(false),
        }
    }
}

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
    config: Config,
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
        config: &SchemeConfig,
    ) -> (Self, Vec<String>) {
        // 角色漂移诊断先于解析：装袋方与读袋方的角色名必须同值（见 `config_role_notes`）。
        let mut notes = config_role_notes(config);
        let config = Config::parse(config);
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
        };
        // 构造即自算学习 mode（平台随后下发的配置袋与之一致，不会造成 mode 抖动）。
        scheme.learning_mode = scheme.mode_from_config();
        (scheme, notes)
    }

    /// 学习 mode 串（参照 `prepare_learning`）：关闭 Tab 学习 → 空串 = 不记录。
    /// 模式串自带版本号（`c69c1a8` 起 v1→v2）：事件与索引按 mode 分区，
    /// 旧版记录仍留在库中但不再命中。
    fn mode_from_config(&self) -> String {
        if !self.config.learning_on_tab {
            return String::new();
        }
        format!(
            "sentence-v2|rules={}|optimal={}|dup={}",
            self.learning_rules,
            self.config.high_freq_limit,
            u8::from(self.config.allow_duplicate_single)
        )
    }

    fn session_mut(&mut self, session: SessionId) -> Option<&mut TigerSession> {
        self.sessions.get_mut(&session.0)
    }
}

/// 页大小归一（契约调用方可能传任意值；与配置层 `Settings::host_options` 同钳制）。
fn page_size_of(config: &Config) -> usize {
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
fn host_options_from(config: &Config) -> HostOptions {
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
fn sync_trigger_keys(context: &mut Context, config: &Config) {
    for (property, value) in [
        (
            K_SOUND_TO_CHAR_SHAPE_KEY,
            config.reverse_lookup_pronunciation_keys.join(","),
        ),
        (
            K_CHAR_TO_SOUND_SHAPE_KEY,
            config.reverse_lookup_character_keys.join(","),
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

    fn option_declarations(&self) -> &'static [OptionDecl] {
        OPTION_DECLARATIONS
    }

    fn learning_mode(&self) -> &str {
        &self.learning_mode
    }

    fn apply_config(&mut self, config: &SchemeConfig) {
        self.config = Config::parse(config);
        self.host_options = host_options_from(&self.config);
        // 学习 mode 的输入都在配置袋里（Tab 学习 / 高频上限 / 单字重码选项值），
        // 由方案自算：变化时同步全部会话并重置解码器的学习索引（旧 mode 的记录不再命中）。
        let mode = self.mode_from_config();
        let changed = mode != self.learning_mode;
        if changed {
            self.learning_mode = mode;
            self.applied_learning = None;
        }
        let min_retained = Some(self.config.min_retained_input_length as i64);
        for session in self.sessions.values_mut() {
            session.min_retained = min_retained;
            if changed {
                session.live.mode = self.learning_mode.clone();
            }
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
                min_retained: Some(self.config.min_retained_input_length as i64),
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
            // 与紧随其后的宿主链共用同一份翻页键绑定（有意偏离上游，见 `ProcessorEnv`）。
            host_options: &*host_options,
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
    use hux_core::scheme::Value;

    fn fixture_dirs() -> Vec<PathBuf> {
        vec![PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../goldens/lexicon")]
    }

    /// 测试配置袋（角色名与 `hux-cfg` 的常量同值；迁移前逐字段的等价物）。
    fn bag(entries: &[(&'static str, Value)]) -> SchemeConfig {
        let mut config = SchemeConfig::new();
        for (role, value) in entries {
            config.set(role, value.clone());
        }
        config
    }

    fn fixture_scheme() -> TigerScheme {
        let config = bag(&[
            (role::HIGH_FREQ_LIMIT, Value::Count(0)),
            (role::PAGE_SIZE, Value::Count(5)),
            (role::LEARNING_ON_TAB, Value::Bool(true)),
        ]);
        TigerScheme::load(&fixture_dirs(), None, &config).0
    }

    #[test]
    fn option_declarations_are_stable_persisted_keys() {
        // 这些字符串是**持久化契约**（`options.yaml` / legacy `user.yaml` 的键；
        // 也是状态菜单与设置页的对接键）：改名会让老用户的设置静默失效。
        let declarations = fixture_scheme().option_declarations();
        let pairs: Vec<(&str, &str)> = declarations
            .iter()
            .map(|decl| (decl.role, decl.key))
            .collect();
        assert_eq!(
            pairs,
            vec![
                ("early_commit", "tiger_sentence_early_commit"),
                (
                    "early_commit_to_preedit",
                    "tiger_sentence_early_commit_to_preedit"
                ),
                (
                    "allow_duplicate_single",
                    "tiger_sentence_allow_duplicate_single"
                ),
                ("digit_select", "tiger_sentence_digit_select"),
            ]
        );
    }

    #[test]
    fn config_bag_maps_every_role_and_defaults_are_unchanged() {
        // 全角色袋 → 逐字段落位（角色名与 `hux-cfg` 的常量一致，由平台测试守护）。
        let full = bag(&[
            (role::HIGH_FREQ_LIMIT, Value::Count(1500)),
            (role::MIN_RETAINED_INPUT_LENGTH, Value::Count(4)),
            (role::PAGE_SIZE, Value::Count(7)),
            (role::PAGE_CYCLE, Value::Bool(true)),
            (role::PAGE_UP_KEYS, Value::Texts(vec!["comma".to_string()])),
            (
                role::PAGE_DOWN_KEYS,
                Value::Texts(vec!["period".to_string()]),
            ),
            (
                role::REVERSE_LOOKUP_PRONUNCIATION_KEYS,
                Value::Texts(vec!["grave".to_string()]),
            ),
            (
                role::REVERSE_LOOKUP_CHARACTER_KEYS,
                Value::Texts(vec!["quotedbl".to_string()]),
            ),
            (role::LEARNING_ON_TAB, Value::Bool(true)),
            (role::ALLOW_DUPLICATE_SINGLE, Value::Bool(true)),
        ]);
        let config = Config::parse(&full);
        assert_eq!(config.high_freq_limit, 1500);
        assert_eq!(config.min_retained_input_length, 4);
        assert_eq!(config.page_size, 7);
        assert!(config.page_cycle);
        assert_eq!(config.page_up_keys, vec!["comma".to_string()]);
        assert_eq!(config.page_down_keys, vec!["period".to_string()]);
        assert_eq!(
            config.reverse_lookup_pronunciation_keys,
            vec!["grave".to_string()]
        );
        assert_eq!(
            config.reverse_lookup_character_keys,
            vec!["quotedbl".to_string()]
        );
        assert!(config.learning_on_tab);
        assert!(config.allow_duplicate_single);

        // 空袋 → 与迁移前的 `SchemeConfig::default()` 逐字段同值（缺角色回退不变）。
        let empty = Config::parse(&SchemeConfig::default());
        assert_eq!(empty.high_freq_limit, 0);
        assert_eq!(empty.min_retained_input_length, 0);
        assert_eq!(empty.page_size, 0);
        assert!(!empty.page_cycle);
        assert!(empty.page_up_keys.is_empty());
        assert!(empty.page_down_keys.is_empty());
        assert!(empty.reverse_lookup_pronunciation_keys.is_empty());
        assert!(empty.reverse_lookup_character_keys.is_empty());
        assert!(!empty.learning_on_tab);
        assert!(!empty.allow_duplicate_single);

        // 未知角色被忽略（契约是通用容器：将来新增角色不破坏本方案）。
        let future = bag(&[
            ("future_role", Value::Text("x".to_string())),
            (role::PAGE_SIZE, Value::Count(9)),
        ]);
        assert_eq!(future.text("future_role"), Some("x"));
        assert_eq!(Config::parse(&future).page_size, 9);
    }

    /// 角色一致性守护的方案侧一半：装配方按 `hux-cfg` 的角色名装袋，本方案按自己的角色名读袋，
    /// 单侧改名必须**可见**（进状态串诊断），不得静默回退默认值（审计 F1）。
    #[test]
    fn config_role_notes_expose_single_sided_role_renames() {
        // 按角色清单装袋（`keep` 过滤出需要的角色；值不重要，只关心角色名）。
        let bag_of = |keep: &dyn Fn(&str) -> bool, extra: &[(&'static str, Value)]| {
            let mut config = bag(&SCHEME_CONFIG_ROLES
                .iter()
                .filter(|role| keep(role))
                .map(|role| (*role, Value::Count(1)))
                .collect::<Vec<_>>());
            config.set(role::ALLOW_DUPLICATE_SINGLE, Value::Bool(true));
            for (role, value) in extra {
                config.set(role, value.clone());
            }
            config
        };
        // 正例：本方案声明的全角色袋（+ 平台追加的运行时选项角色）无诊断。
        let full = bag_of(&|_| true, &[]);
        assert_eq!(config_role_notes(&full), Vec::<String>::new());

        // 负例：装袋侧把 `tab_learning` 改名（模拟 `hux-cfg` 单侧漂移）——
        // 未识别与缺少两侧都点名，用户可在状态串看到「设置没生效」的原因。
        let renamed = bag_of(
            &|role| role != role::LEARNING_ON_TAB,
            &[("tab_learning_X", Value::Bool(true))],
        );
        assert_eq!(
            config_role_notes(&renamed),
            vec![
                "config: 未识别的角色 tab_learning_X".to_string(),
                "config: 缺少角色 tab_learning".to_string(),
            ]
        );
        // `Config::parse` 对该袋仍是静默回退（这正是需要守护的原因）。
        assert!(!Config::parse(&renamed).learning_on_tab);

        // 负例：漏装（平台少写一项）同样点名，不静默当作「不限制保留量」。
        let dropped = bag_of(&|role| role != role::MIN_RETAINED_INPUT_LENGTH, &[]);
        assert_eq!(
            config_role_notes(&dropped),
            vec!["config: 缺少角色 min_retained_raw_length".to_string()]
        );
        assert_eq!(Config::parse(&dropped).min_retained_input_length, 0);
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
        // mode 的输入都在配置袋里（Tab 学习 / 高频上限 / 单字重码选项值），由方案自算。
        let mut scheme = fixture_scheme();
        scheme.apply_config(&bag(&[
            (role::HIGH_FREQ_LIMIT, Value::Count(1500)),
            (role::LEARNING_ON_TAB, Value::Bool(true)),
            (role::ALLOW_DUPLICATE_SINGLE, Value::Bool(true)),
        ]));
        assert_eq!(
            scheme.learning_mode(),
            format!(
                "sentence-v2|rules={}|optimal=1500|dup=1",
                scheme.learning_rules
            )
        );
        assert!(scheme.learning_mode().starts_with("sentence-v2|rules="));

        let mut off = fixture_scheme();
        off.apply_config(&bag(&[
            (role::LEARNING_ON_TAB, Value::Bool(false)),
            (role::ALLOW_DUPLICATE_SINGLE, Value::Bool(true)),
        ]));
        assert_eq!(off.learning_mode(), "", "关闭 Tab 学习 → 空串 = 不记录");
    }

    #[test]
    fn apply_learning_index_records_version_once() {
        // 对应平台原先的 `engine_applies_learning_after_key`：已应用版本属方案状态。
        let mut scheme = fixture_scheme();
        let mut context = Context::new();
        let session = scheme.new_session(&mut context);
        let index = LearningIndex::build(&[], 0.0);
        scheme.apply_learning_index(session, 7, &index);
        assert_eq!(scheme.applied_learning, Some(7));
        scheme.apply_learning_index(session, 7, &index);
        assert_eq!(scheme.applied_learning, Some(7), "同版本不重复应用");
        scheme.apply_learning_index(session, 8, &index);
        assert_eq!(scheme.applied_learning, Some(8));
    }

    #[test]
    fn config_reaches_sessions_and_host_options() {
        // 对应平台原先断言的 `session.min_retained` / `host_options` / 触发键。
        let mut scheme = fixture_scheme();
        let mut context = Context::new();
        let session = scheme.new_session(&mut context);
        let config = bag(&[
            (role::MIN_RETAINED_INPUT_LENGTH, Value::Count(4)),
            (role::PAGE_SIZE, Value::Count(7)),
            (role::PAGE_CYCLE, Value::Bool(true)),
            (role::PAGE_UP_KEYS, Value::Texts(vec!["comma".to_string()])),
            (
                role::REVERSE_LOOKUP_CHARACTER_KEYS,
                Value::Texts(vec!["quotedbl".to_string()]),
            ),
        ]);
        scheme.apply_config(&config);
        assert_eq!(scheme.sessions[&session.0].min_retained, Some(4));
        assert_eq!(scheme.host_options.page_size, 7);
        assert!(scheme.host_options.page_cycle);
        sync_trigger_keys(&mut context, &scheme.config);
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
        scheme.apply_config(&bag(&[
            (role::LEARNING_ON_TAB, Value::Bool(true)),
            (role::ALLOW_DUPLICATE_SINGLE, Value::Bool(false)),
        ]));
        assert!(scheme.learning_mode().starts_with("sentence-v2|rules="));
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
