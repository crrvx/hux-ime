// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! 设置模型：结构、缺省值与派生视图（留存长度、宿主选项）。

use super::types::{
    CandidateLayout, DEFAULT_HIGH_FREQ_LIMIT, MAX_MIN_RETAINED_INPUT_LENGTH, PreeditMode,
};
use hux_core::host::{DEFAULT_PAGE_SIZE, HostOptions, MAX_PAGE_SIZE};
use hux_core::key::KeyEvent;

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
