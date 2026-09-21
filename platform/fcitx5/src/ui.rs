// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! UI 快照：preedit / 候选 / 辅助文本经宿主回调下发（`push_update`）。

use std::ffi::{CString, c_char};

use crate::engine::Engine;
use crate::session::Session;
use hux_cfg::PreeditMode;

/// 转 C 字符串：内嵌 NUL 会截断 C 侧字符串，故先剔除并记一次日志
/// （平台层允许直接打印；此前 `CString::new(..).unwrap_or_default()` 会把整条文本静默丢空）。
pub(crate) fn cstring_lossy(text: &str) -> CString {
    match CString::new(text) {
        Ok(value) => value,
        Err(_) => {
            eprintln!("hux: 文本含 NUL，已剔除后送出");
            CString::new(text.replace('\0', "")).unwrap_or_default()
        }
    }
}

impl Engine {
    pub(crate) fn push_update(&self, session: &Session) {
        let Some(host) = &self.host else {
            return;
        };
        let Some(update) = host.update else {
            return;
        };
        let buffered = self.scheme.buffered_text(&session.context);
        let live_bytes = session.context.live_input();
        let live = String::from_utf8_lossy(live_bytes).into_owned();
        // 参照 librime `Composition::GetPreedit` + 参照 Lua 的候选 preedit：
        // 高亮候选的 preedit（「按字分码」，含缓冲前缀与音反查前缀）始终优先；
        // 组合（光标）之后的原始输入原样接在其后——左/右移动时保持按字分码，
        // 光标落在分码文本末尾、原始尾部之前。
        let highlighted = session
            .context
            .composition
            .back()
            .and_then(|segment| segment.selected_candidate())
            .map(|candidate| candidate.preedit.clone())
            .unwrap_or_default();
        // 预编辑内容（`PreeditMode`）：候选分码（默认，历史行为）/ 原始输入 / 不显示。
        let preedit_mode = self.settings.preedit_mode;
        let (mut preedit, cursor) = if preedit_mode == PreeditMode::Hidden {
            (String::new(), 0)
        } else if preedit_mode == PreeditMode::CandidateCode && !highlighted.is_empty() {
            let cursor = highlighted.len();
            // 末段 `end` 为组合输入（含缓冲 `~` 标记）的字节位；换算到实况输入。
            let marker =
                usize::from(!buffered.is_empty() && session.context.input().first() == Some(&b'~'));
            let composed_end = session
                .context
                .composition
                .back()
                .map(|segment| segment.end)
                .unwrap_or(0)
                .saturating_sub(marker)
                .min(live_bytes.len());
            let mut text = highlighted;
            text.push_str(&String::from_utf8_lossy(&live_bytes[composed_end..]));
            (text, cursor)
        } else {
            // 无高亮候选（如未翻译段）：回退「缓冲 + 实况输入」，光标按字节对应。
            let mut text = String::new();
            text.push_str(&buffered);
            if !buffered.is_empty() && !live.is_empty() {
                text.push(' ');
            }
            let prefix_length = if buffered.is_empty() {
                0
            } else {
                buffered.len() + usize::from(!live.is_empty())
            };
            text.push_str(&live);
            let cursor = (prefix_length + session.context.live_caret()).min(text.len());
            (text, cursor)
        };
        // 参照 `Composition::GetPreedit`：段提示插在光标处（如音反查段的「〔拼音〕」）。
        let prompt = session
            .context
            .composition
            .back()
            .map(|segment| segment.prompt.clone())
            .unwrap_or_default();
        if preedit_mode == PreeditMode::CandidateCode && !prompt.is_empty() {
            preedit.insert_str(cursor.min(preedit.len()), &prompt);
        }
        // 字反查段不下发预编辑：避免应用端 marked text 锁住光标（←/→ 无法移动）。
        let mut cursor = cursor;
        if self.char_to_sound_shape_tagged(session) {
            preedit.clear();
            cursor = 0;
        }
        let (mut texts, mut comments, selected) = match session.context.composition.back() {
            Some(segment) => (
                segment
                    .candidates
                    .iter()
                    .map(|candidate| candidate.text.clone())
                    .collect::<Vec<_>>(),
                segment
                    .candidates
                    .iter()
                    .map(|candidate| candidate.comment.clone())
                    .collect::<Vec<_>>(),
                segment.selected_index as i32,
            ),
            None => (Vec::new(), Vec::new(), 0),
        };
        // 参照 `_hide_candidate`：缓冲且实况输入为空时隐藏候选。
        if session.context.get_option("_hide_candidate") {
            texts.clear();
            comments.clear();
        }
        let preedit = cstring_lossy(&preedit);
        let texts: Vec<CString> = texts.iter().map(|text| cstring_lossy(text)).collect();
        let comments: Vec<CString> = comments
            .iter()
            .map(|comment| cstring_lossy(comment))
            .collect();
        let text_pointers: Vec<*const c_char> = texts.iter().map(|text| text.as_ptr()).collect();
        let comment_pointers: Vec<*const c_char> =
            comments.iter().map(|comment| comment.as_ptr()).collect();
        let aux_up = cstring_lossy(&session.char_to_sound_shape.aux_up);
        let aux_down = cstring_lossy(&session.char_to_sound_shape.aux_down);
        // SAFETY: 指针数组与 C 串在本调用期间有效；计数与数组长度一致。
        unsafe {
            update(
                host.user,
                preedit.as_ptr(),
                cursor as i32,
                text_pointers.as_ptr(),
                comment_pointers.as_ptr(),
                text_pointers.len() as i32,
                selected,
                aux_up.as_ptr(),
                aux_down.as_ptr(),
            );
        }
    }
}
