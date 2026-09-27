// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! 按键绑定与翻页判据：镜像 librime `KeyBinder`（参照 schema 的 `Tab`/`Shift+Tab` 绑定；翻页键见 [`paging_action`]）。

use super::selector::{SelectorAction, selector_action};
use super::{HostOptions, HostResult};
use crate::key::{K_SHIFT_MASK, KeyEvent};
use crate::session::Context;

/// 翻页方向（[`paging_action`] 的结果；`Up` = Page_Up 等价动作，`Down` = Page_Down）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PagingDir {
    /// 上翻页（[`SelectorAction::PreviousPage`]）。
    Up,
    /// 下翻页（[`SelectorAction::NextPage`]）。
    Down,
}

/// `key_binder` 的翻页判据：该键是否会被判为翻页。
///
/// 两侧**同一前置条件**（[`menu_available`] = `!ascii_mode && has_menu`）：
/// - 命中 `page_up_keys` → [`PagingDir::Up`]；
/// - 命中 `page_down_keys` → [`PagingDir::Down`]；
/// - 其余（菜单不可用 / 未命中绑定）→ `None`。
///
/// 宿主绑定与方案处理器共用本判据（避免两处条件漂移）。方案侧在「菜单可见 + 可打印 ASCII 标点」
/// 分支入口先问一次：被宿主判为翻页的键（如缺省 `=`/`-`，以及 schema 绑到翻页的 `[`/`]`）
/// 不由该分支消费，让出被其遮蔽的翻页绑定——**本仓有意偏离上游**。
///
/// **上翻页与下翻页同前置，是本仓的语义强化**：参照的 `-` 绑定带
/// `when: paging`（`key_binder.cc:248-266` 的 `kWhenPaging` **只看末段 `paging` 标签**，
/// 先翻过页才吃该键），本仓改为与下翻页同前置「菜单可见即拦截」，不再看标签。
/// **已知并接受的代价**：菜单可见时 `-`/`=`/`[`/`]` 不再能作为标点打出（被判为翻页而消费）。
/// 偏离登记见 `crates/hux-scheme/tiger/tests/key_sequence_differential.rs` 的 `DEVIATIONS`。
///
/// `ascii_mode` 前置**两侧都保留**：`ascii_mode` 打开时翻页键一律不拦截，仍落标点/原路径。
pub fn paging_action(
    context: &Context,
    options: &HostOptions,
    key_event: &KeyEvent,
) -> Option<PagingDir> {
    // 参照 `key_binder.cc` 的绑定查表（`map<KeyEvent,…>::find(key_event)`）是**精确**的
    // `(keycode, modifier)` 比较，而非 `repr()` 字符串比较：后者每次按键多一次分配，
    // 且 `K_MODIFIER_MASK` 内的**无名位**（16-20/24/25）会让不同修饰状态的键在字符串上碰撞。
    let bound = |keys: &[KeyEvent]| {
        keys.iter()
            .any(|key| key.keycode == key_event.keycode && key.modifier == key_event.modifier)
    };
    if !menu_available(context) {
        return None;
    }
    if bound(&options.page_up_keys) {
        return Some(PagingDir::Up);
    }
    if bound(&options.page_down_keys) {
        return Some(PagingDir::Down);
    }
    None
}

/// 参照 `KeyBinder` 的前置条件：非 `ascii_mode` 且 `has_menu`
/// （上/下翻页判据与 [`key_binder`] 的 Tab/Ctrl 绑定共用）。
fn menu_available(context: &Context) -> bool {
    !context.get_option("ascii_mode") && context.has_menu()
}

/// 参照 `KeyBinder::ProcessKeyEvent`：Tab/Shift+Tab 固定，翻页键取 [`HostOptions`]。
/// 翻页判据统一走 [`paging_action`]（菜单不可用时翻页键**不消费**——
/// 交后续处理器落作标点/输入）。
pub(super) fn key_binder(
    key_event: &KeyEvent,
    context: &mut Context,
    options: &HostOptions,
) -> HostResult {
    if let Some(dir) = paging_action(context, options, key_event) {
        return match dir {
            PagingDir::Up => selector_action(SelectorAction::PreviousPage, context, options),
            PagingDir::Down => selector_action(SelectorAction::NextPage, context, options),
        };
    }
    if !menu_available(context) {
        return HostResult::Forward;
    }
    // 固定绑定表同样是精确的 `(keycode, modifier)` 比较：`{Tab,0}` → 下一候选、
    // `{Tab,Shift}` → 上一候选。注意 X11 的 `ISO_Left_Tab`（0xfe20）**不在**该表里
    // （与参照一致）：它由方案处理器/Tab 循环处理，落到宿主链时照旧 Forward。
    if key_event.keycode == 0xff09 && key_event.modifier == 0 {
        return selector_action(SelectorAction::NextCandidate, context, options);
    }
    if key_event.keycode == 0xff09 && key_event.modifier == K_SHIFT_MASK {
        return selector_action(SelectorAction::PreviousCandidate, context, options);
    }
    HostResult::Forward
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::test_support::*;

    #[test]
    fn key_binder_tab_navigates_candidates() {
        let mut context = context_with_menu(&["甲", "乙"], 0);
        assert_eq!(press(&mut context, "Tab"), HostResult::Consumed);
        assert_eq!(selected(&context), 1);
        assert_eq!(press(&mut context, "Shift+Tab"), HostResult::Consumed);
        assert_eq!(selected(&context), 0);
    }

    #[test]
    fn key_binder_tab_passes_without_menu() {
        let mut empty = Context::new();
        empty.set_input(b"x");
        assert_eq!(press(&mut empty, "Tab"), HostResult::Forward);
    }
}
