// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! 会话：每个输入上下文（窗口/输入框）一份，互不干扰（fcitx5 `InputContextProperty`）。

use hux_core::scheme::SessionId;
use hux_core::session::Context;

/// 字反查会话态：应用侧周边文本（字符制光标）+ 已算好的两排提示。
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
    /// 方案会话句柄（组合、学习暂存、锁、早提交等状态都在方案内，平台只透传）。
    pub(crate) scheme_session: SessionId,
    /// 字反查会话态：应用侧周边文本 + 算好的两排提示。
    pub(crate) char_to_sound_shape: CharToSoundShapeState,
}
