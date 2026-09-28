// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! 设置与运行时开关：设置 / 存储 / 会话三级取值，配置袋下发，选项读写与布局选项。

use hux_cfg::roles::{
    ROLE_ALLOW_DUPLICATE_SINGLE, ROLE_FILTER_NON_HAN, ROLE_FULL_CHARSET, RUNTIME_OPTION_ROLES,
};
use hux_cfg::{CandidateLayout, Settings};
use hux_core::scheme::SchemeConfig;
use hux_core::session::{Context, set_property_if_changed};

use crate::session::Session;

use super::{Engine, RuntimeOptions, assembly, config};

impl Engine {
    /// 状态菜单可切换的运行时开关（顺序即菜单顺序 = C ABI 的 `HUX_OPTION_*` 角色序）：
    /// 方案声明的角色 + 宿主标准的 `full_shape`。方案未声明的角色**不出现在菜单里**。
    pub(crate) fn runtime_options(&self) -> Vec<&'static str> {
        RUNTIME_OPTION_ROLES
            .iter()
            .filter_map(|role| self.option_roles.key(role))
            .collect()
    }

    /// 运行时空开关白名单查询（按键路径每键都会问一次，故不做分配）。
    fn is_runtime_option(&self, name: &str) -> bool {
        RUNTIME_OPTION_ROLES
            .iter()
            .any(|role| self.option_roles.key(role) == Some(name))
    }

    /// 某个运行时开关的**生效值**（会话 → 存储 → 设置缺省；角色无键时按 `fallback` 计）。
    fn effective_option(&self, role: &str, fallback: bool) -> bool {
        self.option_roles
            .key(role)
            .and_then(|name| self.option_value(name))
            .unwrap_or(fallback)
    }

    /// 运行时开关的生效值集合（构造 / 重新部署 / 每次按键共用同一口径）。
    pub(super) fn runtime_option_values(&self) -> RuntimeOptions {
        RuntimeOptions {
            duplicate: self.effective_option(ROLE_ALLOW_DUPLICATE_SINGLE, true),
            full_charset: self.effective_option(ROLE_FULL_CHARSET, true),
            filter_non_han: self.effective_option(ROLE_FILTER_NON_HAN, true),
        }
    }

    /// 设置派生的角色 + 运行时开关的生效值（与 [`assembly::scheme_config_with_runtime`] 同口径）。
    ///
    /// `pub(crate)`：`tests.rs` 用它核对「按键路径下发的配置袋」与构造期一致。
    pub(crate) fn scheme_config_with_runtime(&self) -> SchemeConfig {
        assembly::scheme_config_with_runtime(&self.settings, self.runtime_option_values())
    }

    /// 下发配置袋（设置派生的角色 + 运行时开关的生效值），方案据此自算学习 mode、重建词库。
    ///
    /// 按键路径每次都会问一次：设置未变且运行时开关值不变时直接返回，不重建配置袋。
    pub(super) fn push_scheme_config(&mut self) {
        let runtime = self.runtime_option_values();
        if !self.config_dirty && self.applied_runtime == Some(runtime) {
            return;
        }
        self.config_dirty = false;
        self.applied_runtime = Some(runtime);
        let config = self.scheme_config_with_runtime();
        self.apply_scheme_config(config);
    }

    /// 下发一个配置袋并收录诊断。
    ///
    /// 方案的 `apply_config` 返回逐角色诊断（角色缺失 / 类型不符）：方案已按缺省值回退，
    /// 平台把诊断并入状态串（与装配期 `config:` 诊断同风格），避免运行期静默降级。
    /// 方案可能已按新的高频上限 / 字集开关重建词库 ⇒ 装载摘要同时失效。
    pub(crate) fn apply_scheme_config(&mut self, config: SchemeConfig) {
        let result = self.scheme.apply_config(&config);
        let notes: Vec<String> = match result {
            Ok(()) => Vec::new(),
            Err(errors) => errors
                .iter()
                .map(|error| format!("config: {error}"))
                .collect(),
        };
        self.diagnostics.invalidate_data_info();
        self.diagnostics.observe_config_notes(notes);
    }

    /// 选项事件 → 记录/持久化（参照 `M.options` 的选项通知器）。
    pub(super) fn observe_option(&mut self, context: &mut Context, name: &str) {
        if let Some(options) = self.options.as_mut() {
            options.observe(context, name);
        }
        // 保存失败会写入 [`hux_cfg::OPTIONS_ERROR_PROPERTY`]：并入状态串，
        // 使 `hux_engine_status` 能反映出来（此前该属性全仓无读取方）。
        let error = context
            .get_property(hux_cfg::OPTIONS_ERROR_PROPERTY)
            .filter(|value| !value.is_empty())
            .map(str::to_string);
        self.set_option_error(error);
    }

    /// 选项保存失败诊断 → 状态串（`None` = 清除）。
    ///
    /// 属性是唯一来源（[`Engine::observe_option`] 从上下文属性读，[`Engine::apply_settings`]
    /// 的批量写回直接把结果交到这里），故两处共用本入口。
    fn set_option_error(&mut self, error: Option<String>) {
        self.diagnostics.observe_option_error(error);
    }

    /// 学习库诊断变化 → 并入状态串。
    ///
    /// 此前 `learning.error` 只在 `new_with_dirs` 里读一次：打开失败可见，而**运行期写盘失败**
    /// （LevelDB `put` 返回错误）既无日志也不进 `hux_engine_status`。这里在落库路径上调一次，
    /// 诊断变化即刷新状态串（与 `*_options_error` 同风格）。
    pub(super) fn observe_learning_error(&mut self) {
        self.diagnostics
            .observe_learning_error(self.learning.error.clone());
    }

    /// 运行时开关当前值（状态菜单；全局）：任一会话的生效值，无会话时回退存储/设置缺省。
    pub fn option_value(&self, name: &str) -> Option<bool> {
        if !self.is_runtime_option(name) {
            return None;
        }
        if let Some(session) = self.sessions.values().next() {
            return Some(session.context.get_option(name));
        }
        self.options
            .as_ref()
            .and_then(|store| store.value(name))
            .or_else(|| {
                self.settings
                    .session_option_default(&self.option_roles, name)
            })
    }

    /// 设置运行时开关（状态菜单）：白名单校验 → 写入全部会话 → 持久化（`options.yaml`）。
    pub fn set_option_value(&mut self, name: &str, value: bool) -> bool {
        if !self.is_runtime_option(name) {
            return false;
        }
        let ids: Vec<u64> = self.sessions.keys().copied().collect();
        if ids.is_empty() {
            // 无会话（尚未绑定输入上下文）：直写存储，避免状态菜单切换被静默丢弃。
            let saved = self
                .options
                .as_mut()
                .map(|store| store.set_value(name, value));
            self.push_scheme_config();
            return saved.unwrap_or(true);
        }
        for id in ids {
            self.with_session(id, |engine, session| {
                if session.context.get_option(name) != value {
                    session.context.set_option(name, value);
                }
                engine.observe_option(&mut session.context, name);
            });
        }
        self.push_scheme_config();
        true
    }

    /// 应用外部配置（fcitx5 配置界面 / 测试）：全部项即时生效（含高频字上限——方案据此重建词库）。
    ///
    /// 配置页与状态菜单**共用同一批开关**（[`Settings::store_defaults`] 的角色），语义为单一事实来源：
    /// - 本入口先把设置值写成持久化值并落盘——否则 `options.yaml` 里的旧值会在随后的 `sync` 中
    ///   压制设置值（「配置页改了不生效」的根因）；
    /// - 状态菜单读同一份值（[`Engine::option_value`]）并写同一份存储，故两侧改动都立即生效、
    ///   另一侧立刻反映。
    ///
    /// 作用于全部会话（选项为引擎级）。
    pub fn apply_settings(&mut self, settings: Settings) {
        self.settings = settings;
        self.config_dirty = true;
        // 配置页热键绑定里无法解析的项：点名（此前在 `filter_map` 处静默消失）。
        let hotkey_notes = config::unparsable_key_bindings(&self.settings);
        self.diagnostics.observe_hotkey_notes(hotkey_notes);
        let option_defaults = self.settings.session_option_defaults(&self.option_roles);
        let store_defaults = self.settings.store_defaults(&self.option_roles);
        // 先登记缺省（决定哪些角色由存储管理），再把设置值写成持久化值（一次落盘）。
        let saved = match self.options.as_mut() {
            Some(store) => {
                store.set_defaults(store_defaults.clone());
                store.set_values(&store_defaults)
            }
            None => true,
        };
        let save_error = if saved {
            ""
        } else {
            hux_cfg::OPTIONS_ERROR_MESSAGE
        };
        let ids: Vec<u64> = self.sessions.keys().copied().collect();
        for id in ids {
            self.with_session(id, |engine, session| {
                // 保存失败与 `observe_option` 走同一通道（上下文属性 + 状态串）。
                set_property_if_changed(
                    &mut session.context,
                    hux_cfg::OPTIONS_ERROR_PROPERTY,
                    save_error,
                );
                // 同 `session_new`：可持久化项交由 `store.sync`（带抑制），其余直接写。
                for (name, value) in &option_defaults {
                    if engine
                        .options
                        .as_ref()
                        .is_some_and(|store| store.covers(name))
                    {
                        continue;
                    }
                    if session.context.get_option(name) != *value {
                        session.context.set_option(name, *value);
                    }
                }
                if let Some(store) = engine.options.as_mut() {
                    store.sync(&mut session.context);
                }
                engine.apply_layout_options(session);
            });
        }
        self.set_option_error((!saved).then(|| hux_cfg::OPTIONS_ERROR_MESSAGE.to_string()));
        // 设置 + 运行时选项一起下发（含学习 mode 自算），见 [`Engine::push_scheme_config`]。
        self.push_scheme_config();
    }

    /// 候选布局（context 选项）：host `selector` 读取 `_vertical` 决定 ←→/↑↓ 语义。
    /// 仅「竖排」置位；「横排/跟随全局」维持横排键语义（面板排列见 C++ `LayoutHint`）。
    pub(super) fn apply_layout_options(&self, session: &mut Session) {
        let vertical = self.settings.candidate_layout == CandidateLayout::Vertical;
        if session.context.get_option("_vertical") != vertical {
            session.context.set_option("_vertical", vertical);
        }
    }
}
