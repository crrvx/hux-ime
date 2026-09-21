// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! 方案契约（P4c）：内核与平台驱动方案时使用的**最小**接口。
//!
//! 设计取舍（`docs/refactor.md` §5）：
//! - 只定义「必须回调方案」的动作（`id` / 选项声明 / 数据资产 / 按键 / 组合重建 /
//!   证据与学习策略 / 反查展示），不把任何**方案特有语义**泛化进契约——它们留在方案 profile；
//! - 契约里**没有方案口径的字段名**：选项与配置一律是「角色 → 键 / 值」的数据声明，
//!   角色词汇归配置层（`hux-cfg`）、键名归方案，换方案不改内核；
//! - 契约放 `hux-core`：方案无论如何要依赖 core 的类型（[`KeyEvent`] / [`Context`] / `Candidate`…），
//!   单开 interface crate 只多一跳、无净收益；
//! - 平台是装配根：构造具体方案后以 `dyn Scheme` 驱动，不直接引用方案模块。
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

/// 方案的**选项声明**（`role → key`）：角色是配置层的词汇，键是方案自己的选项名。
///
/// 内核不解释任何角色；配置层（`hux-cfg`）据此把「设置项」接到方案的选项键上，
/// 平台据此构造状态菜单与持久化缺省——换方案只需换方案的声明。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OptionDecl {
    /// 角色名（配置层定义的词汇，如运行时开关的角色）。
    pub role: &'static str,
    /// 该角色对应的选项键（方案的持久化键；`options.yaml` 与上下文选项同名）。
    pub key: &'static str,
}

/// 方案配置袋的取值：小枚举，覆盖「开关 / 计数 / 文本 / 文本列表」四类。
///
/// **契约不解释取值含义**，只保证类型可携带；含义由方案的配置解析决定。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Value {
    Bool(bool),
    Count(usize),
    Text(String),
    Texts(Vec<String>),
}

/// 方案配置袋：**角色 → 值**（平台按角色装配，方案按角色解释）。
///
/// 与 [`OptionDecl`] 同为「数据化声明」：内核不认识任何角色，
/// 缺角色的语义（回退值 / 报错）由方案的解析决定。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SchemeConfig {
    entries: Vec<(&'static str, Value)>,
}

impl SchemeConfig {
    pub fn new() -> Self {
        Self::default()
    }

    /// 写入一个角色（同角色后写覆盖先写）。
    pub fn set(&mut self, role: &'static str, value: Value) {
        match self.entries.iter_mut().find(|(name, _)| *name == role) {
            Some(entry) => entry.1 = value,
            None => self.entries.push((role, value)),
        }
    }

    /// 链式写入（装配处一行一个角色）。
    #[must_use]
    pub fn with(mut self, role: &'static str, value: Value) -> Self {
        self.set(role, value);
        self
    }

    /// 角色对应的值（未装配为 `None`）。
    pub fn get(&self, role: &str) -> Option<&Value> {
        self.entries
            .iter()
            .find(|(name, _)| *name == role)
            .map(|(_, value)| value)
    }

    /// 角色对应的开关值（类型不符或未装配为 `None`）。
    pub fn bool(&self, role: &str) -> Option<bool> {
        match self.get(role) {
            Some(Value::Bool(value)) => Some(*value),
            _ => None,
        }
    }

    /// 角色对应的计数值（类型不符或未装配为 `None`）。
    pub fn count(&self, role: &str) -> Option<usize> {
        match self.get(role) {
            Some(Value::Count(value)) => Some(*value),
            _ => None,
        }
    }

    /// 角色对应的文本（类型不符或未装配为 `None`）。
    pub fn text(&self, role: &str) -> Option<&str> {
        match self.get(role) {
            Some(Value::Text(value)) => Some(value),
            _ => None,
        }
    }

    /// 角色对应的文本列表（类型不符或未装配为空切片）。
    pub fn texts(&self, role: &str) -> &[String] {
        match self.get(role) {
            Some(Value::Texts(value)) => value,
            _ => &[],
        }
    }

    /// 已装配的角色（顺序即装配顺序；守卫与诊断用）。
    pub fn roles(&self) -> impl Iterator<Item = &'static str> + '_ {
        self.entries.iter().map(|(role, _)| *role)
    }
}

/// 每输入上下文一份的会话句柄（由方案分配与解释；平台只透传）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SessionId(pub u64);

/// 方案：引擎级共享资源（码表 / 模型 / 解码器）+ 会话集合。
///
/// 平台持有 `Box<dyn Scheme>` 并只保存 [`SessionId`]；会话状态由方案内部持有，
/// 因此共享资源与会话状态之间的借用拆分由方案负责。
pub trait Scheme {
    /// 方案标识（数据 / 学习库命名用）。
    fn id(&self) -> &'static str;
    /// 方案声明的选项角色 → 键（配置层的持久化键与平台的状态菜单白名单均据此，
    /// 不得在别处硬编码方案选项名）。
    fn option_declarations(&self) -> &'static [OptionDecl];
    /// 方案当前的学习 mode 串（**不透明**，由方案据自身配置与选项自算；空串 = 不学习）。
    fn learning_mode(&self) -> &str;

    /// 应用方案配置（已存在会话同步生效；上下文属性在下次按键 / 重建时惰性同步）。
    fn apply_config(&mut self, config: &SchemeConfig);
    /// 宿主链选项（方案据配置派生；平台不解释其含义）。
    fn host_options(&self) -> &HostOptions;

    /// 学习库就绪状态（未就绪时方案不产出学习事件）。
    fn set_store_ready(&mut self, ready: bool);
    /// 应用学习库索引；`version` 变化时重置该会话的证据与空码态。
    fn apply_learning_index(&mut self, session: SessionId, version: u64, index: &LearningIndex);

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scheme_config_is_a_role_keyed_bag() {
        let config = SchemeConfig::new()
            .with("switch", Value::Bool(true))
            .with("count", Value::Count(5));
        assert_eq!(config.roles().collect::<Vec<_>>(), vec!["switch", "count"]);
        assert_eq!(config.bool("switch"), Some(true));
        // 同角色后写覆盖先写（装配处不改结构即可改值），不新增条目。
        let replaced = config.clone().with("switch", Value::Bool(false));
        assert_eq!(replaced.bool("switch"), Some(false));
        assert_eq!(replaced.roles().count(), 2);
        let mut mutated = config.clone();
        mutated.set("count", Value::Count(9));
        assert_eq!(mutated.count("count"), Some(9));
        assert_eq!(mutated.roles().count(), 2);
    }

    #[test]
    fn scheme_config_accessors_are_type_checked() {
        let config = SchemeConfig::new()
            .with("switch", Value::Bool(true))
            .with("count", Value::Count(3))
            .with("text", Value::Text("abc".to_string()))
            .with("texts", Value::Texts(vec!["k".to_string()]));
        assert_eq!(
            config.count("switch"),
            None,
            "类型不符即 None，不做隐式转换"
        );
        assert_eq!(config.bool("count"), None);
        assert_eq!(config.text("texts"), None);
        assert_eq!(config.text("text"), Some("abc"));
        assert_eq!(config.texts("texts"), ["k".to_string()].as_slice());
        // 未装配与未知角色一律 `None` / 空切片（回退语义由方案决定）。
        assert_eq!(config.text("missing"), None);
        assert_eq!(config.count("missing"), None);
        assert!(config.texts("missing").is_empty());
        assert!(config.get("missing").is_none());
        // 袋的 `Default` 是**空袋**（不是「全零字段」）：缺角色的回退由方案解析决定。
        assert!(SchemeConfig::default().roles().next().is_none());
    }
}
