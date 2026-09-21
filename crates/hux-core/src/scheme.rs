// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! 方案契约（P4c）：内核与平台驱动方案时使用的**最小**接口。
//!
//! 设计取舍（`docs/refactor.md` §5）：
//! - 只定义「必须回调方案」的动作（`id` / 数据资产 / 按键 / 组合重建 / 证据与学习策略 / 反查展示），
//!   不把虎码特有语义（缓冲态、锁、早提交启发式）泛化进契约——它们留在方案 profile；
//! - 契约放 `hux-core`：方案无论如何要依赖 core 的类型（[`KeyEvent`] / [`Context`] / `Candidate`…），
//!   单开 interface crate 只多一跳、无净收益；
//! - 平台是装配根：构造具体方案（如 `hux-scheme-tiger`）后以 `dyn Scheme` 驱动，不直接引用方案模块。
//!
//! 方向约束：`hux-scheme/* → hux-core`；内核不 import 任何方案（CI 校验）。

use crate::host::HostOptions;
use crate::key::KeyEvent;
use crate::learning::{Event, LearningIndex};
use crate::session::Context;
use std::path::PathBuf;

/// 只读数据资产的种类（平台据此定位与记日志；文件名由方案给出）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AssetKind {
    /// n-gram 模型（可缺失）。
    Model,
    /// 运行数据（码表 / 词先验 / 反查索引 / 标点表；可缺失，方案自行降级）。
    Data,
}

/// 数据资产：相对数据目录的文件名（`/` 分隔，不含目录前缀）。
///
/// 资产清单由**方案的装配面常量**声明（如 `hux_scheme_tiger::scheme::ASSETS`）：
/// 模型路径必须在构造方案**之前**解析（[`Scheme`] 需要已加载的模型），
/// 故清单不属于实例方法。
#[derive(Clone, Copy, Debug)]
pub struct Asset {
    pub kind: AssetKind,
    pub file: &'static str,
}

/// 在数据目录中按优先级展开资产候选路径（纯函数：不读环境、不判存在）。
pub fn asset_paths(dirs: &[PathBuf], file: &str) -> Vec<PathBuf> {
    dirs.iter().map(|dir| dir.join(file)).collect()
}

/// 在数据目录中查找第一个存在的资产路径。
pub fn find_asset(dirs: &[PathBuf], file: &str) -> Option<PathBuf> {
    asset_paths(dirs, file)
        .into_iter()
        .find(|path| path.is_file())
}

/// 按键结果：与宿主链的 [`crate::host::HostResult`] **同一个类型**（避免同构重复与手工转换）。
///
/// 语义：`Consumed` = 方案/宿主链消费该键；`Forward` = 交平台决定后续（如转发给应用）。
pub use crate::host::HostResult as KeyOutcome;

/// 方案选项 id 清单：**方案声明自己的选项键**（运行时选项与持久化键的单一来源）。
///
/// 配置层（`hux-cfg`）不再硬编码方案选项名——平台从方案取得本结构后传入；
/// 平台的状态菜单白名单亦据此构造。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OptionIds {
    /// 提前上屏总开关。
    pub early_commit: &'static str,
    /// 提前上屏至预编辑。
    pub early_commit_to_preedit: &'static str,
    /// 单字重码参与组句。
    pub allow_duplicate_single: &'static str,
    /// 数字直选。
    pub digit_select: &'static str,
}

/// 每输入上下文一份的会话句柄（由方案分配与解释；平台只透传）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SessionId(pub u64);

/// 方案配置：平台由 hux 自身设置（fcitx5 配置页）映射而来。
///
/// **契约不解释字段含义**，由方案决定如何生效：虎句把触发键写进上下文属性、
/// 最短保留码数放进会话、页大小与翻页键放进宿主链选项。
#[derive(Clone, Debug, Default)]
pub struct SchemeConfig {
    /// 高频字过滤上限（词库创建时生效；`0` = 不限）。
    pub high_freq_limit: usize,
    /// 提前上屏最短保留码数（`0` = 不额外限制）。
    pub min_retained_raw_length: usize,
    /// 每页候选个数。
    pub page_size: usize,
    /// 翻页循环。
    pub page_cycle: bool,
    /// 上 / 下翻页键（rime 键名）。
    pub page_up_keys: Vec<String>,
    pub page_down_keys: Vec<String>,
    /// 音反查 / 字反查触发键（rime 键名）。
    pub sound_to_char_shape_keys: Vec<String>,
    pub char_to_sound_shape_keys: Vec<String>,
    /// Tab 学习开关（影响学习 mode 编码）。
    pub tab_learning: bool,
}

/// 方案：引擎级共享资源（码表 / 模型 / 解码器）+ 会话集合。
///
/// 平台持有 `Box<dyn Scheme>` 并只保存 [`SessionId`]；会话状态由方案内部持有，
/// 因此共享资源与会话状态之间的借用拆分由方案负责。
pub trait Scheme {
    /// 方案标识（数据 / 学习库命名用；虎句为 `tiger_sentence`）。
    fn id(&self) -> &'static str;
    /// 学习规则串（来自方案数据；平台用于拼学习 mode）。
    fn learning_rules(&self) -> &str;
    /// 方案声明的选项 id（配置层的持久化键与平台的状态菜单白名单均据此，
    /// 不得在别处硬编码方案选项名）。
    fn option_ids(&self) -> OptionIds;
    /// 学习 mode 编码（方案定义；空串 = 不学习）。
    fn learning_mode(&self, rules: &str, duplicate: bool, high_freq_limit: usize) -> String;

    /// 应用方案配置（已存在会话同步生效；上下文属性在下次按键 / 重建时惰性同步）。
    fn apply_config(&mut self, config: &SchemeConfig);
    /// 宿主链选项（方案据配置派生；平台不解释其含义）。
    fn host_options(&self) -> &HostOptions;

    /// 设置学习 mode（写入全部会话的暂存态；变化时重置方案内证据缓存）。
    fn set_learning_mode(&mut self, mode: &str);
    /// 学习库就绪状态（未就绪时方案不产出学习事件）。
    fn set_store_ready(&mut self, ready: bool);
    /// 应用学习库索引；`version` 变化时重置该会话的证据与空码态。
    fn apply_learning_index(
        &mut self,
        session: SessionId,
        version: u64,
        index: &LearningIndex,
        mode: &str,
    );

    /// 新建会话（平台先建上下文，方案写入自己的属性）。
    fn new_session(&mut self, context: &mut Context) -> SessionId;
    /// 释放会话。
    fn free_session(&mut self, session: SessionId);
    /// 重置会话（`deactivate` / `reset`：组合不跨输入上下文保留）。
    fn reset_session(&mut self, session: SessionId, context: &mut Context);

    /// 处理一次按键（方案内部完成翻译、早提交与宿主链提交点）；时间由平台注入。
    fn process_key(
        &mut self,
        session: SessionId,
        context: &mut Context,
        key: &KeyEvent,
        now: f64,
    ) -> Result<KeyOutcome, String>;
    /// 候选点击上屏（与空格同一条确认 / 学习链）；越界 / 无可选段返回 `Ok(false)`。
    fn select_candidate(
        &mut self,
        session: SessionId,
        context: &mut Context,
        index: usize,
        now: f64,
    ) -> Result<bool, String>;
    /// 组合重建（提交或输入变化时；保留段状态含菜单高亮）。
    fn rebuild(
        &mut self,
        session: SessionId,
        context: &mut Context,
        invalidated: bool,
    ) -> Result<(), String>;
    /// 排空待落库的学习事件（平台持久化）。
    fn take_learning_events(&mut self, session: SessionId) -> Vec<Event>;

    /// 预编辑用缓冲文本（方案属性；空串 = 非缓冲态）。
    fn buffered_text(&self, context: &Context) -> String;
    /// 该会话是否处于辅助查询态（如字反查：方向键交应用处理）。
    fn auxiliary_lookup_active(&self, context: &Context) -> bool;
    /// 依据周边文本算两排提示（上排 / 下排）；无数据时为空串（可能更新方案内缓存）。
    fn auxiliary_rows(&mut self, text: &str, cursor_chars: usize) -> (String, String);
}
