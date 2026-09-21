// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

use hashbrown::HashMap;

#[cfg(test)]
use crate::roles::ROLE_DIGIT_SELECT;
use crate::roles::{
    OptionKeys, ROLE_ALLOW_DUPLICATE_SINGLE, ROLE_EARLY_COMMIT, ROLE_EARLY_COMMIT_TO_PREEDIT,
};
#[cfg(test)]
use hux_core::scheme::OptionDecl;
use hux_core::session::Context;

/// 参照 `M.options` 的内建缺省表（键 = 方案声明的选项键；本层不硬编码方案选项名）。
pub fn option_defaults(keys: &OptionKeys) -> HashMap<String, bool> {
    let mut defaults = HashMap::new();
    for (role, value) in [
        (ROLE_EARLY_COMMIT, true),
        (ROLE_ALLOW_DUPLICATE_SINGLE, true),
        (ROLE_EARLY_COMMIT_TO_PREEDIT, false),
    ] {
        if let Some(key) = keys.key(role) {
            defaults.insert(key.to_string(), value);
        }
    }
    defaults
}

/// 测试用选项键表（字面量即**持久化契约**的钉桩：`options.yaml` 的历史键不得改名；
/// 生产路径由平台从方案声明解析，故本表与方案实现无编译期关系）。
#[cfg(test)]
pub(crate) fn test_option_keys() -> OptionKeys {
    OptionKeys::resolve(&[
        OptionDecl {
            role: ROLE_EARLY_COMMIT,
            key: "tiger_sentence_early_commit",
        },
        OptionDecl {
            role: ROLE_EARLY_COMMIT_TO_PREEDIT,
            key: "tiger_sentence_early_commit_to_preedit",
        },
        OptionDecl {
            role: ROLE_ALLOW_DUPLICATE_SINGLE,
            key: "tiger_sentence_allow_duplicate_single",
        },
        OptionDecl {
            role: ROLE_DIGIT_SELECT,
            key: "tiger_sentence_digit_select",
        },
    ])
    .expect("测试声明应覆盖全部方案角色")
}

/// 参照 `M.options` 的选项状态（文件读写与错误属性由 [`crate::OptionsStore`] 承担）。
#[derive(Clone, Debug, Default)]
pub struct Options {
    /// 设置缺省（平台经 [`OptionsStore::set_defaults`] 传入；缺省表见 [`option_defaults`]）。
    pub defaults: HashMap<String, bool>,
    /// 持久化值（`options/<name>`；缺失时回退设置缺省，读时还可回退 `user.yaml` 的 `var/option/<name>`）。
    pub values: HashMap<String, bool>,
    pub revision: u64,
}

impl Options {
    pub fn new(defaults: HashMap<String, bool>) -> Self {
        Self {
            defaults,
            values: HashMap::new(),
            revision: 0,
        }
    }

    /// 直接写入持久化值（无会话时的状态菜单切换用；返回是否发生变化）。
    pub fn set_value(&mut self, name: &str, value: bool) -> bool {
        if !self.defaults.contains_key(name) || self.values.get(name) == Some(&value) {
            return false;
        }
        self.values.insert(name.to_string(), value);
        self.revision += 1;
        true
    }

    /// 该选项是否有声明的缺省（⇒ 由本存储管理，可持久化）。
    pub fn covers(&self, name: &str) -> bool {
        self.defaults.contains_key(name)
    }

    /// 参照 `M.options.sync`：把持久化值（缺省回退缺省表）同步进上下文选项。
    /// 写入按选项名排序（事件顺序确定）；这些写入不会被 [`Options::observe`] 记为
    /// 用户改动（参照的 `live.syncing` 抑制）。
    pub fn sync(&mut self, context: &mut Context) {
        let mut names: Vec<&String> = self.defaults.keys().collect();
        names.sort();
        let mut written = Vec::new();
        for name in names {
            let fallback = self.defaults[name];
            let value = self.values.get(name).copied().unwrap_or(fallback);
            if context.get_option(name) != value {
                context.set_option(name, value);
                written.push(name.clone());
            }
        }
        // 参照 `live.syncing`：**自身写入**的选项通知不得被当成用户改动。
        context.discard_option_events(&written);
    }

    /// 参照 `option_update_notifier` 回调：记录变更并递增 revision；
    /// 返回是否需要持久化（写文件与失败属性由 K3 处理）。
    /// `sync` 自身写入的选项事件在此被忽略（参照 `live.syncing`）。
    pub fn observe(&mut self, context: &Context, name: &str) -> bool {
        if !self.defaults.contains_key(name) {
            return false;
        }
        let value = context.get_option(name);
        if self.values.get(name) == Some(&value) {
            return false;
        }
        self.values.insert(name.to_string(), value);
        self.revision += 1;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]

    fn options_sync_applies_defaults() {
        let mut context = Context::new();

        let mut options = Options::new(option_defaults(&test_option_keys()));

        options.sync(&mut context);

        assert!(context.get_option("tiger_sentence_early_commit"));

        assert!(context.get_option("tiger_sentence_allow_duplicate_single"));

        assert!(!context.get_option("tiger_sentence_early_commit_to_preedit"));
    }

    #[test]

    fn options_sync_discards_own_option_events() {
        let mut context = Context::new();

        let mut options = Options::new(option_defaults(&test_option_keys()));

        options.sync(&mut context);

        // 参照 `live.syncing`：sync 自身写入的选项通知在**写入时**即被丢弃，
        // 不会残留到稍后被当成用户改动（否则会吞掉紧随其后的第一次真实改动）。
        assert!(
            !context
                .drain_events()
                .iter()
                .any(|event| matches!(event, hux_core::session::Event::Option(_))),
            "sync 自身的事件不应留在队列中"
        );
        assert_eq!(options.revision, 0);

        // 随后的真实改动仍应被观察（对照参照：抑制只在写入那一次生效）。
        context.set_option("tiger_sentence_early_commit", false);
        assert!(options.observe(&context, "tiger_sentence_early_commit"));
        assert_eq!(options.revision, 1);
    }

    #[test]

    fn options_observe_records_user_change_once() {
        let mut context = Context::new();

        let mut options = Options::new(option_defaults(&test_option_keys()));

        options.sync(&mut context);

        // 用户改选项 → observe 记录并请求持久化；重复观察不再请求

        context.set_option("tiger_sentence_early_commit_to_preedit", true);

        assert!(options.observe(&context, "tiger_sentence_early_commit_to_preedit"));

        assert_eq!(options.revision, 1);

        assert!(!options.observe(&context, "tiger_sentence_early_commit_to_preedit"));

        assert!(!options.observe(&context, "other_option"));
    }

    #[test]

    fn options_sync_prefers_persisted_values() {
        let mut context = Context::new();

        let mut options = Options::new(option_defaults(&test_option_keys()));

        options.sync(&mut context);

        // 持久化值优先于 schema 缺省

        options
            .values
            .insert("tiger_sentence_early_commit".to_string(), false);

        context.set_option("tiger_sentence_early_commit", true);

        options.sync(&mut context);

        assert!(!context.get_option("tiger_sentence_early_commit"));
    }
}
