// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! 按键与候选点击路径：一次按键 / 一次候选点击交给方案处理，再走提交泵与 UI 刷新。

use hux_core::key::KeyEvent;
use hux_core::scheme::KeyOutcome;
use hux_core::session::Event;

use crate::abi::core_modifiers;
use crate::session::Session;

use super::{EVENT_PUMP_ROUNDS, Engine, wall_clock};

impl Engine {
    /// 处理一次按键：返回是否消费；副作用（提交/preedit/候选）经宿主回调送出。
    ///
    /// 提交且未消费（`express_editor` 的 `char_handler = DirectCommit`）时置
    /// [`Engine::forward_after_commit`]：宿主层据此消费该键并以 `forwardKey` 重发，
    /// 保证客户端先收到提交、后收到按键。
    pub fn key(&mut self, session_id: u64, keysym: u32, states: u32, release: bool) -> bool {
        match self.with_session(session_id, |engine, session| {
            engine.key_in(session, keysym, states, release)
        }) {
            Some(consumed) => consumed,
            None => {
                // 未知 / 已释放会话不得沿用**上一次**按键留下的粘性转发位——否则
                // `hux_engine_key` 会只回 `HUX_KEY_FORWARD_AFTER_COMMIT`（无 CONSUMED），
                // 宿主据此 `filterAndAccept` + `forwardKey` 一个并不存在的提交。
                self.forward_after_commit = false;
                false
            }
        }
    }

    fn key_in(&mut self, session: &mut Session, keysym: u32, states: u32, release: bool) -> bool {
        self.forward_after_commit = false;
        let key = KeyEvent::new(keysym as i32, core_modifiers(states, release));
        // 字反查段：←/→/↑/↓ **交应用处理**（应用光标随动），本层不消费也不改动输入；
        // 两排在应用回传周边文本后的下一次按键（含 release）时刷新。
        if !release
            && self.scheme.auxiliary_lookup_active(&session.context)
            && matches!(key.repr().as_str(), "Left" | "Right" | "Up" | "Down")
        {
            return false;
        }
        let now = wall_clock();
        // 方案内部完成：处理器（翻译/锁/早提交）→ 未消费则宿主链（含提交点学习）。
        let consumed =
            match self
                .scheme
                .process_key(session.scheme_session, &mut session.context, &key, now)
            {
                Ok(KeyOutcome::Consumed) => true,
                Ok(KeyOutcome::Forward) => false,
                Err(error) => {
                    eprintln!("hux: processor error: {error}");
                    false
                }
            };
        self.finish(session, now, Some(consumed));
        consumed
    }

    /// 候选点击（面板候选 `CandidateWord::select`，按会话）：按全局索引选中并上屏。
    /// 走与 `space` 相同的确认/学习链；越界/无可选段返回 `false`。
    pub fn select_candidate(&mut self, session_id: u64, index: usize) -> bool {
        match self.with_session(session_id, |engine, session| {
            engine.select_candidate_in(session, index)
        }) {
            Some(selected) => selected,
            None => {
                // 同 `key`：候选点击也走 `hux_engine_key` 之外的路径，粘性位必须清掉。
                self.forward_after_commit = false;
                false
            }
        }
    }

    fn select_candidate_in(&mut self, session: &mut Session, index: usize) -> bool {
        self.forward_after_commit = false;
        let now = wall_clock();
        match self
            .scheme
            .select_candidate(session.scheme_session, &mut session.context, index, now)
        {
            Ok(true) => {}
            Ok(false) => return false,
            Err(error) => {
                eprintln!("hux: select candidate error: {error}");
                return false;
            }
        }
        self.finish(session, now, None);
        true
    }

    /// 提交泵 + 学习落库 + 组合重建 + UI 刷新（按键与候选点击共用）。
    ///
    /// `key_forward`：按键路径传入消费结果（据提交计算 `forward_after_commit`）；
    /// 候选点击传 `None`（非按键路径，恒不转发）。
    fn finish(&mut self, session: &mut Session, _now: f64, key_forward: Option<bool>) {
        let mut commits = Vec::new();
        let mut invalidated = false;
        // 事件泵：选项事件可能触发确认（进而产生提交），循环至排空。
        // 上限是防御性的（选项事件链不可能无限展开）；超限的残余事件留到下一次按键处理。
        for _ in 0..EVENT_PUMP_ROUNDS {
            let events = session.context.drain_events();
            if events.is_empty() {
                break;
            }
            for event in events {
                match event {
                    Event::Commit(text) => {
                        invalidated = true;
                        commits.push(text);
                    }
                    Event::Option(name) => {
                        self.observe_option(&mut session.context, &name);
                    }
                    Event::Update => {}
                }
            }
        }
        let committed = !commits.is_empty();
        for text in commits {
            self.host_commit(&text);
        }
        self.forward_after_commit = match key_forward {
            Some(consumed) => !consumed && committed,
            None => false,
        };
        // 学习：核心暂存 → 落库；应用索引。
        let submitted = self.scheme.take_learning_events(session.scheme_session);
        if !submitted.is_empty() {
            self.learning.confirm(&submitted);
        }
        // 运行期落库失败（磁盘满 / 库被改成只读 / 锁异常）不进状态串的话，用户只看到
        // 「学习不生效」。
        self.observe_learning_error();
        self.push_scheme_config();
        let version = self.learning.index_version();
        self.scheme
            .apply_learning_index(session.scheme_session, version, self.learning.index());
        // 组合重建（参照 `ConcreteEngine::Compose`，含 update 通知器）
        // （非组合清暂存；缓冲且实况输入为空时隐藏候选）。
        if let Err(error) =
            self.scheme
                .rebuild(session.scheme_session, &mut session.context, invalidated)
        {
            eprintln!("hux: rebuild error: {error}");
        }
        self.refresh_reverse_lookup_aux(session);
        self.push_update(session);
    }

    /// 提交回调（`engine:commit_text`）。
    fn host_commit(&self, text: &str) {
        let Some(host) = &self.host else {
            return;
        };
        let Some(commit) = host.commit else {
            return;
        };
        // SAFETY: 函数指针与 `user` 由宿主提供且在本调用期间有效。
        let text = crate::ui::cstring_lossy(text);
        unsafe { commit(host.user, text.as_ptr()) };
    }
}
