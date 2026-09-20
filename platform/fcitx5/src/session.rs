// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! 会话：每个输入上下文（窗口/输入框）一份，互不干扰（fcitx5 `InputContextProperty`）。

use hux_core::interaction::{CompositionBuilder, LiveLearning, SentenceState};
use hux_core::session::Context;

/// 字反查（⑧-2）会话态：周边文本（字符制光标）+ 窗口起点 + 已算好的提示。
#[derive(Default)]
pub(crate) struct CharToSoundShapeState {
    pub(crate) valid: bool,
    pub(crate) text: String,
    pub(crate) cursor: usize,
    pub(crate) aux_up: String,
    pub(crate) aux_down: String,
}

pub(crate) struct Session {
    pub(crate) context: Context,
    pub(crate) state: SentenceState,
    pub(crate) live: LiveLearning,
    pub(crate) dot_armed: bool,
    pub(crate) min_retained: Option<i64>,
    /// 组合重建（提交或输入变化时重建，保留段状态含菜单高亮）。
    pub(crate) builder: CompositionBuilder,
    /// 字反查（⑧-2）会话态。
    pub(crate) char_to_sound_shape: CharToSoundShapeState,
}
