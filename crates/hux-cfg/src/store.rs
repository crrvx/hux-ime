// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! 选项持久化（参照 `M.options`）：主文件 `tiger_sentence.options.yaml`，
//! 缺失键回退 `<user dir>/user.yaml` 的 `var/option/<name>`（只读）；
//! 保存失败写入属性 `tiger_sentence_options_error`。

use hashbrown::HashMap;
use std::path::{Path, PathBuf};

use crate::Options;
#[cfg(test)]
use crate::option_defaults;
#[cfg(test)]
use crate::roles::OptionKeys;
use hux_core::session::{Context, set_property_if_changed};
use yaml_rust2::{Yaml, YamlEmitter, YamlLoader};

/// 主存储文件名（用户数据目录下）。
pub const OPTIONS_FILE: &str = "tiger_sentence.options.yaml";
/// legacy 回退文件名（rime 用户配置，只读）。
pub const LEGACY_FILE: &str = "user.yaml";
/// 保存失败属性名（参照 `M.options`）。
pub const OPTIONS_ERROR_PROPERTY: &str = "tiger_sentence_options_error";
const OPTIONS_ERROR_MESSAGE: &str = "Unable to save tiger_sentence.options.yaml";
const OPTIONS_KEY: &str = "options";
const LEGACY_ROOT: &str = "var";
const LEGACY_OPTION: &str = "option";

/// 选项存储：完整 YAML 文档（保留未知键）+ core [`Options`] 状态。
pub struct OptionsStore {
    path: PathBuf,
    document: Yaml,
    options: Options,
}

/// 从 YAML 文档中取一层映射（不存在或类型不符返回 `None`）。
fn child<'a>(value: &'a Yaml, key: &str) -> Option<&'a Yaml> {
    match value {
        Yaml::Hash(map) => map.get(&Yaml::String(key.to_string())),
        _ => None,
    }
}

/// 读取 `options:` 下的布尔键值（可选限制为“仅补缺失项”）。
fn read_options(value: &Yaml, values: &mut HashMap<String, bool>, fill_missing_only: bool) {
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
        Self::load_with_defaults(user_dir, option_defaults(keys))
    }

    /// 同 [`OptionsStore::load`]，但以给定缺省回退缺失项
    /// （addon `Settings` 经此成为存储层缺省，合并顺序仍为 `options.yaml` > 设置 > 内建）。
    pub fn load_with_defaults(user_dir: &Path, defaults: HashMap<String, bool>) -> Self {
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
        let mut values = HashMap::new();
        read_options(&document, &mut values, false);
        // legacy：`user.yaml` 的 `var/option/<name>`（只读，仅补缺失项）。
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
            read_options(&wrapper, &mut values, true);
        }
        let mut options = Options::new(defaults);
        options.values = values;
        Self {
            path,
            document,
            options,
        }
    }

    /// 更新设置层缺省（配置界面变化后调用；`options.yaml` 值仍优先）。
    /// 该选项是否由本存储管理（有声明的缺省 ⇒ 可持久化）。
    ///
    /// 平台据此分流：可持久化项交由 [`OptionsStore::sync`] 写入（其写入带抑制名单，
    /// 不会被 [`OptionsStore::observe`] 当成用户改动落盘），其余非持久化项直接写上下文。
    pub fn covers(&self, name: &str) -> bool {
        self.options.covers(name)
    }

    /// 直写持久化值并保存（**无会话**时状态菜单切换仍须落盘；返回是否保存成功）。
    pub fn set_value(&mut self, name: &str, value: bool) -> bool {
        if !self.options.set_value(name, value) {
            return self.options.covers(name);
        }
        self.save().is_ok()
    }

    pub fn set_defaults(&mut self, defaults: HashMap<String, bool>) {
        self.options.defaults = defaults;
    }

    /// 参照 `M.options.sync`：把持久化值（缺省回退内建缺省）同步进上下文选项。
    pub fn sync(&mut self, context: &mut Context) {
        self.options.sync(context);
    }

    /// 单项生效值（持久化值 → 设置缺省）；未知项返回 `None`。
    pub fn value(&self, name: &str) -> Option<bool> {
        self.options
            .values
            .get(name)
            .or_else(|| self.options.defaults.get(name))
            .copied()
    }

    /// 选项变更（上下文事件）：记录；有变更则保存并维护错误属性。
    pub fn observe(&mut self, context: &mut Context, name: &str) {
        if !self.options.observe(context, name) {
            return;
        }
        let error = match self.save() {
            Ok(()) => "",
            Err(_) => OPTIONS_ERROR_MESSAGE,
        };
        set_property_if_changed(context, OPTIONS_ERROR_PROPERTY, error);
    }

    /// 写回主文件（保留未知键；按名排序保证稳定输出）。
    fn save(&self) -> anyhow::Result<()> {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("hux-options-{}-{tag}", std::process::id()));
        std::fs::remove_dir_all(&dir).ok();
        std::fs::create_dir_all(&dir).expect("temp dir");
        dir
    }

    fn drain_option_events(store: &mut OptionsStore, context: &mut Context) {
        for event in context.drain_events() {
            if let hux_core::session::Event::Option(name) = event {
                store.observe(context, &name);
            }
        }
    }

    #[test]
    fn load_applies_stored_options() {
        let dir = temp_dir("load");
        std::fs::write(
            dir.join(OPTIONS_FILE),
            "options:\n  tiger_sentence_early_commit: false\n  some_other_option: true\ncustom: 1\n",
        )
        .expect("write");
        let mut store = OptionsStore::load(&dir, &crate::options::test_option_keys());
        let mut context = Context::new();
        store.sync(&mut context);
        assert!(!context.get_option("tiger_sentence_early_commit"));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn observe_saves_user_change() {
        let dir = temp_dir("observe");
        std::fs::write(
            dir.join(OPTIONS_FILE),
            "options:\n  tiger_sentence_early_commit: false\n",
        )
        .expect("write");
        let mut store = OptionsStore::load(&dir, &crate::options::test_option_keys());
        let mut context = Context::new();
        store.sync(&mut context);
        drain_option_events(&mut store, &mut context);
        // 用户改动 → 记录并保存
        context.set_option("tiger_sentence_early_commit", true);
        store.observe(&mut context, "tiger_sentence_early_commit");
        assert_eq!(store.options.revision, 1);
        assert_ne!(
            context.get_property(OPTIONS_ERROR_PROPERTY),
            Some(OPTIONS_ERROR_MESSAGE),
            "保存成功不应留下错误属性"
        );
        let text = std::fs::read_to_string(dir.join(OPTIONS_FILE)).expect("read");
        assert!(text.contains("tiger_sentence_early_commit: true"), "{text}");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn save_preserves_unknown_keys() {
        let dir = temp_dir("preserve");
        std::fs::write(
            dir.join(OPTIONS_FILE),
            "options:\n  some_other_option: true\ncustom: 1\n",
        )
        .expect("write");
        let mut store = OptionsStore::load(&dir, &crate::options::test_option_keys());
        let mut context = Context::new();
        store.sync(&mut context);
        context.set_option("tiger_sentence_early_commit", true);
        store.observe(&mut context, "tiger_sentence_early_commit");
        // 未知键（`options:` 内与其他顶层键）原样保留
        let text = std::fs::read_to_string(dir.join(OPTIONS_FILE)).expect("read");
        assert!(text.contains("some_other_option: true"), "{text}");
        assert!(text.contains("custom: 1"), "{text}");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn legacy_user_yaml_is_read_only_fallback() {
        let dir = temp_dir("legacy");
        std::fs::write(
            dir.join(LEGACY_FILE),
            "var:\n  option:\n    tiger_sentence_allow_duplicate_single: false\n",
        )
        .expect("write");
        let mut store = OptionsStore::load(&dir, &crate::options::test_option_keys());
        assert_eq!(
            store
                .options
                .values
                .get("tiger_sentence_allow_duplicate_single"),
            Some(&false)
        );
        let mut context = Context::new();
        store.sync(&mut context);
        assert!(!context.get_option("tiger_sentence_allow_duplicate_single"));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn provided_defaults_fill_missing_keys() {
        let dir = temp_dir("defaults");
        let defaults = HashMap::from([("tiger_sentence_early_commit".to_string(), false)]);
        let mut store = OptionsStore::load_with_defaults(&dir, defaults);
        let mut context = hux_core::session::Context::new();
        store.sync(&mut context);
        assert!(
            !context.get_option("tiger_sentence_early_commit"),
            "缺失键应回退到传入缺省"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn save_failure_sets_error_property() {
        let dir = temp_dir("error");
        // 目标路径是目录 → 写文件失败
        std::fs::create_dir_all(dir.join(OPTIONS_FILE)).expect("blocking dir");
        let mut store = OptionsStore::load(&dir, &crate::options::test_option_keys());
        let mut context = Context::new();
        store.sync(&mut context);
        drain_option_events(&mut store, &mut context);
        context.set_option("tiger_sentence_early_commit", false);
        store.observe(&mut context, "tiger_sentence_early_commit");
        assert_eq!(
            context.get_property(OPTIONS_ERROR_PROPERTY),
            Some(OPTIONS_ERROR_MESSAGE)
        );
        std::fs::remove_dir_all(&dir).ok();
    }
}
