// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! 配置袋：设置 + 运行时开关 → 方案配置袋，以及方案选项声明的角色解析与热键诊断。

use hux_cfg::Settings;
use hux_cfg::roles::{
    OptionKeys, ROLE_HIGH_FREQ_LIMIT, ROLE_LEARNING_ON_TAB, ROLE_MIN_RETAINED_INPUT_LENGTH,
    ROLE_PAGE_CYCLE, ROLE_PAGE_DOWN_KEYS, ROLE_PAGE_SIZE, ROLE_PAGE_UP_KEYS,
    ROLE_REVERSE_LOOKUP_CHARACTER_KEYS, ROLE_REVERSE_LOOKUP_PRONUNCIATION_KEYS,
};
use hux_core::key::KeyEvent;
use hux_core::scheme::{OptionDecl, SchemeConfig, Value};

/// 运行时开关的**生效值**（会话 → 存储 → 设置缺省；角色无键时按出厂缺省计）。
///
/// 这些开关都随会话/存储变化，装配处（构造 / 重新部署 / 每次按键）必须按同一口径取一次：
/// 配置袋里的值与上下文选项值不一致时，方案的词库 / 学习 mode 会与会话选项脱节。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct RuntimeOptions {
    /// 单字重码参与组句（学习 mode 的 `dup` 位）。
    pub(crate) duplicate: bool,
    /// 启用全字集（装载追加码表）。
    pub(crate) full_charset: bool,
    /// 过滤追加码表里的非汉字。
    pub(crate) filter_non_han: bool,
}

impl RuntimeOptions {
    /// 设置缺省（构造期：尚无会话与存储，运行时开关的初始值即设置值）。
    pub(crate) fn from_settings(settings: &Settings) -> Self {
        Self {
            duplicate: settings.allow_duplicate_single,
            full_charset: settings.full_charset,
            filter_non_han: settings.filter_non_han,
        }
    }
}

/// 解析方案的选项声明：返回（角色 → 键表, 可选错误诊断）。
///
/// **全有或全无**：`OptionKeys::resolve` 只要发现任一问题（缺角色 / 重复 / 空声明）即 `Err`，
/// 本函数随之返回 `OptionKeys::default()`——**所有**角色都不接线（诊断进状态串，列出问题清单），
/// 而不是「只让出问题的那个角色不接线」。宿主菜单与持久化于是跳过全部角色选项，
/// 绝不静默落到别的键上。
pub(crate) fn resolve_option_roles(declarations: &[OptionDecl]) -> (OptionKeys, Option<String>) {
    match OptionKeys::resolve(declarations) {
        Ok(roles) => (roles, None),
        Err(error) => (OptionKeys::default(), Some(error.to_string())),
    }
}

/// 配置页热键绑定里**无法解析为 rime 键名**的项（返回 `角色=键名` 列表）。
///
/// 反向路径：ABI 把 fcitx5 的 `keysym + 状态位` 经 `KeyEvent::repr()` 转成键名交给本层，
/// 而配置页可以绑到**没有名字的 keysym**（媒体键 / 厂商扩展键）：`repr()` 只能输出
/// `0x1008ff14` / `(unknown)` 这类形式，`KeyEvent::from_repr` 不认 ⇒ 该绑定在
/// `Settings::host_options` 与方案 `host_options_from` 的 `filter_map` 处**静默消失**。
/// 这里点名，进 `hux_engine_status`（`hotkeys:` 前缀；C++ 壳在应用设置后落日志）。
pub(crate) fn unparsable_key_bindings(settings: &Settings) -> Vec<String> {
    let mut notes = Vec::new();
    for (role, reprs) in [
        (
            ROLE_REVERSE_LOOKUP_PRONUNCIATION_KEYS,
            &settings.reverse_lookup_pronunciation_keys,
        ),
        (
            ROLE_REVERSE_LOOKUP_CHARACTER_KEYS,
            &settings.reverse_lookup_character_keys,
        ),
        (ROLE_PAGE_UP_KEYS, &settings.page_up_keys),
        (ROLE_PAGE_DOWN_KEYS, &settings.page_down_keys),
    ] {
        for repr in reprs
            .iter()
            .filter(|repr| KeyEvent::from_repr(repr).is_none())
        {
            notes.push(format!("{role}={repr}"));
        }
    }
    notes
}

/// hux 自身设置 → 方案配置袋（平台是装配根：只有这里知道「设置 → 角色」的对应关系）。
///
/// 角色词汇归 `hux-cfg`；本函数只搬运设置值。
pub(crate) fn scheme_config(settings: &Settings) -> SchemeConfig {
    let host = settings.host_options();
    SchemeConfig::new()
        .with(ROLE_HIGH_FREQ_LIMIT, Value::Count(settings.high_freq_limit))
        .with(
            ROLE_MIN_RETAINED_INPUT_LENGTH,
            Value::Count(settings.min_retained()),
        )
        .with(ROLE_PAGE_SIZE, Value::Count(host.page_size))
        .with(ROLE_PAGE_CYCLE, Value::Bool(host.page_cycle))
        .with(
            ROLE_PAGE_UP_KEYS,
            Value::Texts(settings.page_up_keys.clone()),
        )
        .with(
            ROLE_PAGE_DOWN_KEYS,
            Value::Texts(settings.page_down_keys.clone()),
        )
        .with(
            ROLE_REVERSE_LOOKUP_PRONUNCIATION_KEYS,
            Value::Texts(settings.reverse_lookup_pronunciation_keys.clone()),
        )
        .with(
            ROLE_REVERSE_LOOKUP_CHARACTER_KEYS,
            Value::Texts(settings.reverse_lookup_character_keys.clone()),
        )
        .with(ROLE_LEARNING_ON_TAB, Value::Bool(settings.learning_on_tab))
}
