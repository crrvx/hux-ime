// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! 方案装配、配置映射与会话生命周期的单元测试（原名内联于门面文件）。
//! 用例按被测主题归档在 tests/ 子目录。

mod config;
mod learning;
mod lexicon;

use super::assets::role;
use super::*;
use hux_core::scheme::Value;

fn fixture_dirs() -> Vec<PathBuf> {
    vec![hux_test_support::repo_path("goldens/lexicon")]
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
