// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! legacy 回退：旧 fcitx5 用户配置 `user.yaml` 的 `var/option/<name>`（只读，仅补缺失项）。

use super::load::{child, read_options};
use super::{LEGACY_FILE, LEGACY_OPTION, LEGACY_ROOT, OPTIONS_KEY};
use hux_core::collections::Map;
use std::path::Path;
use yaml_rust2::{Yaml, YamlLoader};

/// 把 legacy 文档的 `var/option/<name>` 补进 `values`（只补缺失项；不回写旧文件）。
pub(super) fn fill_from_legacy(user_dir: &Path, values: &mut Map<String, bool>) {
    if let Ok(text) = std::fs::read_to_string(user_dir.join(LEGACY_FILE))
        && let Ok(mut docs) = YamlLoader::load_from_str(&text)
        && let Some(document) = docs.drain(..).next()
        && let Some(kind) = child(&document, LEGACY_ROOT)
    {
        let wrapper = Yaml::Hash(
            [(
                Yaml::String(OPTIONS_KEY.to_string()),
                child(kind, LEGACY_OPTION).cloned().unwrap_or(Yaml::Null),
            )]
            .into_iter()
            .collect(),
        );
        read_options(&wrapper, values, true);
    }
}
