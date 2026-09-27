// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! 设置类型与常量：候选排列、预编辑内容与上限常量。

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
