// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! addon 配置模型（Rust 半）：外部设置（fcitx5 配置界面 / 测试）与内建缺省。
//!
//! 合并顺序照参照 schema 语义：**`tiger_sentence.options.yaml`（user 覆盖） > 本设置 > 内建缺省**；
//! 可持久化开关（提前上屏、提前上屏至预编辑、单字重码组句、全角标点、数字直选 5 项）
//! 以本设置为存储层缺省（配置 / 状态菜单变更后
//! 经 `apply_settings` 重放存储，`options.yaml` 仍优先）；`ascii_punct` 等作会话初始选项；
//! `learning_on_tab` 门控学习 mode（`false` → 空串 = 不学习，对照参照 `prepare_learning` 的 `enabled`；
//! 线上键 = 上游方案选项 `tiger_sentence/tab_learning`），
//! `high_freq_limit` 变更即时重建词库（见方案的 `apply_config`）。
//!
//! 本层拥有设置词汇（涉及方案与宿主的字段名即角色名，见 [`crate::roles`]）；**选项键由方案声明**
//! （`hux_core::scheme::OptionDecl`），故各入口接收已解析的 [`OptionKeys`] 而不是硬编码键名。

use crate::roles::{
    OptionKeys, ROLE_ALLOW_DUPLICATE_SINGLE, ROLE_ASCII_PUNCT, ROLE_DIGIT_SELECT,
    ROLE_EARLY_COMMIT, ROLE_EARLY_COMMIT_TO_PREEDIT, ROLE_FULL_SHAPE,
};
use hux_core::collections::Map;
use hux_core::host::{DEFAULT_PAGE_SIZE, HostOptions, MAX_PAGE_SIZE};
use hux_core::key::KeyEvent;

/// 高频字过滤上限的缺省值（`0` = 不限；变更即时生效）。
pub const DEFAULT_HIGH_FREQ_LIMIT: usize = 1500;

/// 候选排列（参照 `style` 语义）。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CandidateLayout {
    /// 跟随 fcitx5 全局「候选竖排」设置（默认，不设布局提示；选字键语义维持横排）。
    #[default]
    FollowGlobal,
    Horizontal,
    Vertical,
}

/// 预编辑内容（桌面显示；默认候选分码，即历史行为）。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PreeditMode {
    /// 高亮候选分码优先（现状）。
    #[default]
    CandidateCode,
    /// 原始输入（缓冲 + 实况输入）。
    RawInput,
    /// 不显示预编辑。
    Hidden,
}

/// 最短保留输入长度上限（配置页 `MinRetainedRawLength` 的钳制值）。
pub const MAX_MIN_RETAINED_INPUT_LENGTH: usize = 20;

/// 虎句方案引擎设置（与参照 schema / fcitx5 配置界面一一对应）。
#[derive(Clone, Debug, PartialEq)]
pub struct Settings {
    /// 提前上屏（选项键由方案声明，见 `hux_core::scheme::OptionDecl`，本层不写死）。
    pub early_commit: bool,
    /// 提前上屏至预编辑（选项键同上）。
    pub early_commit_to_preedit: bool,
    /// 单字重码组句（选项键同上）。
    pub allow_duplicate_single: bool,
    /// 全角标点（选项键 = rime 标准名 `full_shape`）。
    pub full_shape: bool,
    /// ASCII 标点（选项键 = rime 标准名 `ascii_punct`）。
    pub ascii_punct: bool,
    /// Tab 确认即写入学习库（线上键 = 上游选项 `tiger_sentence/tab_learning`）。
    pub learning_on_tab: bool,
    /// 高频字过滤上限（参照 `tiger_sentence/high_freq_limit`；变更即时重建词库）。
    pub high_freq_limit: usize,
    /// 反查触发键（rime 键名，可多项）：按**读音**入口（输入读音列出对应字词）与
    /// 按**字符**入口（取光标处字符列出其读音与编码）。
    pub reverse_lookup_pronunciation_keys: Vec<String>,
    pub reverse_lookup_character_keys: Vec<String>,
    /// 每页候选个数（参照 `menu/page_size`；上限 [`MAX_PAGE_SIZE`]）。
    pub page_size: usize,
    /// 上/下翻页键（rime 键名，可多项；缺省对应参照 `key_binder` 的 `-`/`=`）。
    pub page_up_keys: Vec<String>,
    pub page_down_keys: Vec<String>,
    /// 数字直选（addon 扩展，默认开）：菜单可见时数字直接上屏当前页候选（1–9；0=10）。
    pub digit_select: bool,
    /// 候选排列（横排/竖排）。
    pub candidate_layout: CandidateLayout,
    /// 预编辑内容（候选分码/原始输入/不显示）。
    pub preedit_mode: PreeditMode,
    /// 翻页循环（参照 `menu/page_down_cycle`，默认关）。
    pub page_cycle: bool,
    /// 提前上屏/空码上屏的最短保留**输入**长度（线上键 = 上游选项
    /// `tiger_sentence/min_retained_raw_length`；0 = 不额外限制；
    /// 钳制到 `0..=`[`MAX_MIN_RETAINED_INPUT_LENGTH`]）。
    pub min_retained_input_length: usize,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            early_commit: true,
            early_commit_to_preedit: false,
            allow_duplicate_single: true,
            full_shape: false,
            ascii_punct: false,
            learning_on_tab: true,
            high_freq_limit: DEFAULT_HIGH_FREQ_LIMIT,
            reverse_lookup_pronunciation_keys: vec!["grave".to_string()],
            reverse_lookup_character_keys: vec!["asciitilde".to_string()],
            page_size: DEFAULT_PAGE_SIZE,
            page_up_keys: vec!["minus".to_string(), "bracketleft".to_string()],
            page_down_keys: vec!["equal".to_string(), "bracketright".to_string()],
            digit_select: true,
            candidate_layout: CandidateLayout::FollowGlobal,
            preedit_mode: PreeditMode::CandidateCode,
            page_cycle: false,
            min_retained_input_length: 0,
        }
    }
}

impl Settings {
    /// 会话初始选项（写入 context；`options.yaml` 的同名项随后覆盖）。
    /// `keys` 由平台在装配处从方案声明解析（见 [`crate::roles::OptionKeys`]）；
    /// 方案未声明的角色不参与接线。
    pub fn option_defaults(&self, keys: &OptionKeys) -> Vec<(&'static str, bool)> {
        let mut defaults = Vec::new();
        // 顺序即写入顺序（保持既有顺序：三个方案开关 → 宿主标准项 → 数字直选）。
        for (role, value) in [
            (ROLE_EARLY_COMMIT, self.early_commit),
            (ROLE_EARLY_COMMIT_TO_PREEDIT, self.early_commit_to_preedit),
            (ROLE_ALLOW_DUPLICATE_SINGLE, self.allow_duplicate_single),
        ] {
            if let Some(key) = keys.key(role) {
                defaults.push((key, value));
            }
        }
        defaults.push((ROLE_FULL_SHAPE, self.full_shape));
        defaults.push((ROLE_ASCII_PUNCT, self.ascii_punct));
        if let Some(key) = keys.key(ROLE_DIGIT_SELECT) {
            defaults.push((key, self.digit_select));
        }
        defaults
    }

    /// 单项设置缺省（[`Settings::option_defaults`] 的查询形式）。
    pub fn option_default(&self, keys: &OptionKeys, name: &str) -> Option<bool> {
        self.option_defaults(keys)
            .into_iter()
            .find(|(key, _)| *key == name)
            .map(|(_, value)| value)
    }

    /// 存储层缺省：可持久化的核心开关（`options.yaml` 缺失键回退到这些值）。
    pub fn store_defaults(&self, keys: &OptionKeys) -> Map<String, bool> {
        let mut defaults: Map<String, bool> = Map::new();
        for (role, value) in [
            (ROLE_EARLY_COMMIT, self.early_commit),
            (ROLE_EARLY_COMMIT_TO_PREEDIT, self.early_commit_to_preedit),
            (ROLE_ALLOW_DUPLICATE_SINGLE, self.allow_duplicate_single),
            (ROLE_DIGIT_SELECT, self.digit_select),
        ] {
            if let Some(key) = keys.key(role) {
                defaults.insert(key.to_string(), value);
            }
        }
        // 宿主标准项的角色名即键名：不依赖方案声明。
        defaults.insert(ROLE_FULL_SHAPE.to_string(), self.full_shape);
        defaults
    }

    /// 最短保留输入长度（钳制到 `0..=`[`MAX_MIN_RETAINED_INPUT_LENGTH`]）。
    pub fn min_retained(&self) -> usize {
        self.min_retained_input_length
            .min(MAX_MIN_RETAINED_INPUT_LENGTH)
    }

    /// 宿主选项（翻页键与页大小）：键名解析失败项忽略；页大小钳制到 `1..=MAX_PAGE_SIZE`。
    pub fn host_options(&self) -> HostOptions {
        let parse = |reprs: &[String]| -> Vec<KeyEvent> {
            reprs
                .iter()
                .filter_map(|repr| KeyEvent::from_repr(repr))
                .collect()
        };
        HostOptions {
            page_size: self.page_size.clamp(1, MAX_PAGE_SIZE),
            page_up_keys: parse(&self.page_up_keys),
            page_down_keys: parse(&self.page_down_keys),
            page_cycle: self.page_cycle,
        }
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
        assert!(settings.learning_on_tab);
        assert_eq!(settings.high_freq_limit, DEFAULT_HIGH_FREQ_LIMIT);
        assert_eq!(settings.page_size, DEFAULT_PAGE_SIZE);
        assert_eq!(
            settings.page_up_keys,
            vec!["minus".to_string(), "bracketleft".to_string()]
        );
        assert_eq!(
            settings.page_down_keys,
            vec!["equal".to_string(), "bracketright".to_string()]
        );
        assert_eq!(
            settings.reverse_lookup_pronunciation_keys,
            vec!["grave".to_string()]
        );
        assert_eq!(
            settings.reverse_lookup_character_keys,
            vec!["asciitilde".to_string()]
        );
        assert!(settings.digit_select);
        assert_eq!(settings.candidate_layout, CandidateLayout::FollowGlobal);
        assert_eq!(settings.preedit_mode, PreeditMode::CandidateCode);
        assert!(!settings.page_cycle);
        assert_eq!(settings.min_retained_input_length, 0);
        assert_eq!(settings.min_retained(), 0);
    }

    #[test]
    fn min_retained_clamps_upper_bound() {
        let settings = Settings {
            min_retained_input_length: 999,
            ..Default::default()
        };
        assert_eq!(settings.min_retained(), MAX_MIN_RETAINED_INPUT_LENGTH);
    }

    #[test]
    fn host_options_clamp_page_size() {
        let low = Settings {
            page_size: 0,
            ..Default::default()
        }
        .host_options();
        assert_eq!(low.page_size, 1, "页大小下限为 1");
        let high = Settings {
            page_size: 999,
            ..Default::default()
        }
        .host_options();
        assert_eq!(high.page_size, MAX_PAGE_SIZE, "页大小上限为 10");
    }

    #[test]
    fn host_options_parse_keys_ignores_invalid() {
        let options = Settings {
            page_up_keys: vec!["comma".to_string(), "not-a-key".to_string()],
            page_down_keys: Vec::new(),
            ..Default::default()
        }
        .host_options();
        assert_eq!(
            options.page_up_keys,
            vec![KeyEvent::from_repr("comma").unwrap()]
        );
        // 契约：**显式给出即以此为准**（空列表 = 不绑定）。生产路径经配置袋把
        // 原始字符串交给方案（`hux-scheme/tiger` 的 `host_options_from`），
        // 两侧语义必须一致 —— 方案侧由 `empty_page_key_lists_unbind_the_keys` 钉住。
        assert!(options.page_down_keys.is_empty(), "空列表 = 不绑定翻页键");
    }

    #[test]
    fn option_defaults_follow_settings() {
        let settings = Settings {
            full_shape: true,
            ..Default::default()
        };
        let keys = crate::options::test_option_keys();
        let defaults = settings.option_defaults(&keys);
        assert!(defaults.contains(&("full_shape", true)));
        assert!(defaults.contains(&(keys.key(ROLE_EARLY_COMMIT).unwrap(), true)));
        // 顺序保持既有写入顺序（方案开关 → 宿主标准项 → 数字直选）。
        assert_eq!(
            defaults.iter().map(|(name, _)| *name).collect::<Vec<_>>(),
            vec![
                "tiger_sentence_early_commit",
                "tiger_sentence_early_commit_to_preedit",
                "tiger_sentence_allow_duplicate_single",
                "full_shape",
                "ascii_punct",
                "tiger_sentence_digit_select",
            ]
        );
    }

    #[test]
    fn store_defaults_cover_core_switches() {
        let keys = crate::options::test_option_keys();
        let store_defaults = Settings::default().store_defaults(&keys);
        let key = |role| keys.key(role).expect("测试表应完整");
        assert_eq!(
            store_defaults.get(key(ROLE_EARLY_COMMIT_TO_PREEDIT)),
            Some(&false)
        );
        assert_eq!(store_defaults.get("full_shape"), Some(&false));
        assert_eq!(store_defaults.get(key(ROLE_DIGIT_SELECT)), Some(&true));
        assert_eq!(store_defaults.len(), 5);
    }

    #[test]
    fn option_keys_absent_roles_are_skipped_not_faked() {
        // 方案未声明的角色**不接线**（不回落成角色名字面量，以免与方案键混淆）。
        let empty = OptionKeys::default();
        let defaults = Settings::default().option_defaults(&empty);
        assert_eq!(
            defaults.iter().map(|(name, _)| *name).collect::<Vec<_>>(),
            vec!["full_shape", "ascii_punct"],
            "仅宿主标准项保留"
        );
        assert_eq!(Settings::default().store_defaults(&empty).len(), 1);
    }
}
