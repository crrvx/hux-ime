// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! 引擎：共享数据（词库/解码/选项/学习库）+ 按输入上下文隔离的多会话。
//!
//! 拆成几块：本文件 = `Engine` 的字段、构造装配与重新部署；[`assembly`] = 装配输入与装载
//! （构造 / 重新部署共用）；[`diagnostics`] = 状态串与诊断；[`config`] = 配置袋与角色解析；
//! [`keys`] = 按键与候选点击路径；[`options`] = 设置与运行时开关；[`lifecycle`] = 会话表；
//! [`reverse_lookup`] = 字反查态与周边文本。

use std::collections::HashMap;
use std::ffi::CString;
use std::path::PathBuf;

use hux_cfg::roles::OptionKeys;
use hux_cfg::{OptionsStore, Settings};
use hux_core::scheme::Scheme;

use crate::abi::HostCallback;
use crate::learning_store::LearningStore;
use crate::session::Session;

mod assembly;
mod config;
mod diagnostics;
mod keys;
mod lifecycle;
mod options;
mod reverse_lookup;

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

    /// 数据装载摘要（`hux_engine_data_info`）：首次调用按方案算一次并缓存
    /// （`&self` 入口，故用 `OnceLock`）；配置下发 / 重新部署时置空失效（旧指针随之失效）。
    pub(crate) fn data_info(&self) -> &CString {
        self.diagnostics.data_info(self.scheme.as_ref())
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
}
