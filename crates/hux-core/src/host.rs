// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! 宿主等价物：方案侧 `processor` 返回 Forward 后，参照链上由 librime 原生组件
//! （`key_binder` → `speller` → `punctuator` → `selector` → `navigator` → `express_editor`）处理的按键。
//!
//! 分工：`speller` 由**方案侧**处理器承担（`hux-scheme/tiger` 的 `interaction::processor`）；
//! 本模块实现其余组件，不经方案（`punctuator` 用 core 的标点表 [`crate::punct`]）。
//!
//! 映射依据（参照 schema）：
//! - 菜单布局 `Horizontal | Stacked`（未设 `_vertical`/`_linear`/`_horizontal`）；
//! - `menu/page_size: 5`（`page_down_cycle` 缺省 false）；页大小与翻页键可由 addon 经
//!   [`HostOptions`] 配置（缺省取参照 schema 的键：`-` → Page_Up、`=` → Page_Down；**前置条件
//!   按本仓语义**——菜单可见即判翻页，不要求参照 `when: paging` 的标签，见 [`paging_action`]）；
//! - `key_binder/bindings`：`Tab` → Down、`Shift+Tab` → Up（`when: has_menu`）。
//!
//! 简化（有金样覆盖的部分一律按真值实现）：
//! - navigator 的 `spans_` 跳转（多段/词组边界）按单段处理：Left/Right 逐字节移动，
//!   `Ctrl/Shift+Left|Right` 直接跳到首/尾（标点段与词组边界不做 spans 细分）；
//! - `editor/char_handler`（Printables 直接提交）按 `DirectCommit` 语义实现。
//!
//! 结构：顶层保留 [`process_key`] 与共享类型；6 个参照组件各占一个子模块
//! （`commit_notifier` / `punctuator` / `key_binder` / `selector` / `navigator` /
//! `express_editor`），各自镜像上游同名组件并就近放单测。

use crate::key::KeyEvent;
use crate::punct::PunctTable;
use crate::session::Context;

use self::express_editor::editor;
use self::key_binder::key_binder;
use self::navigator::navigator;
use self::punctuator::punctuator;
use self::selector::selector;

mod commit_notifier;
mod express_editor;
mod key_binder;
mod navigator;
mod punctuator;
mod selector;
#[cfg(test)]
mod test_support;

pub use self::key_binder::{PagingDir, paging_action};

/// 参照 schema `menu/page_size`。
pub const DEFAULT_PAGE_SIZE: usize = 5;
/// 页大小上限（配置 `PageSize` 1–10；数字直选 `0`=第 10 个）。
pub const MAX_PAGE_SIZE: usize = 10;

/// 宿主可配置项（addon 设置注入）：每页候选个数与上/下翻页键（可多项）。
///
/// 缺省即参照 schema 的键位：`menu/page_size: 5`、`-` → Page_Up、`=` → Page_Down；
/// 前置条件按本仓语义（**菜单可见即判翻页，两侧同前置**，见 [`paging_action`]）；
/// Page_Up/Page_Down 等导航键不随此变化。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HostOptions {
    /// 每页候选个数（≥ 1；须与宿主候选面板一致）。
    pub page_size: usize,
    /// 上翻页键列表：**菜单可见时生效**（不要求参照 `when: paging` 的末段标签——本仓按
    /// 把上/下翻页统一为「菜单可见即拦截」，见 [`paging_action`]；代价是菜单可见时该键不再落标点）。
    pub page_up_keys: Vec<KeyEvent>,
    /// 下翻页键列表：菜单可用（`has_menu`）时生效。
    pub page_down_keys: Vec<KeyEvent>,
    /// 翻页循环（参照 `menu/page_down_cycle`，默认关）：**末页再下翻回首页**。
    ///
    /// 只有下翻方向与参照一致：参照 `Selector::PreviousPage` 没有循环分支
    /// （首页上翻恒 `Highlight(0)`），故本项不作用于上翻。
    pub page_cycle: bool,
}

impl Default for HostOptions {
    fn default() -> Self {
        Self {
            page_size: DEFAULT_PAGE_SIZE,
            page_up_keys: vec![KeyEvent::from_repr("minus").expect("minus")],
            page_down_keys: vec![KeyEvent::from_repr("equal").expect("equal")],
            page_cycle: false,
        }
    }
}

/// 宿主处理结果：`Consumed` 对应参照链返回 `kAccepted`（吞键）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HostResult {
    Consumed,
    Forward,
}

/// 宿主链提交点回调：由**方案侧**实现（core 不持有方案状态）。
///
/// 对应 librime `Context::Commit()` 内、清空组合前的 `commit_notifier`：
/// `punctuator` / `express_editor` 提交文本时回调，方案据此落学习等。
pub trait CommitObserver {
    fn on_commit(&mut self, context: &Context, commit_text: &str);
}

/// 参照处理器链（`key_binder` → `speller` → `punctuator` → `selector` → `navigator`
/// → `express_editor`；`speller` 由方案侧 `processor` 承担，见模块头）。
///
/// `observer`（可空）供宿主链的提交点回调方案（学习等）。
pub fn process_key(
    key_event: &KeyEvent,
    context: &mut Context,
    punct: Option<&PunctTable>,
    options: &HostOptions,
    observer: Option<&mut dyn CommitObserver>,
) -> HostResult {
    if key_event.release() {
        return HostResult::Forward;
    }
    let mut observer = observer;
    // 依序执行，前一处理器吞键则不再继续（参照引擎的处理器链）。
    let mut result = key_binder(key_event, context, options);
    if result == HostResult::Consumed {
        return result;
    }
    result = punctuator(key_event, context, punct, &mut observer);
    if result == HostResult::Consumed {
        return result;
    }
    result = selector(key_event, context, options);
    if result == HostResult::Consumed {
        return result;
    }
    result = navigator(key_event, context);
    if result == HostResult::Consumed {
        return result;
    }
    editor(key_event, context, &mut observer)
}
