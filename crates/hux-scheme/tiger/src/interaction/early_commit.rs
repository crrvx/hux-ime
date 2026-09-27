// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! 提前上屏（证据驱动的前缀提前上屏 / 空码整句提交）：门面与子模块接线。
//!
//! 子模块分工：`evidence` 证据与 tracker 判据，`submit` 共用提交件，
//! `capture` 空码候选捕获，`mature` 成熟前缀提交，`early` 提前上屏主入口，
//! `empty_code` 空码自动上屏。

mod capture;
mod early;
mod empty_code;
mod evidence;
mod mature;
mod submit;

/// 强证据（`strong_count`）看**纯模型**份额 `base_share`；个性化不再制造强证据。
pub(crate) const EARLY_COMMIT_STRONG_SHARE: f64 = 0.999;
pub(crate) const EARLY_COMMIT_REQUIRED_EVIDENCE: usize = 3;
pub(crate) const EARLY_COMMIT_REQUIRED_STRONG: usize = 2;
pub(crate) const EARLY_COMMIT_MAXIMUM_NEUTRAL_GAP: usize = 3;
pub(crate) const EARLY_COMMIT_RETAINED_RAW_LENGTH: usize = 3;

/// 参照 `has_selection_suffix`：显式选重后缀（分号/引号/数字）。
///
/// 判定实现在 [`crate::decode::has_selection_suffix`]（beam 侧与交互侧共用一份）。
/// 撇号在此是**选重**后缀，与断音 [`crate::sound_to_char_shape::SYLLABLE_DELIMITER`] 同名不同义。
pub(crate) use crate::decode::has_selection_suffix;

pub(crate) use capture::*;
pub(crate) use early::*;
pub(crate) use empty_code::*;
pub(crate) use evidence::*;
pub(crate) use mature::*;
pub(crate) use submit::*;
