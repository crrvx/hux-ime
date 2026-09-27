// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! 会话表：会话的新建 / 释放 / 借用执行，以及单个会话的重置与重新部署。

use hux_core::session::Context;

use crate::session::{ReverseLookupState, Session};

use super::Engine;

impl Engine {
    /// 新建会话（一个输入上下文注册/激活时创建）；选项/触发键/学习模式按共享状态初始化。
    pub fn session_new(&mut self) -> u64 {
        let mut context = Context::new();
        // 宿主缺省：`_auto_commit`（librime `express_editor` 默认 true）。
        context.set_option("_auto_commit", true);
        // 先写设置缺省，再由 `options.yaml` 持久化值覆盖。
        // 分流：**可持久化项**（存储有声明的缺省）只经 `store.sync` 写入——其写入带抑制名单，
        // 不会被随后的选项事件当成用户改动写进 `options.yaml`；其余（如 `ascii_punct`）直接写。
        // 参照实现同此：缺省经 `M.options.sync` 写入并由 `live.syncing` 抑制。
        for (name, value) in self.settings.session_option_defaults(&self.option_roles) {
            if self
                .options
                .as_ref()
                .is_some_and(|store| store.covers(name))
            {
                continue;
            }
            context.set_option(name, value);
        }
        if let Some(options) = self.options.as_mut() {
            options.sync(&mut context);
        }
        // 运行时开关（状态菜单切换、不落盘的项）一律**以现存会话为模板**，避免新会话回退到设置缺省。
        // 本块与是否存在存储**无关**（上面的 `store.covers` 门只作用于设置项），故不能读作「无存储时才走」。
        // 取 `values().next()`（任一会话）是安全的：运行时开关在同一引擎内由同一份状态菜单维护、各会话恒等，
        // 因此结果与选取哪个会话无关（幂等）。若将来运行时开关允许按会话分叉，这里必须换成显式单一来源。
        if let Some(existing) = self.sessions.values().next() {
            for name in self.runtime_options() {
                let value = existing.context.get_option(name);
                if context.get_option(name) != value {
                    context.set_option(name, value);
                }
            }
        }
        let scheme_session = self.scheme.new_session(&mut context);
        let mut session = Session {
            context,
            scheme_session,
            reverse_lookup: ReverseLookupState::default(),
        };
        self.apply_layout_options(&mut session);
        let id = self.next_session;
        self.next_session += 1;
        self.sessions.insert(id, session);
        id
    }

    /// 释放会话（输入上下文销毁时）。
    pub fn session_free(&mut self, session_id: u64) {
        if let Some(session) = self.sessions.remove(&session_id) {
            self.scheme.free_session(session.scheme_session);
        }
    }

    /// 取会话执行闭包后放回（借用拆分：会话与共享状态互不重叠）。
    pub(super) fn with_session<R>(
        &mut self,
        session_id: u64,
        f: impl FnOnce(&mut Self, &mut Session) -> R,
    ) -> Option<R> {
        let mut session = self.sessions.remove(&session_id)?;
        let result = f(self, &mut session);
        self.sessions.insert(session_id, session);
        Some(result)
    }

    /// 重置会话（`deactivate`/`reset`；组合不跨输入上下文保留）。
    pub fn reset(&mut self, session_id: u64) {
        self.with_session(session_id, |engine, session| engine.reset_in(session));
    }

    fn reset_in(&mut self, session: &mut Session) {
        // 契约：重置即丢弃——先清掉未派发的事件（含可能的提交），避免下次按键补上屏。
        session.context.drain_events();
        session.reverse_lookup = ReverseLookupState::default();
        session.context.clear();
        self.scheme
            .reset_session(session.scheme_session, &mut session.context);
        if let Some(options) = self.options.as_mut() {
            options.sync(&mut session.context);
        }
        self.push_update(session);
    }

    /// 重新部署时重置单个会话：平台 id 不变，方案侧会话重建。
    ///
    /// 顺序与 [`Engine::reset_in`] 一致（先丢弃未派发事件，再清组合与反查态）；
    /// 不推 UI 快照——宿主在重新部署动作里统一清面板（旧候选已随会话作废）。
    pub(super) fn redeploy_session(&mut self, session: &mut Session) {
        session.context.drain_events();
        session.reverse_lookup = ReverseLookupState::default();
        session.context.clear();
        session.scheme_session = self.scheme.new_session(&mut session.context);
        if let Some(options) = self.options.as_mut() {
            options.sync(&mut session.context);
        }
        self.apply_layout_options(session);
    }
}
