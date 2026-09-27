// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! 引擎：共享数据（词库/解码/选项/学习库）+ 按输入上下文隔离的多会话。
//!
//! 拆成三块：本文件 = 会话管理与按键路径；[`assembly`] = 装配输入与装载（构造 / 重新部署
//! 共用）；[`diagnostics`] = 状态串与诊断；[`config`] = 配置袋与角色解析。

use std::collections::HashMap;
use std::ffi::CString;
use std::path::PathBuf;

use hux_cfg::roles::{
    OptionKeys, ROLE_ALLOW_DUPLICATE_SINGLE, ROLE_FILTER_NON_HAN, ROLE_FULL_CHARSET,
    RUNTIME_OPTION_ROLES,
};
use hux_cfg::{CandidateLayout, OptionsStore, Settings};
use hux_core::key::KeyEvent;
use hux_core::scheme::{KeyOutcome, Scheme, SchemeConfig};
use hux_core::session::{Context, Event, set_property_if_changed};

use crate::abi::{HostCallback, core_modifiers};
use crate::learning_store::LearningStore;
use crate::session::{ReverseLookupState, Session};

mod assembly;
mod config;
mod diagnostics;

pub(crate) use assembly::{Assembled, Assembly, option_keys, wall_clock};
pub(crate) use config::RuntimeOptions;
// 测试在同一条 `crate::engine::*` 路径下设夹具与核对配置袋；生产路径用不到这三个名字。
#[cfg(test)]
pub(crate) use assembly::ModelSource;
#[cfg(test)]
pub(crate) use config::{resolve_option_roles, scheme_config};
pub(crate) use diagnostics::Diagnostics;

/// 事件泵每轮按键的最大轮数（选项事件可能触发确认，进而产生新事件）。
const EVENT_PUMP_ROUNDS: usize = 4;

pub struct Engine {
    pub(crate) host: Option<HostCallback>,
    /// 方案（平台经 `dyn Scheme` 驱动，不直接引用方案模块；共享资源与会话态都在方案内）。
    pub(crate) scheme: Box<dyn Scheme>,
    pub(crate) sessions: HashMap<u64, Session>,
    next_session: u64,
    /// 选项存储（用户目录不可用时为 `None`，此时仅用内建缺省）。
    pub(crate) options: Option<OptionsStore>,
    /// 外部配置（fcitx5 配置界面 / 测试；默认 = 内建缺省）。
    pub(crate) settings: Settings,
    /// 学习库（用户目录不可用时为禁用占位）。
    pub(crate) learning: LearningStore,
    /// 角色 → 选项键（装配处由方案声明解析；缺角色即报错，见 [`Engine::new_with_dirs`]）。
    pub(crate) option_roles: OptionKeys,
    /// 角色序（= [`RUNTIME_OPTION_ROLES`]）的选项键 C 字符串；缺失角色为 `None`
    /// （`hux_engine_option_key` 返回 NULL，宿主跳过该项）。
    pub(crate) option_keys: Vec<Option<CString>>,
    /// 设置派生的配置袋是否需要重下发（`apply_settings` 置位）。
    config_dirty: bool,
    /// 上次下发的运行时开关生效值（`None` = 尚未下发）。
    applied_runtime: Option<RuntimeOptions>,
    /// 本次按键「已提交且未消费」：宿主层应消费该键并以 `forwardKey` 重发，
    /// 保证「提交 → 按键」送达顺序（对齐 fcitx5 核心 `KeyEventOrderFix` 修法）。
    pub(crate) forward_after_commit: bool,
    /// 装配输入（目录 / 模型来源；构造与「重新部署」共用一份）。
    assembly: Assembly,
    /// 状态与诊断（状态串 / 配置诊断 / 选项保存错误 / 学习库 / 热键诊断 / 装载摘要）。
    pub(crate) diagnostics: Diagnostics,
    /// 模型摘要（`hux_engine_model_info` 的指针来源）：**重新部署后替换**，
    /// 此前返回的指针随即失效（同 `status` 的契约）。
    pub(crate) model_info: CString,
    /// 模型文件路径（`hux_engine_model_path` 的指针来源）：与 `model_info` 同一替换时机。
    /// 解析到了就是该文件；没解析到（无模型）是**该放的位置**（文件可以不存在）；
    /// 任何路径都给不出时为 `None`（ABI 返回 NULL）。
    pub(crate) model_path: Option<CString>,
}
impl Engine {
    pub(crate) fn new(host: Option<HostCallback>) -> Self {
        // `HUX_MODEL` 显式覆盖（此时路径固定）；未设置则按数据目录查找
        // （`Auto` ⇒「重新部署」会重新查找，新装入的模型随之生效）。
        let model_source = Assembly::model_source(std::env::var_os("HUX_MODEL").map(PathBuf::from));
        Self::with_assembly(host, Assembly::from_env(model_source))
    }

    /// 按指定目录构造：目录 / 模型 / 选项目录全部显式注入，**不经 XDG 缺省**（来源记为注入，
    /// 故「重新部署」沿用它们）。供测试与平台内装配使用；生产装配走 [`Engine::new`]。
    ///
    /// 模型传 `None` 即「未指定」⇒ 走默认查找（各数据目录里的方案模型资产）；夹具目录
    /// 里都没有模型文件，故与「不装模型」同效，而重新部署时按同一来源重新查找。
    // 非测试构建下平台装配尚未接入（ABI 侧只经 `Engine::new`）；接口本身是正式面，不作死码。
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn new_with_dirs(
        host: Option<HostCallback>,
        dirs: Vec<PathBuf>,
        model_path: Option<PathBuf>,
        options_dir: Option<PathBuf>,
    ) -> Self {
        Self::with_assembly(host, Assembly::injected(dirs, model_path, options_dir))
    }

    /// 构造本体：把一份装配输入装成引擎（「重新部署」按同一路径重装，见 [`Engine::redeploy`]）。
    fn with_assembly(host: Option<HostCallback>, assembly: Assembly) -> Self {
        let settings = Settings::default();
        // 构造方案前先按设置装配配置袋；运行时开关的初始值取设置缺省（尚无会话与存储）。
        let applied_runtime = RuntimeOptions::from_settings(&settings);
        let assembled = assembly.load(&settings, applied_runtime);
        let Assembled {
            scheme,
            option_roles,
            options,
            learning,
            learning_error,
            model_path,
            model_info,
            notes,
        } = assembled;
        let option_keys = option_keys(&option_roles);
        Self {
            host,
            scheme: Box::new(scheme),
            sessions: HashMap::new(),
            next_session: 1,
            options,
            settings,
            learning,
            option_roles,
            option_keys,
            config_dirty: false,
            applied_runtime: Some(applied_runtime),
            forward_after_commit: false,
            assembly,
            diagnostics: Diagnostics::new(notes, learning_error),
            model_info,
            model_path,
        }
    }

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
    fn runtime_option_values(&self) -> RuntimeOptions {
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
    fn push_scheme_config(&mut self) {
        let runtime = self.runtime_option_values();
        if !self.config_dirty && self.applied_runtime == Some(runtime) {
            return;
        }
        self.config_dirty = false;
        self.applied_runtime = Some(runtime);
        let config = self.scheme_config_with_runtime();
        self.apply_scheme_config(config);
    }

    /// 数据装载摘要（`hux_engine_data_info`）：首次调用按方案算一次并缓存
    /// （`&self` 入口，故用 `OnceLock`）；配置下发 / 重新部署时置空失效（旧指针随之失效）。
    pub(crate) fn data_info(&self) -> &CString {
        self.diagnostics.data_info(self.scheme.as_ref())
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
    fn with_session<R>(
        &mut self,
        session_id: u64,
        f: impl FnOnce(&mut Self, &mut Session) -> R,
    ) -> Option<R> {
        let mut session = self.sessions.remove(&session_id)?;
        let result = f(self, &mut session);
        self.sessions.insert(session_id, session);
        Some(result)
    }

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

    /// 当前组合末段是否为字反查段（进入/退出由方案处理器负责：触发字符推入/清空组合）；
    /// 查码段内方向键交应用处理（见 `key_in` 开头的早退）。
    pub(crate) fn reverse_lookup_tagged(&self, session: &Session) -> bool {
        self.scheme.auxiliary_lookup_active(&session.context)
    }

    /// 重算两排提示（上排 = 光标左侧拼音、下排 = 虎码）；不在查码段则清空。
    fn refresh_reverse_lookup_aux(&mut self, session: &mut Session) {
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

    /// 选项事件 → 记录/持久化（参照 `M.options` 的选项通知器）。
    fn observe_option(&mut self, context: &mut Context, name: &str) {
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
    fn observe_learning_error(&mut self) {
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
    fn apply_layout_options(&self, session: &mut Session) {
        let vertical = self.settings.candidate_layout == CandidateLayout::Vertical;
        if session.context.get_option("_vertical") != vertical {
            session.context.set_option("_vertical", vertical);
        }
    }

    /// 重新部署：**重走一遍构造期的读取**并重置全部现有会话状态。
    ///
    /// 重做的读取（与构造同源、同顺序，见 [`Assembly::load`]）：目录（数据目录 / 选项目录）
    /// → 模型路径 → 方案数据（词库 / 词先验 / 标点 / 模型）→ 选项存储（重读 `options.yaml`，
    /// 重放到全部会话）→ 学习库（重开，重读 `e/` 事件）。进程级环境变量（`HUX_DATA_DIRS` /
    /// `HUX_MODEL`）在同一进程内无法改变：目录按同一规则重算（结果与构造时相同），
    /// 模型仍由 [`ModelSource`] 定源（显式路径沿用、默认查找重查）。
    ///
    /// 平台侧会话 id 不变（宿主的输入上下文与 id 的对应关系保持，IC 不需要重建），
    /// 方案侧会话全部重建 ⇒ 组合、候选、学习暂存、反查态一并作废（宿主负责清面板）。
    /// 返回 `true` = 已重新装配。
    pub fn redeploy(&mut self) -> bool {
        // 目录与模型：与构造同一规则（见 [`Assembly::resolve_dirs`] / [`ModelSource`]）。
        self.assembly.resolve_dirs();
        // 配置袋与构造同源：设置派生的角色 + 运行时开关的生效值（单字重码 / 字集开关）。
        let runtime = self.runtime_option_values();
        // 学习库：重开（重读库文件）。必须先释放旧句柄——同一路径二次打开会撞上 LevelDB 的
        // 独占锁（rusty-leveldb 的 `LOCK`）；库名依赖方案 id，故 `load` 已按**新**方案的 id 打开。
        if self.assembly.has_options_dir() {
            self.learning = LearningStore::disabled("reloading");
        }
        let assembled = self.assembly.load(&self.settings, runtime);
        let Assembled {
            scheme,
            option_roles,
            options,
            learning,
            learning_error,
            model_path,
            model_info,
            notes,
        } = assembled;
        // 学习库：换上刚打开的那一份（旧句柄已在 `load` 之前释放，见上）。
        self.learning = learning;
        // 选项存储：重开（重读 `options.yaml`）；缺省仍取当前设置（与构造同一口径）。
        // 会话上下文在下面的逐会话重置里由 `sync` 重放。
        self.options = options;
        // 释放旧方案的会话（平台侧 id 与宿主输入上下文不受影响），再换上重新装配的方案。
        let old_sessions: Vec<_> = self
            .sessions
            .values()
            .map(|session| session.scheme_session)
            .collect();
        for scheme_session in old_sessions {
            self.scheme.free_session(scheme_session);
        }
        self.scheme = Box::new(scheme);
        // 角色表随方案重新解析（角色缺失时宿主菜单跳过该项，诊断进状态串）。
        self.option_roles = option_roles;
        self.option_keys = option_keys(&self.option_roles);
        // 状态串换上新装配说明（逐角色诊断由随后的配置下发重新产出）。
        self.diagnostics.reset(notes, learning_error);
        // 逐会话重置：平台 id 保留，方案侧会话重建（触发键 / 最小保留量随新方案刷新，
        // 选项按重读后的存储重放）。
        let ids: Vec<u64> = self.sessions.keys().copied().collect();
        for id in ids {
            self.with_session(id, |engine, session| engine.redeploy_session(session));
        }
        // 新方案拿到配置袋（与构造同源；学习索引在下一次按键时下发）。
        self.config_dirty = true;
        self.applied_runtime = None;
        self.push_scheme_config();
        // 模型摘要 / 模型路径 / 数据装载摘要：指针在此替换（此前返回的指针随即失效，
        // 见 `hux_abi.h`）。
        self.model_info = model_info;
        self.model_path = model_path;
        self.diagnostics.invalidate_data_info();
        true
    }

    /// 重新部署时重置单个会话：平台 id 不变，方案侧会话重建。
    ///
    /// 顺序与 [`Engine::reset_in`] 一致（先丢弃未派发事件，再清组合与反查态）；
    /// 不推 UI 快照——宿主在重新部署动作里统一清面板（旧候选已随会话作废）。
    fn redeploy_session(&mut self, session: &mut Session) {
        session.context.drain_events();
        session.reverse_lookup = ReverseLookupState::default();
        session.context.clear();
        session.scheme_session = self.scheme.new_session(&mut session.context);
        if let Some(options) = self.options.as_mut() {
            options.sync(&mut session.context);
        }
        self.apply_layout_options(session);
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
