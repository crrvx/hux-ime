// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! 虎句方案对 [`hux_core::scheme::Scheme`] 的实现。
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
    Asset, AssetKind, ConfigError, KeyOutcome, OptionDecl, Scheme, SchemeConfig, SessionId,
    asset_paths, find_asset,
};
use hux_core::session::Context;

use crate::char_to_sound_shape;
use crate::decode::Decoder;
use crate::interaction::{
    CompositionBuilder, HostCommitObserver, K_CHAR_TO_SOUND_SHAPE_KEY, K_SOUND_TO_CHAR_SHAPE_KEY,
    LiveLearning, OPTION_ALLOW_DUPLICATE_SINGLE, OPTION_DIGIT_SELECT, OPTION_EARLY_COMMIT,
    OPTION_EARLY_COMMIT_TO_PREEDIT, OPTION_FILTER_NON_HAN, OPTION_FULL_CHARSET, ProcessorEnv,
    ProcessorResult, SentenceState, buffered_text, processor, reset_early_evidence,
    select_candidate_at, set_property_if_changed, update_notifier,
};
use crate::lexical;
use crate::lexicon::{LEXICAL_FILE, Lexicon, LexiconOptions, MODEL_PATH, Supplement};
use crate::ngram::MobileModel;

/// 方案标识（与上游数据互通；学习库命名沿用）。
pub const SCHEME_ID: &str = "tiger_sentence";
/// 码表 / 字频 / 白名单 / 补充 / 反查索引 / 标点表的文件名（平台记日志与打包用）。
/// `CODES_FILE` 是**必需**的主表；同目录的 `tiger_sentence.codes.<name>.txt` 是可选追加表
/// （内核按文件名字典序拼在主表之后，见 `data/README.md`），不进 `ASSETS`。
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
    pub const FULL_CHARSET: &str = "full_charset";
    pub const FILTER_NON_HAN: &str = "filter_non_han";
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
    OptionDecl {
        role: role::FULL_CHARSET,
        key: OPTION_FULL_CHARSET,
    },
    OptionDecl {
        role: role::FILTER_NON_HAN,
        key: OPTION_FILTER_NON_HAN,
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
///
/// 字集开关经配置袋下发（配置层角色 → 键在平台侧解析）：方案在 [`Config::parse`] 里
/// 据此重建词库，故它们虽在上下文里也有选项值，读袋仍是唯一入口。
const RUNTIME_CONFIG_ROLES: &[&str] = &[
    role::ALLOW_DUPLICATE_SINGLE,
    role::FULL_CHARSET,
    role::FILTER_NON_HAN,
];

/// 配置袋**角色集合**层面的诊断：装配处（平台）按 `hux-cfg` 的角色名装袋，本方案按
/// 自己的角色名读袋；装出了本方案不认识的名字（单侧改名）在此点名。
///
/// 逐角色的「缺失 / 类型不符」由 [`Config::parse`] 的 [`ConfigError`] 给出（见
/// [`config_diagnostics`]）——两者都进状态串，用户侧不再是「设置没生效」的哑失败
/// 。
fn config_role_notes(bag: &SchemeConfig) -> Vec<String> {
    let recognized =
        |role: &str| SCHEME_CONFIG_ROLES.contains(&role) || RUNTIME_CONFIG_ROLES.contains(&role);
    let unknown: Vec<&str> = bag.roles().filter(|role| !recognized(role)).collect();
    if unknown.is_empty() {
        Vec::new()
    } else {
        vec![format!("config: 未识别的角色 {}", unknown.join(", "))]
    }
}

/// 装配诊断汇总：角色集合（[`config_role_notes`]）+ 逐角色（[`ConfigError`]），
/// 文案统一带 `config:` 前缀（与既有状态串诊断同风格）。
fn config_diagnostics(bag: &SchemeConfig, errors: &[ConfigError]) -> Vec<String> {
    let mut notes = config_role_notes(bag);
    notes.extend(errors.iter().map(|error| format!("config: {error}")));
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
    /// 翻页键：`None` = 角色缺失（用 core 缺省绑定）；`Some([])` = **显式给出空列表**
    /// ⇒ 不绑定。`Vec<String>` 无法区分两者（配置页清空翻页键会退回 core 缺省的 `-`/`=`），
    /// 故用 `Option`。
    page_up_keys: Option<Vec<String>>,
    page_down_keys: Option<Vec<String>>,
    reverse_lookup_pronunciation_keys: Vec<String>,
    reverse_lookup_character_keys: Vec<String>,
    learning_on_tab: bool,
    allow_duplicate_single: bool,
    /// 启用全字集（追加码表）。
    full_charset: bool,
    /// 过滤追加码表里的非汉字。
    filter_non_han: bool,
}

/// 取计数角色；失败时记诊断并按缺省 `0` 回退（迁移前 `unwrap_or(0)` 的同值回退）。
fn require_count(
    config: &SchemeConfig,
    role: &'static str,
    errors: &mut Vec<ConfigError>,
) -> usize {
    config.require_count(role).unwrap_or_else(|error| {
        errors.push(error);
        0
    })
}

/// 取开关角色；失败时记诊断并按 `fallback` 回退。
fn require_bool_or(
    config: &SchemeConfig,
    role: &'static str,
    fallback: bool,
    errors: &mut Vec<ConfigError>,
) -> bool {
    config.require_bool(role).unwrap_or_else(|error| {
        errors.push(error);
        fallback
    })
}

/// 取开关角色；失败时记诊断并按缺省 `false` 回退。
fn require_bool(config: &SchemeConfig, role: &'static str, errors: &mut Vec<ConfigError>) -> bool {
    require_bool_or(config, role, false, errors)
}

/// 取可选的文本列表角色：区分「角色缺失」与「显式空列表」（配置袋契约的基础）。
///
/// `require_texts` 把两者都化成空 `Vec`；本函数的 `None` 只表示**角色缺失 / 类型不符**。
fn optional_texts(
    config: &SchemeConfig,
    role: &'static str,
    errors: &mut Vec<ConfigError>,
) -> Option<Vec<String>> {
    match config.require_texts(role) {
        Ok(values) => Some(values.to_vec()),
        Err(error) => {
            errors.push(error);
            None
        }
    }
}

/// 取文本列表角色；失败时记诊断并按缺省空列表回退。
fn require_texts(
    config: &SchemeConfig,
    role: &'static str,
    errors: &mut Vec<ConfigError>,
) -> Vec<String> {
    config
        .require_texts(role)
        .unwrap_or_else(|error| {
            errors.push(error);
            &[]
        })
        .to_vec()
}

impl Config {
    /// 解析配置袋；同时返回逐角色诊断（缺角色 / 类型不符）。
    ///
    /// 取值失败时按缺省值回退（与迁移前的 `unwrap_or` 同值），但**不再静默**：
    /// 诊断由 [`TigerScheme::load`] / [`TigerScheme::apply_config`] 回给平台进状态串。
    fn parse(config: &SchemeConfig) -> (Self, Vec<ConfigError>) {
        let mut errors = Vec::new();
        let parsed = Self {
            high_freq_limit: require_count(config, role::HIGH_FREQ_LIMIT, &mut errors),
            min_retained_input_length: require_count(
                config,
                role::MIN_RETAINED_INPUT_LENGTH,
                &mut errors,
            ),
            page_size: require_count(config, role::PAGE_SIZE, &mut errors),
            page_cycle: require_bool(config, role::PAGE_CYCLE, &mut errors),
            page_up_keys: optional_texts(config, role::PAGE_UP_KEYS, &mut errors),
            page_down_keys: optional_texts(config, role::PAGE_DOWN_KEYS, &mut errors),
            reverse_lookup_pronunciation_keys: require_texts(
                config,
                role::REVERSE_LOOKUP_PRONUNCIATION_KEYS,
                &mut errors,
            ),
            reverse_lookup_character_keys: require_texts(
                config,
                role::REVERSE_LOOKUP_CHARACTER_KEYS,
                &mut errors,
            ),
            learning_on_tab: require_bool(config, role::LEARNING_ON_TAB, &mut errors),
            allow_duplicate_single: require_bool(config, role::ALLOW_DUPLICATE_SINGLE, &mut errors),
            // 字集开关的缺省是**开**（出厂口径：全字集 + 过滤）：缺角色（装配方漏装）时
            // 按缺省回退，而不是静默退化成「只装主表 / 不过滤」；诊断照旧点名。
            full_charset: require_bool_or(config, role::FULL_CHARSET, true, &mut errors),
            filter_non_han: require_bool_or(config, role::FILTER_NON_HAN, true, &mut errors),
        };
        (parsed, errors)
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
    /// 模型装载状态的一行摘要（宿主状态菜单「模型」项；见 [`crate::model_status`]）。
    model_info: String,
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
        let lexicon = Lexicon::load_with(dirs, config.high_freq_limit, lexicon_options(&config));
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
            model_info: model_status.summary(),
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

/// 词库的字集开关（角色 → [`LexiconOptions`]）：缺角色已在 [`Config::parse`] 回退为出厂口径。
fn lexicon_options(config: &Config) -> LexiconOptions {
    LexiconOptions {
        extra_code_tables: config.full_charset,
        filter_non_han: config.filter_non_han,
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
    // 契约：**角色显式给出即以此为准**（`Some([])` = 不绑定，与 `hux-cfg` 的
    // `Settings::host_options()` 同语义）；只有角色**缺失**时才保留 core 缺省绑定。
    if let Some(reprs) = &config.page_up_keys {
        options.page_up_keys = parse(reprs);
    }
    if let Some(reprs) = &config.page_down_keys {
        options.page_down_keys = parse(reprs);
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

    fn model_info(&self) -> &str {
        &self.model_info
    }

    fn data_info(&self) -> String {
        self.decoder.lexicon().data_info()
    }

    fn apply_config(&mut self, config: &SchemeConfig) -> Result<(), Vec<ConfigError>> {
        let (parsed, errors) = Config::parse(config);
        self.config = parsed;
        self.host_options = host_options_from(&self.config);
        // 高频字上限 / 字集开关改变 ⇒ 重建词库索引（参照 `M.apply_high_freq_limit`）。
        // 平台在装配方案**之后**才下发配置页设置，故这里必须能重建；只在真的变化时重建
        // （每次按键路径都会经 `push_scheme_config` 走到本函数）。
        let limit = self.config.high_freq_limit;
        let options = lexicon_options(&self.config);
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
        let min_retained = Some(self.config.min_retained_input_length as i64);
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

    /// 全角色袋（每个角色都给一个**类型正确**的值），`overrides` 覆盖同名角色。
    /// 真实装配路径（平台）就是这个形态：此后任何缺口都会回诊断。
    fn full_bag(overrides: &[(&'static str, Value)]) -> SchemeConfig {
        let mut config = bag(&[
            (role::HIGH_FREQ_LIMIT, Value::Count(1500)),
            (role::MIN_RETAINED_INPUT_LENGTH, Value::Count(0)),
            (role::PAGE_SIZE, Value::Count(5)),
            (role::PAGE_CYCLE, Value::Bool(false)),
            (role::PAGE_UP_KEYS, Value::Texts(vec!["minus".to_string()])),
            (
                role::PAGE_DOWN_KEYS,
                Value::Texts(vec!["equal".to_string()]),
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
            (role::FULL_CHARSET, Value::Bool(true)),
            (role::FILTER_NON_HAN, Value::Bool(true)),
        ]);
        for (name, value) in overrides {
            config.set(name, value.clone());
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
                ("full_charset", "tiger_sentence_full_charset"),
                ("filter_non_han", "tiger_sentence_filter_non_han"),
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
            (role::FULL_CHARSET, Value::Bool(true)),
            (role::FILTER_NON_HAN, Value::Bool(true)),
        ]);
        let (config, errors) = Config::parse(&full);
        assert_eq!(errors, Vec::<ConfigError>::new(), "全角色袋无诊断");
        assert_eq!(config.high_freq_limit, 1500);
        assert_eq!(config.min_retained_input_length, 4);
        assert_eq!(config.page_size, 7);
        assert!(config.page_cycle);
        assert_eq!(config.page_up_keys, Some(vec!["comma".to_string()]));
        assert_eq!(config.page_down_keys, Some(vec!["period".to_string()]));
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
        assert!(config.full_charset);
        assert!(config.filter_non_han);

        // 空袋 → 与迁移前的 `SchemeConfig::default()` 逐字段同值（缺角色回退不变），
        // 但每个角色都产出「缺少角色」诊断（不再静默）。
        let (empty, empty_errors) = Config::parse(&SchemeConfig::default());
        // 配置角色 + 三个运行时选项角色（单字重码 / 全字集 / 过滤非汉字）。
        assert_eq!(empty_errors.len(), SCHEME_CONFIG_ROLES.len() + 3);
        assert!(
            empty_errors
                .iter()
                .all(|error| matches!(error, ConfigError::Missing { .. })),
            "空袋应逐角色报缺失：{empty_errors:?}"
        );
        assert_eq!(empty.high_freq_limit, 0);
        assert_eq!(empty.min_retained_input_length, 0);
        assert_eq!(empty.page_size, 0);
        assert!(!empty.page_cycle);
        assert_eq!(empty.page_up_keys, None, "缺角色 ≠ 显式空列表");
        assert_eq!(empty.page_down_keys, None);
        assert!(empty.reverse_lookup_pronunciation_keys.is_empty());
        assert!(empty.reverse_lookup_character_keys.is_empty());
        assert!(!empty.learning_on_tab);
        assert!(!empty.allow_duplicate_single);
        // 字集开关缺角色时回退**出厂缺省（开）**，不退化成「只装主表 / 不过滤」。
        assert!(empty.full_charset);
        assert!(empty.filter_non_han);

        // 未知角色被忽略（契约是通用容器：将来新增角色不破坏本方案）。
        let future = bag(&[
            ("future_role", Value::Text("x".to_string())),
            (role::PAGE_SIZE, Value::Count(9)),
        ]);
        assert_eq!(future.text("future_role"), Some("x"));
        let (parsed, errors) = Config::parse(&future);
        assert_eq!(parsed.page_size, 9);
        assert!(
            errors.iter().all(|error| error.role() != role::PAGE_SIZE),
            "已装配且类型正确的角色不得报诊断：{errors:?}"
        );
    }

    /// 角色一致性守护的方案侧一半：装配方按 `hux-cfg` 的角色名装袋，本方案按自己的角色名读袋，
    /// 单侧改名必须**可见**（进状态串诊断），不得静默回退默认值。
    #[test]
    fn config_role_notes_expose_single_sided_role_renames() {
        // 按角色清单装袋（`keep` 过滤出需要的角色；每个角色给**类型正确**的值，
        // 这样诊断只会来自角色集合或刻意构造的类型不符）。
        let bag_of = |keep: &dyn Fn(&str) -> bool, extra: &[(&'static str, Value)]| {
            let mut config = SchemeConfig::new();
            for (name, value) in [
                (role::HIGH_FREQ_LIMIT, Value::Count(1)),
                (role::MIN_RETAINED_INPUT_LENGTH, Value::Count(1)),
                (role::PAGE_SIZE, Value::Count(1)),
                (role::PAGE_CYCLE, Value::Bool(true)),
                (role::PAGE_UP_KEYS, Value::Texts(vec!["minus".to_string()])),
                (
                    role::PAGE_DOWN_KEYS,
                    Value::Texts(vec!["equal".to_string()]),
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
                (role::FULL_CHARSET, Value::Bool(true)),
                (role::FILTER_NON_HAN, Value::Bool(true)),
            ] {
                if keep(name) {
                    config.set(name, value);
                }
            }
            for (role, value) in extra {
                config.set(role, value.clone());
            }
            config
        };
        // 诊断 = 角色集合（未识别）+ 逐角色（缺失 / 类型不符），文案统一带 `config:` 前缀。
        let diagnostics = |bag: &SchemeConfig| {
            let (_, errors) = Config::parse(bag);
            config_diagnostics(bag, &errors)
        };
        // 正例：本方案声明的全角色袋（+ 平台追加的运行时选项角色）无诊断。
        let full = bag_of(&|_| true, &[]);
        assert_eq!(config_role_notes(&full), Vec::<String>::new());
        assert_eq!(diagnostics(&full), Vec::<String>::new());

        // 负例：装袋侧把 `tab_learning` 改名（模拟 `hux-cfg` 单侧漂移）——
        // 未识别与缺少两侧都点名，用户可在状态串看到「设置没生效」的原因。
        let renamed = bag_of(
            &|role| role != role::LEARNING_ON_TAB,
            &[("tab_learning_X", Value::Bool(true))],
        );
        assert_eq!(
            config_role_notes(&renamed),
            vec!["config: 未识别的角色 tab_learning_X".to_string()]
        );
        assert_eq!(
            diagnostics(&renamed),
            vec![
                "config: 未识别的角色 tab_learning_X".to_string(),
                "config: 缺少角色 tab_learning".to_string(),
            ]
        );
        // 取值仍按缺省回退（可观测行为不变：缺角色 = 不学习），但回退**不再静默**。
        let (parsed, errors) = Config::parse(&renamed);
        assert!(!parsed.learning_on_tab);
        assert_eq!(
            errors,
            vec![ConfigError::Missing {
                role: role::LEARNING_ON_TAB
            }]
        );

        // 负例：**类型不符**（把 `Count` 塞进开关角色）——此前 `bool()` 只返回 `None`，
        // 全链路静默；现在逐角色点名。
        let wrong_type = bag_of(&|_| true, &[(role::LEARNING_ON_TAB, Value::Count(1))]);
        assert_eq!(config_role_notes(&wrong_type), Vec::<String>::new());
        assert_eq!(
            diagnostics(&wrong_type),
            vec!["config: 角色 tab_learning 类型不符（期望 开关，实际 计数）".to_string()]
        );
        assert_eq!(
            Config::parse(&wrong_type).1,
            vec![ConfigError::TypeMismatch {
                role: role::LEARNING_ON_TAB,
                expected: "开关",
                found: "计数",
            }]
        );
        assert!(
            !Config::parse(&wrong_type).0.learning_on_tab,
            "类型不符按缺省回退"
        );

        // 负例：漏装（平台少写一项）同样点名，不静默当作「不限制保留量」。
        let dropped = bag_of(&|role| role != role::MIN_RETAINED_INPUT_LENGTH, &[]);
        assert_eq!(
            config_role_notes(&dropped),
            Vec::<String>::new(),
            "角色集合层面无未识别项"
        );
        assert_eq!(
            diagnostics(&dropped),
            vec!["config: 缺少角色 min_retained_raw_length".to_string()]
        );
        assert_eq!(Config::parse(&dropped).0.min_retained_input_length, 0);
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
        scheme
            .apply_config(&full_bag(&[
                (role::HIGH_FREQ_LIMIT, Value::Count(1500)),
                (role::LEARNING_ON_TAB, Value::Bool(true)),
                (role::ALLOW_DUPLICATE_SINGLE, Value::Bool(true)),
            ]))
            .expect("全角色袋");
        assert_eq!(
            scheme.learning_mode(),
            format!(
                "sentence-v2|rules={}|optimal=1500|dup=1",
                scheme.learning_rules
            )
        );
        assert!(scheme.learning_mode().starts_with("sentence-v2|rules="));

        // 格式归属方案（平台只看不透明串）⇒ 单字重码关闭时的 `dup=0` 也在本文件钉住。
        let mut no_duplicate = fixture_scheme();
        no_duplicate
            .apply_config(&full_bag(&[
                (role::HIGH_FREQ_LIMIT, Value::Count(1500)),
                (role::LEARNING_ON_TAB, Value::Bool(true)),
                (role::ALLOW_DUPLICATE_SINGLE, Value::Bool(false)),
            ]))
            .expect("全角色袋");
        assert_eq!(
            no_duplicate.learning_mode(),
            format!(
                "sentence-v2|rules={}|optimal=1500|dup=0",
                no_duplicate.learning_rules
            )
        );

        let mut off = fixture_scheme();
        off.apply_config(&full_bag(&[
            (role::LEARNING_ON_TAB, Value::Bool(false)),
            (role::ALLOW_DUPLICATE_SINGLE, Value::Bool(true)),
        ]))
        .expect("全角色袋");
        assert_eq!(off.learning_mode(), "", "关闭 Tab 学习 → 空串 = 不记录");
    }

    #[test]
    fn apply_config_rebuilds_the_lexicon_for_a_new_high_freq_limit() {
        // 平台在装配方案**之后**才把配置页设置下发（`hux_engine_new` → 宿主 `applyConfig`），
        // 故上限只在 `load` 时生效等于「设置永不生效」；本用例钉住重新下发即重建。
        // `jvn`：`华` 是主码、`仍`（rank 564）的非主码，上限 > 0 时被过滤。
        let texts = |scheme: &TigerScheme, code: &str| -> Vec<String> {
            scheme
                .decoder
                .lexicon()
                .probe(code)
                .expect("码存在")
                .iter()
                .map(|entry| entry.text.clone())
                .collect()
        };
        let mut scheme = fixture_scheme(); // 夹具按上限 0（不过滤）装载
        assert_eq!(
            texts(&scheme, "jvn"),
            vec!["华".to_string(), "仍".to_string()]
        );
        scheme
            .apply_config(&full_bag(&[(role::HIGH_FREQ_LIMIT, Value::Count(1500))]))
            .expect("全角色袋");
        assert_eq!(texts(&scheme, "jvn"), vec!["华".to_string()]);
        assert_eq!(scheme.decoder.lexicon().data_status().high_freq_limit, 1500);
        // 放开上限同样重建（不是「只收紧一次」）。
        scheme
            .apply_config(&full_bag(&[(role::HIGH_FREQ_LIMIT, Value::Count(0))]))
            .expect("全角色袋");
        assert_eq!(
            texts(&scheme, "jvn"),
            vec!["华".to_string(), "仍".to_string()]
        );
        assert_eq!(scheme.decoder.lexicon().data_status().high_freq_limit, 0);
        assert_eq!(
            scheme.learning_mode(),
            format!(
                "sentence-v2|rules={}|optimal=0|dup=1",
                scheme.learning_rules
            )
        );
    }

    /// 夹具码表 + **一张追加码表**（一个扩展 B 汉字与一个部首，各给一个新码）：
    /// 字集开关的用例要有追加表才能看出效果（[`fixture_dirs`] 里没有）。
    fn charset_dirs() -> PathBuf {
        let dir = hux_test_support::temp_dir("scheme-charset");
        let fixture = fixture_dirs().remove(0);
        for name in [
            "tiger_sentence.codes.txt",
            "tiger_sentence.char_ranks.txt",
            "tiger_sentence.full_code_whitelist.txt",
        ] {
            std::fs::copy(fixture.join(name), dir.join(name)).expect("复制夹具码表");
        }
        std::fs::write(
            dir.join("tiger_sentence.codes.huma.txt"),
            "𤕫\tzzzv\n⽧\tzzzw\n",
        )
        .expect("写追加码表");
        dir
    }

    /// 两个字集开关都改词库内容 ⇒ 重新下发配置即重建（同高频上限的重建路径）。
    ///
    /// 语义：关掉全字集只装主表；过滤只作用于**追加表**（主表里的非汉字照旧）。
    #[test]
    fn apply_config_rebuilds_the_lexicon_for_the_charset_options() {
        let dir = charset_dirs();
        let scheme_of = |overrides: &[(&'static str, Value)]| -> TigerScheme {
            TigerScheme::load(std::slice::from_ref(&dir), None, &full_bag(overrides)).0
        };
        let texts = |scheme: &TigerScheme, code: &str| -> Vec<String> {
            scheme
                .decoder
                .lexicon()
                .probe(code)
                .unwrap_or_else(|| panic!("码 {code} 不存在"))
                .iter()
                .map(|entry| entry.text.clone())
                .collect()
        };

        // 仅主表的条目数（关掉全字集装载；下面用它作基准口径）。
        let primary_entries = scheme_of(&[(role::FULL_CHARSET, Value::Bool(false))])
            .decoder
            .lexicon()
            .codes_entries;

        // 出厂口径（全字集开 + 过滤开）：追加表的汉字在，部首被过滤。
        let mut scheme = scheme_of(&[]);
        assert_eq!(texts(&scheme, "zzzv"), vec!["𤕫".to_string()]);
        assert!(
            scheme.decoder.lexicon().probe("zzzw").is_none(),
            "追加表里的部首应被过滤"
        );
        assert_eq!(scheme.decoder.lexicon().extra_code_tables().len(), 1);
        let filtered_entries = scheme.decoder.lexicon().codes_entries;
        assert_eq!(
            filtered_entries,
            primary_entries + 1,
            "过滤开：追加表只剩那个汉字"
        );
        assert!(
            scheme.data_info().starts_with(&format!(
                "code_tables=[tiger_sentence.codes.txt,tiger_sentence.codes.huma.txt] \
                 entries={filtered_entries}"
            )),
            "装载摘要应含实际装载的码表：{}",
            scheme.data_info()
        );
        assert!(
            scheme
                .data_info()
                .ends_with("full_charset=1 filter_non_han=1")
        );

        // 关掉全字集：追加表独有码消失、诊断口径为空（重新打开同样重建，不是「只关一次」）。
        scheme
            .apply_config(&full_bag(&[(role::FULL_CHARSET, Value::Bool(false))]))
            .expect("全角色袋");
        assert!(scheme.decoder.lexicon().probe("zzzv").is_none());
        assert!(scheme.decoder.lexicon().extra_code_tables().is_empty());
        assert_eq!(
            scheme.data_info(),
            format!(
                "code_tables=[tiger_sentence.codes.txt] entries={primary_entries} \
                 chars={} full_charset=0 filter_non_han=1",
                scheme.decoder.lexicon().character_codes.len()
            ),
            "关掉全字集后摘要只剩主表"
        );
        scheme
            .apply_config(&full_bag(&[(role::FULL_CHARSET, Value::Bool(true))]))
            .expect("全角色袋");
        assert_eq!(texts(&scheme, "zzzv"), vec!["𤕫".to_string()]);

        // 关掉过滤：追加表的部首入词库（条目正好多一条），主表内容不动。
        scheme
            .apply_config(&full_bag(&[(role::FILTER_NON_HAN, Value::Bool(false))]))
            .expect("全角色袋");
        assert_eq!(texts(&scheme, "zzzw"), vec!["⽧".to_string()]);
        assert_eq!(
            scheme.decoder.lexicon().codes_entries,
            primary_entries + 2,
            "过滤关：追加表两行都入词库"
        );
        assert!(
            scheme
                .data_info()
                .ends_with("full_charset=1 filter_non_han=0")
        );

        std::fs::remove_dir_all(&dir).ok();
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
        let config = full_bag(&[
            (role::MIN_RETAINED_INPUT_LENGTH, Value::Count(4)),
            (role::PAGE_SIZE, Value::Count(7)),
            (role::PAGE_CYCLE, Value::Bool(true)),
            (role::PAGE_UP_KEYS, Value::Texts(vec!["comma".to_string()])),
            (
                role::REVERSE_LOOKUP_CHARACTER_KEYS,
                Value::Texts(vec!["quotedbl".to_string()]),
            ),
        ]);
        scheme.apply_config(&config).expect("全角色袋");
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
    fn empty_page_key_lists_unbind_the_keys() {
        // 角色**显式给出空列表** ⇒ 不绑定翻页键（与 `hux-cfg` 的
        // `Settings::host_options()` 同语义，即配置页清空键列表后真的不再翻页）；
        // 角色**缺失** ⇒ 保留 core 缺省绑定（`HostOptions::default()` 的 `-`/`=`）。
        let mut scheme = fixture_scheme();
        scheme
            .apply_config(&full_bag(&[
                (role::PAGE_UP_KEYS, Value::Texts(Vec::new())),
                (role::PAGE_DOWN_KEYS, Value::Texts(Vec::new())),
            ]))
            .expect("全角色袋");
        assert!(
            scheme.host_options.page_up_keys.is_empty(),
            "显式空列表 ⇒ 上翻页键不绑定"
        );
        assert!(
            scheme.host_options.page_down_keys.is_empty(),
            "显式空列表 ⇒ 下翻页键不绑定"
        );
        // 缺角色（空袋）⇒ 保持缺省绑定，与 core 一致。
        let mut missing = fixture_scheme();
        // 空袋 ⇒ 逐角色诊断，但配置照常落地（与 `apply_config` 的既有语义一致）。
        let errors = missing
            .apply_config(&SchemeConfig::default())
            .expect_err("空袋必须回逐角色诊断");
        assert!(
            errors
                .iter()
                .all(|error| matches!(error, hux_core::scheme::ConfigError::Missing { .. })),
            "空袋应逐角色报缺失：{errors:?}"
        );
        assert_eq!(
            missing.host_options.page_up_keys,
            HostOptions::default().page_up_keys
        );
        assert_eq!(
            missing.host_options.page_down_keys,
            HostOptions::default().page_down_keys
        );
        assert!(
            !HostOptions::default().page_up_keys.is_empty(),
            "core 缺省上翻页绑定应为非空（否则本用例是恒真的）"
        );
    }

    #[test]
    fn session_lifecycle_and_learning_events() {
        let mut scheme = fixture_scheme();
        let mut context = Context::new();
        let session = scheme.new_session(&mut context);
        assert!(scheme.take_learning_events(session).is_empty());
        scheme
            .apply_config(&full_bag(&[
                (role::LEARNING_ON_TAB, Value::Bool(true)),
                (role::ALLOW_DUPLICATE_SINGLE, Value::Bool(false)),
            ]))
            .expect("全角色袋");
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
