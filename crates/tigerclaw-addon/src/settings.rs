//! addon 配置模型（Rust 半）：外部设置（fcitx5 配置界面 / 测试）与内建缺省。
//!
//! 合并顺序照参照 schema 语义：**`tiger_sentence.options.yaml`（user 覆盖） > 本设置 > 内建缺省**；
//! 三项早提交选项由存储层负责覆盖（C++ 对话框经 ABI 传入后将成为存储层缺省，待接线），
//! `full_shape`/`ascii_punct` 直接作为会话初始选项，`tab_learning` 门控学习 mode（`false` → 空串 = 不学习，
//! 对照参照 `prepare_learning` 的 `enabled`），`high_freq_limit` 在创建词库时生效（修改需重启）。

use tigerclaw_core::interaction::{
    OPTION_ALLOW_DUPLICATE_SINGLE, OPTION_EARLY_COMMIT, OPTION_EARLY_COMMIT_TO_PREEDIT,
};
use tigerclaw_core::lexicon::DEFAULT_HIGH_FREQ_LIMIT;

/// 虎整句方案引擎设置（与参照 schema / fcitx5 配置界面一一对应）。
#[derive(Clone, Debug, PartialEq)]
pub struct Settings {
    /// 提前上屏（`tiger_sentence_early_commit`）。
    pub early_commit: bool,
    /// 提前上屏至预编辑（`tiger_sentence_early_commit_to_preedit`）。
    pub early_commit_to_preedit: bool,
    /// 单字重码组句（`tiger_sentence_allow_duplicate_single`）。
    pub allow_duplicate_single: bool,
    /// 全角标点（`full_shape`）。
    pub full_shape: bool,
    /// ASCII 标点（`ascii_punct`）。
    pub ascii_punct: bool,
    /// Tab 选字写入学习库（参照 `tiger_sentence/tab_learning`）。
    pub tab_learning: bool,
    /// 高频字过滤上限（参照 `tiger_sentence/high_freq_limit`；创建词库时生效）。
    pub high_freq_limit: usize,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            early_commit: true,
            early_commit_to_preedit: false,
            allow_duplicate_single: true,
            full_shape: false,
            ascii_punct: false,
            tab_learning: true,
            high_freq_limit: DEFAULT_HIGH_FREQ_LIMIT,
        }
    }
}

impl Settings {
    /// 会话初始选项（写入 context；`options.yaml` 的同名项随后覆盖）。
    pub fn option_defaults(&self) -> Vec<(&'static str, bool)> {
        vec![
            (OPTION_EARLY_COMMIT, self.early_commit),
            (OPTION_EARLY_COMMIT_TO_PREEDIT, self.early_commit_to_preedit),
            (OPTION_ALLOW_DUPLICATE_SINGLE, self.allow_duplicate_single),
            ("full_shape", self.full_shape),
            ("ascii_punct", self.ascii_punct),
        ]
    }

    /// 存储层缺省：三项早提交选项（`options.yaml` 缺失键回退到这些值）。
    pub fn store_defaults(&self) -> hashbrown::HashMap<String, bool> {
        [
            (OPTION_EARLY_COMMIT, self.early_commit),
            (OPTION_EARLY_COMMIT_TO_PREEDIT, self.early_commit_to_preedit),
            (OPTION_ALLOW_DUPLICATE_SINGLE, self.allow_duplicate_single),
        ]
        .into_iter()
        .map(|(name, value)| (name.to_string(), value))
        .collect()
    }

    /// 拼学习 mode 串（参照 `prepare_learning`：关闭 Tab 学习 → 空串 = 不记录）。
    pub fn learning_mode(&self, rules: &str, duplicate: u8) -> String {
        if !self.tab_learning {
            return String::new();
        }
        format!(
            "sentence-v1|rules={rules}|optimal={}|dup={duplicate}",
            self.high_freq_limit
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_builtin_semantics() {
        let settings = Settings::default();
        assert!(settings.early_commit);
        assert!(!settings.early_commit_to_preedit);
        assert!(settings.allow_duplicate_single);
        assert!(!settings.full_shape);
        assert!(!settings.ascii_punct);
        assert!(settings.tab_learning);
        assert_eq!(settings.high_freq_limit, DEFAULT_HIGH_FREQ_LIMIT);
    }

    #[test]
    fn option_defaults_and_learning_mode() {
        let settings = Settings {
            full_shape: true,
            tab_learning: false,
            high_freq_limit: 100,
            ..Default::default()
        };
        let defaults = settings.option_defaults();
        assert!(defaults.contains(&("full_shape", true)));
        assert!(defaults.contains(&(OPTION_EARLY_COMMIT, true)));
        assert_eq!(settings.learning_mode("abc", 1), "");
        let store_defaults = settings.store_defaults();
        assert_eq!(
            store_defaults.get(OPTION_EARLY_COMMIT_TO_PREEDIT),
            Some(&false)
        );
        assert_eq!(store_defaults.len(), 3);
        let with_learning = Settings {
            high_freq_limit: 100,
            ..Default::default()
        };
        assert_eq!(
            with_learning.learning_mode("abc", 1),
            "sentence-v1|rules=abc|optimal=100|dup=1"
        );
    }
}
