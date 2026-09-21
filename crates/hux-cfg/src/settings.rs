// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! addon 配置模型（Rust 半）：外部设置（fcitx5 配置界面 / 测试）与内建缺省。
//!
//! 合并顺序照参照 schema 语义：**`tiger_sentence.options.yaml`（user 覆盖） > 本设置 > 内建缺省**；
//! 可持久化开关（提前上屏、提前上屏至预编辑、单字重码组句、全角标点、数字直选 5 项）
//! 以本设置为存储层缺省（配置 / 状态菜单变更后
//! 经 `apply_settings` 重放存储，`options.yaml` 仍优先）；`ascii_punct` 等作会话初始选项；
//! `tab_learning` 门控学习 mode（`false` → 空串 = 不学习，对照参照 `prepare_learning` 的 `enabled`），
//! `high_freq_limit` 在创建词库时生效（修改需重启）。

use hux_core::host::{DEFAULT_PAGE_SIZE, HostOptions, MAX_PAGE_SIZE};
use hux_core::key::KeyEvent;
use hux_core::scheme::OptionIds;

/// 高频字过滤上限的缺省值（词库创建时生效；`0` = 不限）。
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

/// 最短保留码数上限（配置页 `MinRetainedRawLength` 的钳制值）。
pub const MAX_MIN_RETAINED_RAW_LENGTH: usize = 20;

/// 虎句方案引擎设置（与参照 schema / fcitx5 配置界面一一对应）。
#[derive(Clone, Debug, PartialEq)]
pub struct Settings {
    /// 提前上屏（选项键由方案声明，见 `hux_core::scheme::OptionIds`，本层不写死）。
    pub early_commit: bool,
    /// 提前上屏至预编辑（选项键同上）。
    pub early_commit_to_preedit: bool,
    /// 单字重码组句（选项键同上）。
    pub allow_duplicate_single: bool,
    /// 全角标点（`full_shape`）。
    pub full_shape: bool,
    /// ASCII 标点（`ascii_punct`）。
    pub ascii_punct: bool,
    /// Tab 选字写入学习库（参照 `tiger_sentence/tab_learning`）。
    pub tab_learning: bool,
    /// 高频字过滤上限（参照 `tiger_sentence/high_freq_limit`；创建词库时生效）。
    pub high_freq_limit: usize,
    /// 音反查（拼音反查码）/ 字反查（查光标处汉字的音与虎码）触发键（rime 键名，可多项）。
    pub sound_to_char_shape_keys: Vec<String>,
    pub char_to_sound_shape_keys: Vec<String>,
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
    /// 提前上屏/空码上屏的最短保留编码数（参照 `tiger_sentence/min_retained_raw_length`；
    /// 0 = 不额外限制；钳制到 `0..=`[`MAX_MIN_RETAINED_RAW_LENGTH`]）。
    pub min_retained_raw_length: usize,
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
            sound_to_char_shape_keys: vec!["Alt+colon".to_string()],
            char_to_sound_shape_keys: vec!["Alt+quotedbl".to_string()],
            page_size: DEFAULT_PAGE_SIZE,
            page_up_keys: vec!["minus".to_string(), "bracketleft".to_string()],
            page_down_keys: vec!["equal".to_string(), "bracketright".to_string()],
            digit_select: true,
            candidate_layout: CandidateLayout::FollowGlobal,
            preedit_mode: PreeditMode::CandidateCode,
            page_cycle: false,
            min_retained_raw_length: 0,
        }
    }
}

impl Settings {
    /// 会话初始选项（写入 context；`options.yaml` 的同名项随后覆盖）。
    /// `ids` 由平台从方案取得（见 `hux_core::scheme::OptionIds`）。
    pub fn option_defaults(&self, ids: &OptionIds) -> Vec<(&'static str, bool)> {
        vec![
            (ids.early_commit, self.early_commit),
            (ids.early_commit_to_preedit, self.early_commit_to_preedit),
            (ids.allow_duplicate_single, self.allow_duplicate_single),
            ("full_shape", self.full_shape),
            ("ascii_punct", self.ascii_punct),
            (ids.digit_select, self.digit_select),
        ]
    }

    /// 单项设置缺省（[`Settings::option_defaults`] 的查询形式）。
    pub fn option_default(&self, ids: &OptionIds, name: &str) -> Option<bool> {
        self.option_defaults(ids)
            .into_iter()
            .find(|(key, _)| *key == name)
            .map(|(_, value)| value)
    }

    /// 存储层缺省：可持久化的核心开关（`options.yaml` 缺失键回退到这些值）。
    pub fn store_defaults(&self, ids: &OptionIds) -> hashbrown::HashMap<String, bool> {
        [
            (ids.early_commit, self.early_commit),
            (ids.early_commit_to_preedit, self.early_commit_to_preedit),
            (ids.allow_duplicate_single, self.allow_duplicate_single),
            ("full_shape", self.full_shape),
            (ids.digit_select, self.digit_select),
        ]
        .into_iter()
        .map(|(name, value)| (name.to_string(), value))
        .collect()
    }

    /// 最短保留码数（钳制到 `0..=`[`MAX_MIN_RETAINED_RAW_LENGTH`]）。
    pub fn min_retained(&self) -> usize {
        self.min_retained_raw_length
            .min(MAX_MIN_RETAINED_RAW_LENGTH)
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
        assert!(settings.tab_learning);
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
            settings.sound_to_char_shape_keys,
            vec!["Alt+colon".to_string()]
        );
        assert_eq!(
            settings.char_to_sound_shape_keys,
            vec!["Alt+quotedbl".to_string()]
        );
        assert!(settings.digit_select);
        assert_eq!(settings.candidate_layout, CandidateLayout::FollowGlobal);
        assert_eq!(settings.preedit_mode, PreeditMode::CandidateCode);
        assert!(!settings.page_cycle);
        assert_eq!(settings.min_retained_raw_length, 0);
        assert_eq!(settings.min_retained(), 0);
    }

    #[test]
    fn min_retained_clamps_upper_bound() {
        let settings = Settings {
            min_retained_raw_length: 999,
            ..Default::default()
        };
        assert_eq!(settings.min_retained(), MAX_MIN_RETAINED_RAW_LENGTH);
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
        assert!(options.page_down_keys.is_empty(), "空列表 = 不绑定翻页键");
    }

    #[test]
    fn option_defaults_follow_settings() {
        let settings = Settings {
            full_shape: true,
            ..Default::default()
        };
        let ids = crate::options::test_option_ids();
        let defaults = settings.option_defaults(&ids);
        assert!(defaults.contains(&("full_shape", true)));
        assert!(defaults.contains(&(ids.early_commit, true)));
    }

    #[test]
    fn store_defaults_cover_core_switches() {
        let ids = crate::options::test_option_ids();
        let store_defaults = Settings::default().store_defaults(&ids);
        assert_eq!(
            store_defaults.get(ids.early_commit_to_preedit),
            Some(&false)
        );
        assert_eq!(store_defaults.get("full_shape"), Some(&false));
        assert_eq!(store_defaults.get(ids.digit_select), Some(&true));
        assert_eq!(store_defaults.len(), 5);
    }
}
