// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! 字反查：查码段判定、应用侧周边文本与两排提示的会话态。

use crate::session::Session;

use super::Engine;

impl Engine {
    /// 当前组合末段是否为字反查段（进入/退出由方案处理器负责：触发字符推入/清空组合）；
    /// 查码段内方向键交应用处理（见 `key_in` 开头的早退）。
    pub(crate) fn reverse_lookup_tagged(&self, session: &Session) -> bool {
        self.scheme.auxiliary_lookup_active(&session.context)
    }

    /// 重算两排提示（上排 = 光标左侧拼音、下排 = 虎码）；不在查码段则清空。
    pub(super) fn refresh_reverse_lookup_aux(&mut self, session: &mut Session) {
        let tagged = self.reverse_lookup_tagged(session);
        let state = &mut session.reverse_lookup;
        if !tagged || !state.valid {
            // 不在查码段，或周边文本不可用（如终端）：提示能否呈现取决于前端，
            // 统一清空两排。
            state.aux_up.clear();
            state.aux_down.clear();
            return;
        }
        let (up, down) = self.scheme.auxiliary_rows(&state.text, state.cursor);
        state.aux_up = up;
        state.aux_down = down;
    }

    /// 宿主送入应用侧周边文本（字符制光标；`None` = 应用不支持/不可用）。
    pub fn set_surrounding(&mut self, session_id: u64, text: Option<&str>, cursor_chars: usize) {
        self.with_session(session_id, |engine, session| {
            engine.set_surrounding_in(session, text, cursor_chars);
        });
    }

    fn set_surrounding_in(
        &mut self,
        session: &mut Session,
        text: Option<&str>,
        cursor_chars: usize,
    ) {
        let state = &mut session.reverse_lookup;
        match text {
            Some(text) => {
                state.valid = true;
                state.text = text.to_string();
                state.cursor = cursor_chars.min(state.text.chars().count());
            }
            None => {
                state.valid = false;
                state.text.clear();
                state.cursor = 0;
            }
        }
        if self.reverse_lookup_tagged(session) {
            self.refresh_reverse_lookup_aux(session);
        }
    }
}
