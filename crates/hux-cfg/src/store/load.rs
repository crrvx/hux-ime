// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! 读取主文件 `tiger_sentence.options.yaml`（解析失败即视为空文档）与 YAML 文档解析助手。

use super::legacy::fill_from_legacy;
use super::{OPTIONS_FILE, OPTIONS_KEY, OptionsStore};
use crate::Options;
#[cfg(test)]
use crate::builtin_option_defaults;
#[cfg(test)]
use crate::roles::OptionKeys;
use hux_core::collections::Map;
use std::path::Path;
use yaml_rust2::{Yaml, YamlLoader};

/// 从 YAML 文档中取一层映射（不存在或类型不符返回 `None`）。
pub(super) fn child<'a>(value: &'a Yaml, key: &str) -> Option<&'a Yaml> {
    match value {
        Yaml::Hash(map) => map.get(&Yaml::String(key.to_string())),
        _ => None,
    }
}

/// 读取 `options:` 下的布尔键值（可选限制为“仅补缺失项”）。
pub(super) fn read_options(value: &Yaml, values: &mut Map<String, bool>, fill_missing_only: bool) {
    let Some(Yaml::Hash(map)) = child(value, OPTIONS_KEY) else {
        return;
    };
    for (key, value) in map {
        if let (Yaml::String(name), Yaml::Boolean(flag)) = (key, value)
            && (!fill_missing_only || !values.contains_key(name))
        {
            values.insert(name.clone(), *flag);
        }
    }
}

impl OptionsStore {
    /// 参照 `open_store`：读取主文件与 legacy 回退；解析失败即视为空文档。
    /// 测试用便捷入口（生产路径由 addon 传入 `Settings` 缺省）。
    #[cfg(test)]
    pub fn load(user_dir: &Path, keys: &OptionKeys) -> Self {
        Self::load_with_defaults(user_dir, builtin_option_defaults(keys))
    }

    /// 同 [`OptionsStore::load`]，但以给定缺省回退缺失项
    /// （addon `Settings` 经此成为存储层缺省，合并顺序仍为 `options.yaml` > 设置 > 内建）。
    pub fn load_with_defaults(user_dir: &Path, defaults: Map<String, bool>) -> Self {
        let path = user_dir.join(OPTIONS_FILE);
        let mut document = match std::fs::read_to_string(&path) {
            Ok(text) => YamlLoader::load_from_str(&text)
                .ok()
                .and_then(|mut docs| docs.drain(..).next())
                .unwrap_or(Yaml::Hash(yaml_rust2::yaml::Hash::new())),
            Err(_) => Yaml::Hash(yaml_rust2::yaml::Hash::new()),
        };
        if !matches!(document, Yaml::Hash(_)) {
            document = Yaml::Hash(yaml_rust2::yaml::Hash::new());
        }
        let mut values = Map::new();
        read_options(&document, &mut values, false);
        // legacy：`user.yaml` 的 `var/option/<name>`（只读，仅补缺失项）。
        fill_from_legacy(user_dir, &mut values);
        let mut options = Options::new(defaults);
        options.values = values;
        Self {
            path,
            document,
            options,
        }
    }
}
