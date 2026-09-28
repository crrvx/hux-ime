// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! 写回主文件：在既有文档上就地合并（保留未知键与键序；按名排序保证稳定输出）。

use super::{OPTIONS_KEY, OptionsStore};
use yaml_rust2::{Yaml, YamlEmitter};

impl OptionsStore {
    /// 写回主文件（保留未知键；按名排序保证稳定输出）。
    pub(super) fn save(&self) -> anyhow::Result<()> {
        let mut document = self.document.clone();
        if !matches!(document, Yaml::Hash(_)) {
            document = Yaml::Hash(yaml_rust2::yaml::Hash::new());
        }
        let Yaml::Hash(map) = &mut document else {
            return Err(anyhow::anyhow!("invalid options document"));
        };
        let mut names: Vec<&String> = self.options.values.keys().collect();
        names.sort();
        // 在既有 `options:` 映射上更新（保留未知键与键序）。
        let entry = map
            .entry(Yaml::String(OPTIONS_KEY.to_string()))
            .or_insert_with(|| Yaml::Hash(yaml_rust2::yaml::Hash::new()));
        if !matches!(entry, Yaml::Hash(_)) {
            *entry = Yaml::Hash(yaml_rust2::yaml::Hash::new());
        }
        let Yaml::Hash(options) = entry else {
            return Err(anyhow::anyhow!("invalid options mapping"));
        };
        for name in names {
            options.insert(
                Yaml::String(name.clone()),
                Yaml::Boolean(self.options.values[name]),
            );
        }
        let mut text = String::new();
        YamlEmitter::new(&mut text).dump(&document)?;
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&self.path, text)?;
        Ok(())
    }
}
