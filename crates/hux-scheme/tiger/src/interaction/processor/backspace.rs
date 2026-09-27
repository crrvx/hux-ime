// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! 退格 / 删除分支（`handle_backspace`）：缓冲态弹字符、锁内摘除一字节，其余交宿主。

use super::super::*;
use super::KeyDispatch;

/// `BackSpace` 删光标前一字节、`Delete` 删光标处字节（均相对实时输入）；越界返回 `None`。
fn delete_target(repr: &str, caret: usize, raw_length: usize) -> Option<usize> {
    let first = if repr == "BackSpace" {
        caret as isize - 1
    } else {
        caret as isize
    };
    if first < 0 || first >= raw_length as isize {
        return None;
    }
    Some(first as usize)
}

/// 摘除 `raw[first]` 之后剩余的字节。
fn splice_out(raw: &[u8], first: usize) -> Vec<u8> {
    let mut remaining = raw[..first].to_vec();
    remaining.extend_from_slice(&raw[first + 1..]);
    remaining
}

impl KeyDispatch<'_, '_> {
    /// 参照 `BackSpace` / `Delete`：缓冲删除、锁内编辑，其余交宿主。
    pub(super) fn handle_backspace(&mut self) -> anyhow::Result<ProcessorResult> {
        self.live.pending.clear();
        self.live.baseline = None;
        self.state.tab_pending = false;
        reset_early_evidence(self.state);
        self.state.empty_code_pending = None;
        if !self.state.buffered_text.is_empty() {
            return Ok(self.backspace_buffered());
        }
        if self.state.active_lock().is_some() {
            return Ok(self.backspace_locked());
        }
        self.state.save(self.context);
        Ok(ProcessorResult::Forward)
    }

    /// 缓冲态：实时输入为空时弹掉缓冲末字符，否则在输入内摘除一个字节。
    fn backspace_buffered(&mut self) -> ProcessorResult {
        let raw = live_input(self.context);
        let caret = input_caret(self.context);
        if self.repr == "BackSpace" && raw.is_empty() {
            self.pop_buffered_char(&raw);
            return ProcessorResult::Consume;
        }
        let Some(first) = delete_target(self.repr, caret, raw.len()) else {
            return ProcessorResult::Consume;
        };
        let remaining = splice_out(&raw, first);
        invalidate_edit_state(
            self.context,
            self.state,
            self.state.committed_raw.len() + first,
            self.state.committed_raw.len() + remaining.len(),
        );
        restore_composition_input(self.context, &remaining);
        self.context.set_caret(first + 1);
        ProcessorResult::Consume
    }

    /// 输入为空：弹掉缓冲末字符，同步截断已提交文本并重建单锁。
    fn pop_buffered_char(&mut self, raw: &[u8]) {
        let mut letters: Vec<char> = self.state.buffered_text.chars().collect();
        let removed = letters.pop();
        self.state.buffered_text = letters.into_iter().collect();
        let removed_length = removed.map(char::len_utf8).unwrap_or(0);
        let mut keep = self
            .state
            .committed_text
            .len()
            .saturating_sub(removed_length);
        // 属性可能来自旧版本/外部：仅在字符边界上截断，避免 panic。
        while keep > 0 && !self.state.committed_text.is_char_boundary(keep) {
            keep -= 1;
        }
        self.state.committed_text.truncate(keep);
        if self.state.buffered_text.is_empty() {
            self.state.reset(self.context, false);
        } else {
            self.state.locks = vec![Lock {
                raw: self.state.committed_raw.clone(),
                text: self.state.committed_text.clone(),
                boundaries: format!(
                    "{},{};",
                    self.state.committed_raw.len(),
                    self.state.committed_text.len()
                ),
            }];
            self.state.save(self.context);
        }
        restore_composition_input(self.context, raw);
    }

    /// 锁内编辑：摘除光标处字节；结果为空则清组合，否则按 `BackSpace` / `Delete` 挪光标。
    fn backspace_locked(&mut self) -> ProcessorResult {
        let raw = live_input(self.context);
        let caret = input_caret(self.context);
        let Some(first) = delete_target(self.repr, caret, raw.len()) else {
            self.state.save(self.context);
            return ProcessorResult::Forward;
        };
        let remaining = splice_out(&raw, first);
        invalidate_edit_state(
            self.context,
            self.state,
            self.state.committed_raw.len() + first,
            self.state.committed_raw.len() + remaining.len(),
        );
        if remaining.is_empty() {
            self.context.clear();
            self.state.reset(self.context, false);
        } else if self.repr == "BackSpace" {
            self.context.pop_input(1);
        } else {
            self.context.delete_input(1);
        }
        ProcessorResult::Consume
    }
}
