// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! 非字符键的 `repr` 分发链与各具名键分支（空闲 / 菜单标点 / 回车 / 退出 / 导航 / 翻页）。

use super::super::*;
use super::{KeyDispatch, learning_commit};
use hux_core::host;

impl KeyDispatch<'_, '_> {
    /// `repr` 分发链：纯字符键 → 空闲 → 菜单标点 → 各具名键；未命中交宿主处理器。
    pub(super) fn dispatch_key(
        &mut self,
        params: EarlyCommitParams,
    ) -> anyhow::Result<ProcessorResult> {
        if let Some(ch) = is_plain_char_key(self.key_event, self.repr) {
            return self.handle_printable(ch, params);
        }
        if !self.context.is_composing() {
            return self.handle_idle();
        }
        if let Some(result) = self.handle_menu_punctuation()? {
            return Ok(result);
        }
        if self.repr == "Return" || self.repr == "KP_Enter" {
            return self.handle_enter();
        }
        if self.repr == "Escape" {
            return self.handle_escape();
        }
        if self.repr == "BackSpace" || self.repr == "Delete" {
            return self.handle_backspace();
        }
        if self.repr == "Left" || self.repr == "Right" || self.repr == "Home" || self.repr == "End"
        {
            return self.handle_navigation();
        }
        if self.repr == "Tab" || self.repr == "ISO_Left_Tab" || self.repr == "Shift+Tab" {
            return self.handle_tab();
        }
        if self.repr == "Up"
            || self.repr == "Down"
            || self.repr == "Page_Up"
            || self.repr == "Page_Down"
        {
            return self.handle_page_keys();
        }
        if self.repr == "space" {
            return self.handle_space();
        }
        Ok(ProcessorResult::Forward)
    }

    /// 未处于组合态：`_dotAfterDigitArmed` 的小数点直接上屏，其余交宿主处理器。
    fn handle_idle(&mut self) -> anyhow::Result<ProcessorResult> {
        if self.dot_armed
            && (self.repr == "period" || self.repr == "KP_Decimal")
            && !self.key_event.shift()
            && modifier_free(self.key_event)
        {
            self.context.direct_commit(".");
            return Ok(ProcessorResult::Consume);
        }
        Ok(ProcessorResult::Forward)
    }

    /// 菜单可见时遇可打印标点：按当前选中项暂存学习、确认组合后把原键交标点处理器。
    /// 命中返回 `Some(Forward)`，否则 `None` 继续后续分发。
    fn handle_menu_punctuation(&mut self) -> anyhow::Result<Option<ProcessorResult>> {
        let codepoint = self.key_event.keycode;
        // 菜单可见（不必处于缓冲态）时遇可打印标点：先按当前选中项暂存学习、确认组合，
        // 再把原键交标点处理器（参照 `abad411`：标点段一旦追加进组合，
        // `learning_selection` 就再也不能解码该输入——例如 `zhhbi,`——或取回句子的选中项）。
        //
        // **本仓有意偏离上游 `abad411`**：
        // 上游对该分支内的**所有**可打印 ASCII 标点一律先确认组合再交标点表，于是宿主
        // `key_binder` 的翻页绑定（缺省 `-`/`=`，以及 schema 绑到翻页的 `[`/`]`）被永久遮蔽
        // （最小复现 `j a equal`：期望翻页，实际提交「一=」）。此处先问**与宿主同一套**翻页判据
        // [`host::paging_action`]：会被判为翻页的键不消费、落回宿主链执行翻页；其余标点维持上游行为。
        if self.context.has_menu()
            && (33..=126).contains(&codepoint)
            && (codepoint as u8 as char).is_ascii_punctuation()
            && modifier_free(self.key_event)
            && host::paging_action(self.context, self.env.host_options, self.key_event).is_none()
        {
            let selection = learning_selection(self.decoder, self.context, self.state)?;
            learning_stage(
                self.live,
                self.state,
                selection.selected.as_ref(),
                &selection.raw,
                None,
                self.env.now,
            );
            confirm_selection(
                Some(&mut learning_commit(self.decoder, self.live, self.env.now)),
                self.context,
                self.state,
            );
            return Ok(Some(ProcessorResult::Forward));
        }
        Ok(None)
    }

    /// 参照 `Return` / `KP_Enter`：缓冲文本 + 实时输入直接上屏。
    fn handle_enter(&mut self) -> anyhow::Result<ProcessorResult> {
        self.live.pending.clear();
        self.live.baseline = None;
        let text = format!(
            "{}{}",
            self.state.buffered_text,
            String::from_utf8_lossy(&live_input(self.context))
        );
        self.context.direct_commit(&text);
        self.context.clear();
        self.state.reset(self.context, false);
        Ok(ProcessorResult::Consume)
    }

    /// 参照 `Escape`：丢弃组合与瞬态状态。
    fn handle_escape(&mut self) -> anyhow::Result<ProcessorResult> {
        self.live.pending.clear();
        self.live.baseline = None;
        self.context.clear();
        self.state.reset(self.context, false);
        Ok(ProcessorResult::Consume)
    }

    /// 参照 `Left` / `Right` / `Home` / `End`：缓冲态下钳制光标，其余交宿主。
    fn handle_navigation(&mut self) -> anyhow::Result<ProcessorResult> {
        self.live.pending.clear();
        self.live.baseline = None;
        // 手动光标导航不保留追加证据与待确认 Tab。
        self.state.tab_pending = false;
        reset_early_evidence(self.state);
        self.state.empty_code_pending = None;
        self.state.save(self.context);
        if !self.state.buffered_text.is_empty()
            && (self.repr == "Home" || (self.repr == "Left" && input_caret(self.context) == 0))
        {
            self.context.set_caret(1);
            return Ok(ProcessorResult::Consume);
        }
        Ok(ProcessorResult::Forward)
    }

    /// 参照 `Tab` / `ISO_Left_Tab` / `Shift+Tab`：循环高亮并挂起早提交证据。
    fn handle_tab(&mut self) -> anyhow::Result<ProcessorResult> {
        if !self.state.tab_pending && self.live.store_ready {
            let selection = learning_selection(self.decoder, self.context, self.state)?;
            self.live.baseline = selection.first;
        }
        reset_early_evidence(self.state);
        self.state.suspended = true;
        self.state.empty_code_pending = None;
        self.state.save(self.context);
        if cycle_candidate_highlight(self.context, if self.repr == "Tab" { 1 } else { -1 }) {
            self.state.tab_pending = true;
            self.state.save(self.context);
            return Ok(ProcessorResult::Consume);
        }
        // 菜单不可用时交给 schema 的 Down/Up 绑定与 navigate。
        Ok(ProcessorResult::Forward)
    }

    /// 参照 `Up` / `Down` / `Page_Up` / `Page_Down`：挂起早提交证据后交宿主。
    fn handle_page_keys(&mut self) -> anyhow::Result<ProcessorResult> {
        reset_early_evidence(self.state);
        self.state.suspended = true;
        self.state.empty_code_pending = None;
        self.state.save(self.context);
        Ok(ProcessorResult::Forward)
    }

    /// 参照 `space`：菜单可见时确认当前选中项并上屏，随后复位。
    fn handle_space(&mut self) -> anyhow::Result<ProcessorResult> {
        if self.context.has_menu() {
            let selection = learning_selection(self.decoder, self.context, self.state)?;
            learning_stage(
                self.live,
                self.state,
                selection.selected.as_ref(),
                &selection.raw,
                None,
                self.env.now,
            );
            confirm_selection(
                Some(&mut learning_commit(self.decoder, self.live, self.env.now)),
                self.context,
                self.state,
            );
        }
        self.live.pending.clear();
        self.live.baseline = None;
        self.state.reset(self.context, false);
        Ok(ProcessorResult::Consume)
    }
}
