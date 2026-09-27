// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! 可打印字符分支（`handle_printable`）：反查段选重、数字直选、光标编辑与录入收尾。

use super::super::*;
use super::{KeyDispatch, learning_commit};

impl KeyDispatch<'_, '_> {
    /// 纯字符键（`is_plain_char_key` 命中）：反查段选重 / 数字直选 / 空闲数字上屏 /
    /// Tab 确认 / 照常录入与早提交。
    pub(super) fn handle_printable(
        &mut self,
        ch: char,
        params: EarlyCommitParams,
    ) -> anyhow::Result<ProcessorResult> {
        if let Some(result) = self.printable_idle_reset(ch) {
            return Ok(result);
        }
        if let Some(result) = self.printable_lookup_selection(ch) {
            return Ok(result);
        }
        if let Some(result) = self.printable_digit(ch)? {
            return Ok(result);
        }
        let is_letter = ch.is_ascii_lowercase();
        let live_before = live_input(self.context);
        let caret = input_caret(self.context);
        let mut full_before = self.state.committed_raw.as_bytes().to_vec();
        full_before.extend_from_slice(&live_before);
        if caret != live_before.len() {
            return Ok(self.insert_at_caret(ch, caret, full_before.len()));
        }
        if self.state.tab_pending && is_letter {
            if let Some(result) = self.confirm_pending_tab(ch, &full_before)? {
                return Ok(result);
            }
        }
        self.finish_printable(ch, &full_before, is_letter, params)
    }

    /// 字反查段清理、空闲复位、分号引号让位与输入长度上限；命中即已处理完该键。
    fn printable_idle_reset(&mut self, ch: char) -> Option<ProcessorResult> {
        // 字反查段：其它普通键先清空组合，随后照常处理该键。
        if char_to_sound_shape::tagged(self.context) {
            self.context.clear();
        }
        if !self.context.is_composing()
            && (!self.state.committed_raw.is_empty()
                || !self.state.last_seen_raw.is_empty()
                || !self.state.trackers.is_empty()
                || self.state.suspended
                || self.state.continuation_after_auto_commit
                || self.state.active_lock().is_some()
                || self.state.tab_pending)
        {
            self.state.reset(self.context, false);
        }
        // 分号/引号只在组合中作 rank 选择器；空闲时交标点处理器。
        if !self.context.is_composing() && (ch == ';' || ch == '\'') {
            return Some(ProcessorResult::Forward);
        }
        if live_input(self.context).len() >= MAX_RAW_LENGTH {
            return Some(ProcessorResult::Consume);
        }
        None
    }

    /// 音反查段内的数字 / 分号选择键；其余键返回 `None` 落到后续分支。
    fn printable_lookup_selection(&mut self, ch: char) -> Option<ProcessorResult> {
        let is_letter = ch.is_ascii_lowercase();
        // 音反查段（`` ` `` 前缀）不得把选择键并入拼音：拼写表会把它追加进输入并打断
        // 反查段。数字在此按**上游的绝对索引**（`index = digit - 1`，越界惰性消费）
        // 高亮 + 确认后提交，分号惰性；撇号由识别模式放行（是否参与音节切分见
        // `sound_to_char_shape::matches_pattern` 的说明）。
        //
        // 该判据必须**先于**下方的 addon 数字直选：后者是**页相对**落点
        // （`page_start + position`），二者仅在「反查段菜单停在第 1 页且
        // `page_size >= digit`」时巧合一致；菜单翻到第 2 页起（或 `page_size >= 10`）
        // 时上游按绝对索引选、addon 按本页位置选，结果不同。
        // 参照 `lua/tiger_sentence.lua` @ `92a0b54`（`local index = tonumber(ch) - 1`）；
        // 上游该分支位于 `max_raw_length` 早退与「空闲数字直接上屏」之后，
        // 本仓同序（空闲数字要求 `!is_composing`，与反查段互斥）。
        if !is_letter
            && self.context.composition.back().is_some_and(|segment| {
                segment.has_tag(sound_to_char_shape::SOUND_TO_CHAR_SHAPE_TAG)
            })
        {
            if ch.is_ascii_digit() {
                self.live.pending.clear();
                self.live.baseline = None;
                let index = (ch as u8 - b'0') as isize - 1;
                let count = self
                    .context
                    .composition
                    .back()
                    .map(|segment| segment.candidates.len())
                    .unwrap_or(0);
                if index >= 0 && (index as usize) < count {
                    // 高亮 + 确认与 Space 同路（`Context::select` 可能提交整句）。
                    self.context.highlight(index as usize);
                    confirm_selection(
                        Some(&mut learning_commit(self.decoder, self.live, self.env.now)),
                        self.context,
                        self.state,
                    );
                }
                self.state.reset(self.context, false);
                return Some(ProcessorResult::Consume);
            }
            if ch == ';' {
                return Some(ProcessorResult::Consume);
            }
        }
        None
    }

    /// 数字键两段：addon 数字直选（页相对）与空闲数字直接上屏（全角选项下为全角）。
    fn printable_digit(&mut self, ch: char) -> anyhow::Result<Option<ProcessorResult>> {
        // 数字直选（`OPTION_DIGIT_SELECT`；addon 扩展）：菜单可见时直接上屏当前页候选。
        if self.context.get_option(OPTION_DIGIT_SELECT)
            && ch.is_ascii_digit()
            && self.context.has_menu()
            && let Some(position) = digit_page_position(ch)
            && select_page_candidate(
                self.decoder,
                self.context,
                self.state,
                self.live,
                self.env.now,
                self.env.page_size,
                position,
            )?
        {
            return Ok(Some(ProcessorResult::Consume));
        }
        // 空闲数字直接上屏（全角选项下为全角）。
        if ch.is_ascii_digit() && !self.context.is_composing() {
            if self.context.get_option("full_shape") {
                const FULL_SHAPE_DIGITS: [char; 10] =
                    ['０', '１', '２', '３', '４', '５', '６', '７', '８', '９'];
                let index = (ch as u8 - b'0') as usize;
                self.context
                    .direct_commit(&FULL_SHAPE_DIGITS[index].to_string());
            } else {
                self.context.direct_commit(&ch.to_string());
            }
            *self.env.dot_armed = true;
            return Ok(Some(ProcessorResult::Consume));
        }
        Ok(None)
    }

    /// 光标不在实时输入尾部：按手动编辑路径插入字符并作废受影响锁。
    fn insert_at_caret(&mut self, ch: char, caret: usize, full_length: usize) -> ProcessorResult {
        self.live.pending.clear();
        self.live.baseline = None;
        invalidate_edit_state(
            self.context,
            self.state,
            self.state.committed_raw.len() + caret,
            full_length + ch.len_utf8(),
        );
        self.context.push_input(ch.to_string().as_bytes());
        ProcessorResult::Consume
    }

    /// 常规录入收尾：清 Tab 标志、追加字符，随后按选项尝试空码提交 / 早提交。
    fn finish_printable(
        &mut self,
        ch: char,
        full_before: &[u8],
        is_letter: bool,
        params: EarlyCommitParams,
    ) -> anyhow::Result<ProcessorResult> {
        self.state.tab_pending = false;
        self.live.baseline = None;
        if !is_letter {
            self.state.empty_code_pending = None;
            self.state.save(self.context);
        }
        self.context.push_input(ch.to_string().as_bytes());
        if is_letter
            && try_empty_code_commit(
                &mut learning_commit(self.decoder, self.live, self.env.now),
                self.context,
                self.state,
                full_before,
                ch.to_string().as_bytes(),
                params,
                self.env.dot_armed,
            )?
        {
            return Ok(ProcessorResult::Consume);
        }
        try_early_commit(
            &mut learning_commit(self.decoder, self.live, self.env.now),
            self.context,
            self.state,
            params,
            self.env.dot_armed,
        )?;
        Ok(ProcessorResult::Consume)
    }
}
