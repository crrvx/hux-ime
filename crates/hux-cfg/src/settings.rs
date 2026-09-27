// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! addon 配置模型（Rust 半）：外部设置（fcitx5 配置界面 / 测试）与内建缺省。
//!
//! 合并顺序照参照 schema 语义：**`tiger_sentence.options.yaml`（user 覆盖） > 本设置 > 内建缺省**；
//! 可持久化开关（提前上屏、提前上屏至预编辑、单字重码组句、全角标点、数字直选 5 项）
//! 以本设置为存储层缺省；**配置页推送时**本设置的值写回该文件（
//! [`crate::OptionsStore::set_values`]）——两侧是同一批项，故不再出现「`options.yaml` 的旧值
//! 压制配置页」；`ascii_punct` 等作会话初始选项；
//! `learning_on_tab` 门控学习 mode（`false` → 空串 = 不学习，对照参照 `prepare_learning` 的 `enabled`；
//! 线上键 = 上游方案选项 `tiger_sentence/tab_learning`），
//! `high_freq_limit` 变更即时重建词库（见方案的 `apply_config`）。
//!
//! 本层拥有设置词汇（涉及方案与宿主的字段名即角色名，见 [`crate::roles`]）；**选项键由方案声明**
//! （`hux_core::scheme::OptionDecl`），故各入口接收已解析的 [`OptionKeys`] 而不是硬编码键名。

use crate::roles::{
    OptionKeys, ROLE_ALLOW_DUPLICATE_SINGLE, ROLE_ASCII_PUNCT, ROLE_DIGIT_SELECT,
    ROLE_EARLY_COMMIT, ROLE_EARLY_COMMIT_TO_PREEDIT, ROLE_FILTER_NON_HAN, ROLE_FULL_CHARSET,
    ROLE_FULL_SHAPE,
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
    /// 启用全字集（addon 扩展，默认开）：关掉只装主表码表，不装追加码表；
    /// 变更即时重建词库（见方案的 `apply_config`）。
    pub full_charset: bool,
    /// 过滤非汉字（addon 扩展，默认开）：追加码表里的部首/笔画/注音/假名等不入词库
    /// （主表行不受影响）；变更即时重建词库。
    pub filter_non_han: bool,
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
            full_charset: true,
            filter_non_han: true,
            candidate_layout: CandidateLayout::FollowGlobal,
            preedit_mode: PreeditMode::CandidateCode,
            page_cycle: false,
            min_retained_input_length: 0,
        }
    }
}

impl Settings {
    /// 会话初始选项（写入 context；`options.yaml` 的同名项随后覆盖）：返回值**有序**，
    /// 顺序即写入顺序。与内建缺省表 [`crate::builtin_option_defaults`]（按名查询的 `Map`）
    /// 同名易混，故各按来源命名。
    /// `keys` 由平台在装配处从方案声明解析（见 [`crate::roles::OptionKeys`]）；
    /// 方案未声明的角色不参与接线。
    pub fn session_option_defaults(&self, keys: &OptionKeys) -> Vec<(&'static str, bool)> {
        let mut defaults = Vec::new();
        // 顺序即写入顺序（保持既有顺序：三个方案开关 → 宿主标准项 → 运行时开关
        // 按 `RUNTIME_OPTION_ROLES` 的先后：数字直选 → 全字集 → 过滤非汉字）。
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
        for (role, value) in [
            (ROLE_DIGIT_SELECT, self.digit_select),
            (ROLE_FULL_CHARSET, self.full_charset),
            (ROLE_FILTER_NON_HAN, self.filter_non_han),
        ] {
            if let Some(key) = keys.key(role) {
                defaults.push((key, value));
            }
        }
        defaults
    }

    /// 单项设置缺省（[`Settings::session_option_defaults`] 的查询形式）。
    pub fn session_option_default(&self, keys: &OptionKeys, name: &str) -> Option<bool> {
        self.session_option_defaults(keys)
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
            (ROLE_FULL_CHARSET, self.full_charset),
            (ROLE_FILTER_NON_HAN, self.filter_non_han),
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

    /// 缺省值是发布语义的一部分，改这里等于改用户的开箱行为，故逐项钉住。
    #[test]
    fn defaults_match_builtin_semantics() {
        let settings = Settings::default();
        assert!(settings.early_commit, "缺省应开启早提交");
        assert!(
            !settings.early_commit_to_preedit,
            "缺省不得开启「上屏前先进预编辑」"
        );
        assert!(settings.allow_duplicate_single, "缺省应允许单字重码");
        assert!(!settings.full_shape, "缺省不得是中文标点模式");
        assert!(!settings.ascii_punct, "缺省不得是英文标点模式");
        assert!(settings.learning_on_tab, "缺省应开启 Tab 学习");
        assert_eq!(
            settings.high_freq_limit, DEFAULT_HIGH_FREQ_LIMIT,
            "高频字上限缺省应为 DEFAULT_HIGH_FREQ_LIMIT"
        );
        assert_eq!(
            settings.page_size, DEFAULT_PAGE_SIZE,
            "页大小缺省应为 DEFAULT_PAGE_SIZE"
        );
        assert_eq!(
            settings.page_up_keys,
            vec!["minus".to_string(), "bracketleft".to_string()],
            "翻页上键缺省应为 minus 与 bracketleft"
        );
        assert_eq!(
            settings.page_down_keys,
            vec!["equal".to_string(), "bracketright".to_string()],
            "翻页下键缺省应为 equal 与 bracketright"
        );
        assert_eq!(
            settings.reverse_lookup_pronunciation_keys,
            vec!["grave".to_string()],
            "音反查键缺省应为 grave"
        );
        assert_eq!(
            settings.reverse_lookup_character_keys,
            vec!["asciitilde".to_string()],
            "字反查键缺省应为 asciitilde"
        );
        assert!(settings.digit_select, "缺省应开启数字选字");
        assert!(settings.full_charset, "缺省应开启全字集");
        assert!(settings.filter_non_han, "缺省应过滤非汉字");
        assert_eq!(
            settings.candidate_layout,
            CandidateLayout::FollowGlobal,
            "候选布局缺省跟随全局"
        );
        assert_eq!(
            settings.preedit_mode,
            PreeditMode::CandidateCode,
            "预编辑缺省显示候选编码"
        );
        assert!(!settings.page_cycle, "缺省不得开启翻页循环");
        assert_eq!(
            settings.min_retained_input_length, 0,
            "留存输入长度缺省为 0"
        );
        assert_eq!(
            settings.min_retained(),
            0,
            "缺省设置下 min_retained() 应给 0"
        );
    }

    /// 超限配置必须夹到上限而不是原样透传，避免宿主拿到无法兑现的留存长度。
    #[test]
    fn min_retained_clamps_upper_bound() {
        let settings = Settings {
            min_retained_input_length: 999,
            ..Default::default()
        };
        assert_eq!(
            settings.min_retained(),
            MAX_MIN_RETAINED_INPUT_LENGTH,
            "超上限的留存长度必须夹到 MAX_MIN_RETAINED_INPUT_LENGTH"
        );
    }

    /// 页大小的 0 与超大值都必须在交给宿主前落进合法区间。
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

    /// 配置里的键名是字符串，交给宿主前必须解析成键码；解析失败的项只丢自己，不牵连其余绑定。
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
            vec![KeyEvent::from_repr("comma").unwrap()],
            "非法键名必须丢弃，只留可解析的 comma"
        );
        // 契约：**显式给出即以此为准**（空列表 = 不绑定）。生产路径经配置袋把
        // 原始字符串交给方案（`hux-scheme/tiger` 的 `host_options_from`），
        // 两侧语义必须一致 —— 方案侧由 `empty_page_key_lists_unbind_the_keys` 钉住。
        assert!(options.page_down_keys.is_empty(), "空列表 = 不绑定翻页键");
    }

    /// 缺省下发顺序是方案与宿主约定的写入序，顺序变化会让宿主状态栏与配置页错位。
    #[test]
    fn session_option_defaults_follow_settings() {
        let settings = Settings {
            full_shape: true,
            ..Default::default()
        };
        let keys = crate::options::test_option_keys();
        let defaults = settings.session_option_defaults(&keys);
        assert!(
            defaults.contains(&("full_shape", true)),
            "full_shape 开启应作为会话选项缺省下发"
        );
        assert!(
            defaults.contains(&(keys.key(ROLE_EARLY_COMMIT).unwrap(), true)),
            "方案角色键经 OptionKeys 解析后同样要下发 true"
        );
        // 顺序保持既有写入顺序（方案开关 → 宿主标准项 → 运行时开关按角色序）。
        assert_eq!(
            defaults.iter().map(|(name, _)| *name).collect::<Vec<_>>(),
            vec![
                "tiger_sentence_early_commit",
                "tiger_sentence_early_commit_to_preedit",
                "tiger_sentence_allow_duplicate_single",
                "full_shape",
                "ascii_punct",
                "tiger_sentence_digit_select",
                "tiger_sentence_full_charset",
                "tiger_sentence_filter_non_han",
            ],
            "下发顺序固定为方案开关、宿主标准项、运行时开关按角色序"
        );
    }

    /// options.yaml 的缺省集合必须覆盖全部核心开关，漏写会让配置页缺项。
    #[test]
    fn store_defaults_cover_core_switches() {
        let keys = crate::options::test_option_keys();
        let store_defaults = Settings::default().store_defaults(&keys);
        let key = |role| keys.key(role).expect("测试表应完整");
        assert_eq!(
            store_defaults.get(key(ROLE_EARLY_COMMIT_TO_PREEDIT)),
            Some(&false),
            "早提交到预编辑缺省 false 必须落进 store 缺省"
        );
        assert_eq!(
            store_defaults.get("full_shape"),
            Some(&false),
            "宿主标准键 full_shape 的缺省必须落进 store 缺省"
        );
        assert_eq!(
            store_defaults.get(key(ROLE_DIGIT_SELECT)),
            Some(&true),
            "数字选字缺省 true 必须落进 store 缺省"
        );
        // 字集开关同样经 `apply_settings` 写回 `options.yaml`（缺省开）。
        assert_eq!(
            store_defaults.get(key(ROLE_FULL_CHARSET)),
            Some(&true),
            "全字集缺省 true 必须落进 store 缺省"
        );
        assert_eq!(
            store_defaults.get(key(ROLE_FILTER_NON_HAN)),
            Some(&true),
            "过滤非汉字缺省 true 必须落进 store 缺省"
        );
        assert_eq!(store_defaults.len(), 7, "store 缺省应恰好覆盖 7 个开关");
    }

    /// 方案没声明的角色不接线：不得回落成角色名字面量，否则会与真实方案键混淆。
    #[test]
    fn option_keys_absent_roles_are_skipped_not_faked() {
        // 方案未声明的角色**不接线**（不回落成角色名字面量，以免与方案键混淆）。
        let empty = OptionKeys::default();
        let defaults = Settings::default().session_option_defaults(&empty);
        assert_eq!(
            defaults.iter().map(|(name, _)| *name).collect::<Vec<_>>(),
            vec!["full_shape", "ascii_punct"],
            "仅宿主标准项保留"
        );
        assert_eq!(
            Settings::default().store_defaults(&empty).len(),
            1,
            "无方案声明时 store 缺省只剩宿主标准项 1 条"
        );
    }
}
