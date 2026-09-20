// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

use hashbrown::HashMap;

use hux_core::scheme::OptionIds;
use hux_core::session::Context;

/// 参照 `M.options` 的内建缺省表（键 = 方案声明的选项 id；本层不硬编码方案选项名）。
pub fn option_defaults(ids: &OptionIds) -> HashMap<String, bool> {
    HashMap::from([
        (ids.early_commit.to_string(), true),
        (ids.allow_duplicate_single.to_string(), true),
        (ids.early_commit_to_preedit.to_string(), false),
    ])
}

/// 测试用选项 id（与 `hux-scheme-tiger` 的实际值一致；生产路径由平台从方案取得）。
#[cfg(test)]
pub(crate) fn test_option_ids() -> OptionIds {
    OptionIds {
        early_commit: "tiger_sentence_early_commit",
        early_commit_to_preedit: "tiger_sentence_early_commit_to_preedit",
        allow_duplicate_single: "tiger_sentence_allow_duplicate_single",
        digit_select: "tiger_sentence_digit_select",
    }
}

/// 参照 `M.options` 的配置存储（文件读写、错误属性由 K3 承担）。
#[derive(Clone, Debug, Default)]
pub struct Options {
    /// schema 缺省（`tiger_sentence/option_defaults/<name>`，回退内建缺省）。
    pub defaults: HashMap<String, bool>,
    /// 持久化值（`options/<name>`，缺省回退 `user.yaml` 的 `var/option/<name>`）。
    pub values: HashMap<String, bool>,
    pub revision: u64,
    /// `sync` 写入上下文、等待宿主回灌选项事件时跳过的选项名（参照 `live.syncing`）。
    sync_writes: Vec<String>,
}

impl Options {
    pub fn new(defaults: HashMap<String, bool>) -> Self {
        Self {
            defaults,
            values: HashMap::new(),
            revision: 0,
            sync_writes: Vec::new(),
        }
    }

    /// 参照 `M.options.sync`：把持久化值（缺省回退 schema 缺省）同步进上下文选项。
    /// 写入按选项名排序（事件顺序确定）；这些写入不会被 [`Options::observe`] 记为
    /// 用户改动（参照的 `live.syncing` 抑制）。
    pub fn sync(&mut self, context: &mut Context) {
        self.sync_writes.clear();
        let mut names: Vec<&String> = self.defaults.keys().collect();
        names.sort();
        for name in names {
            let fallback = self.defaults[name];
            let value = self.values.get(name).copied().unwrap_or(fallback);
            if context.get_option(name) != value {
                context.set_option(name, value);
                self.sync_writes.push(name.clone());
            }
        }
    }

    /// 参照 `option_update_notifier` 回调：记录变更并递增 revision；
    /// 返回是否需要持久化（写文件与失败属性由 K3 处理）。
    /// `sync` 自身写入的选项事件在此被忽略（参照 `live.syncing`）。
    pub fn observe(&mut self, context: &Context, name: &str) -> bool {
        if let Some(position) = self.sync_writes.iter().position(|value| value == name) {
            self.sync_writes.remove(position);
            return false;
        }
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

        let mut options = Options::new(option_defaults(&test_option_ids()));

        options.sync(&mut context);

        assert!(context.get_option("tiger_sentence_early_commit"));

        assert!(context.get_option("tiger_sentence_allow_duplicate_single"));

        assert!(!context.get_option("tiger_sentence_early_commit_to_preedit"));
    }

    #[test]

    fn options_observe_ignores_synced_writes() {
        let mut context = Context::new();

        let mut options = Options::new(option_defaults(&test_option_ids()));

        options.sync(&mut context);

        // sync 自身写入的选项事件不计为用户改动（参照 live.syncing 抑制）

        assert!(!options.observe(&context, "tiger_sentence_early_commit"));

        assert_eq!(options.revision, 0);
    }

    #[test]

    fn options_observe_records_user_change_once() {
        let mut context = Context::new();

        let mut options = Options::new(option_defaults(&test_option_ids()));

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

        let mut options = Options::new(option_defaults(&test_option_ids()));

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
