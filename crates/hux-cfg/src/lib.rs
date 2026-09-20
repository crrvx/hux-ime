// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! hux 自身的可配置项：设置项定义与默认值、选项存储与合并顺序。
//!
//! 合并顺序：`tiger_sentence.options.yaml`（用户覆盖） > 设置（fcitx5 配置界面/调用方） > 内建缺省；
//! 持久化实现见 [`OptionsStore`]，同步/观察语义见 [`Options`]（参照 `M.options`）。

mod options;
mod settings;
mod store;

pub use hux_core::scheme::OptionIds;
pub use options::{Options, option_defaults};
pub use settings::{
    CandidateLayout, DEFAULT_HIGH_FREQ_LIMIT, MAX_MIN_RETAINED_RAW_LENGTH, PreeditMode, Settings,
};
pub use store::{LEGACY_FILE, OPTIONS_ERROR_PROPERTY, OPTIONS_FILE, OptionsStore};
