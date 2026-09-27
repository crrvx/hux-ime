// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! 虎码口径的方案配置：由契约的键值袋 [`Config`] 解析而来，并给出逐角色诊断。

use hux_core::host::{HostOptions, MAX_PAGE_SIZE};
use hux_core::key::KeyEvent;
use hux_core::scheme::{ConfigError, SchemeConfig};

use super::assets::{RUNTIME_CONFIG_ROLES, SCHEME_CONFIG_ROLES, role};
use crate::lexicon::LexiconOptions;

/// 配置袋**角色集合**层面的诊断：装配处（平台）按 `hux-cfg` 的角色名装袋，本方案按
/// 自己的角色名读袋；装出了本方案不认识的名字（单侧改名）在此点名。
///
/// 逐角色的「缺失 / 类型不符」由 [`Config::parse`] 的 [`ConfigError`] 给出（见
/// [`config_diagnostics`]）——两者都进状态串，用户侧不再是「设置没生效」的哑失败
/// 。
pub(super) fn config_role_notes(bag: &SchemeConfig) -> Vec<String> {
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
pub(super) fn config_diagnostics(bag: &SchemeConfig, errors: &[ConfigError]) -> Vec<String> {
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
pub(super) struct Config {
    pub(super) high_freq_limit: usize,
    pub(super) min_retained_input_length: usize,
    pub(super) page_size: usize,
    pub(super) page_cycle: bool,
    /// 翻页键：`None` = 角色缺失（用 core 缺省绑定）；`Some([])` = **显式给出空列表**
    /// ⇒ 不绑定。`Vec<String>` 无法区分两者（配置页清空翻页键会退回 core 缺省的 `-`/`=`），
    /// 故用 `Option`。
    pub(super) page_up_keys: Option<Vec<String>>,
    pub(super) page_down_keys: Option<Vec<String>>,
    pub(super) reverse_lookup_pronunciation_keys: Vec<String>,
    pub(super) reverse_lookup_character_keys: Vec<String>,
    pub(super) learning_on_tab: bool,
    pub(super) allow_duplicate_single: bool,
    /// 启用全字集（追加码表）。
    pub(super) full_charset: bool,
    /// 过滤追加码表里的非汉字。
    pub(super) filter_non_han: bool,
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
    /// 诊断由 [`TigerScheme::load`](super::TigerScheme::load) / [`TigerScheme::apply_config`](super::TigerScheme::apply_config) 回给平台进状态串。
    pub(super) fn parse(config: &SchemeConfig) -> (Self, Vec<ConfigError>) {
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

    /// 词库的字集开关（角色 → [`LexiconOptions`]）：缺角色已在 [`Config::parse`] 回退为出厂口径。
    pub(super) fn lexicon_options(&self) -> LexiconOptions {
        LexiconOptions {
            extra_code_tables: self.full_charset,
            filter_non_han: self.filter_non_han,
        }
    }

    /// 页大小归一（契约调用方可能传任意值；与配置层 `Settings::host_options` 同钳制）。
    pub(super) fn page_size(&self) -> usize {
        self.page_size.clamp(1, MAX_PAGE_SIZE)
    }

    /// 由方案配置派生宿主链选项（键名解析失败项忽略）。
    pub(super) fn host_options(&self) -> HostOptions {
        let parse = |reprs: &[String]| -> Vec<KeyEvent> {
            reprs
                .iter()
                .filter_map(|repr| KeyEvent::from_repr(repr))
                .collect()
        };
        let mut options = HostOptions {
            page_size: self.page_size(),
            page_cycle: self.page_cycle,
            ..HostOptions::default()
        };
        // 契约：**角色显式给出即以此为准**（`Some([])` = 不绑定，与 `hux-cfg` 的
        // `Settings::host_options()` 同语义）；只有角色**缺失**时才保留 core 缺省绑定。
        if let Some(reprs) = &self.page_up_keys {
            options.page_up_keys = parse(reprs);
        }
        if let Some(reprs) = &self.page_down_keys {
            options.page_down_keys = parse(reprs);
        }
        options
    }
}
