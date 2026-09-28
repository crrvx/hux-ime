// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! 候选选择器：镜像 librime `Selector`（候选上下移动、翻页、首页/末页与页边界行为）。

use super::{HostOptions, HostResult};
use crate::key::KeyEvent;
use crate::session::Context;

#[derive(Clone, Copy)]
pub(super) enum SelectorAction {
    PreviousCandidate,
    NextCandidate,
    PreviousPage,
    NextPage,
    Home,
    End,
}

impl SelectorAction {
    /// 参照 `Selector::ProcessKeyEvent` 的 Horizontal|Stacked keymap（精确修饰匹配）。
    fn from_key(key_event: &KeyEvent, vertical: bool) -> Option<Self> {
        match (key_event.keycode, vertical) {
            (0xff52 | 0xff97, false) => Some(Self::PreviousCandidate), // Up / KP_Up
            (0xff54 | 0xff99, false) => Some(Self::NextCandidate),     // Down / KP_Down
            (0xff55 | 0xff9a, _) => Some(Self::PreviousPage),          // Page_Up / KP_Prior
            (0xff56 | 0xff9b, _) => Some(Self::NextPage),              // Page_Down / KP_Next
            (0xff50 | 0xff95, _) => Some(Self::Home),                  // Home / KP_Home
            (0xff57 | 0xff9c, _) => Some(Self::End),                   // End / KP_End
            (0xff53 | 0xff98, true) => Some(Self::PreviousCandidate),  // Right / KP_Right（竖排）
            (0xff51 | 0xff96, true) => Some(Self::NextCandidate),      // Left / KP_Left（竖排）
            _ => None,
        }
    }
}

/// 参照 `Selector::ProcessKeyEvent`：组合/菜单前置条件 + keymap。
pub(super) fn selector(
    key_event: &KeyEvent,
    context: &mut Context,
    options: &HostOptions,
) -> HostResult {
    if key_event.alt() || key_event.super_modifier() {
        return HostResult::Forward;
    }
    // 精确匹配（无修饰）；keymap 无 Ctrl/Shift 绑定，FallbackOptions::None。
    if key_event.modifier != 0 {
        return HostResult::Forward;
    }
    let vertical = context.get_option("_vertical");
    let Some(action) = SelectorAction::from_key(key_event, vertical) else {
        return HostResult::Forward;
    };
    let Some(segment) = context.composition.back() else {
        return HostResult::Forward;
    };
    if !segment.translated || segment.has_tag("raw") {
        return HostResult::Forward;
    }
    selector_action(action, context, options)
}

/// 参照 `Selector` 各动作（返回值即 `kAccepted`/`kNoop`）。
pub(super) fn selector_action(
    action: SelectorAction,
    context: &mut Context,
    options: &HostOptions,
) -> HostResult {
    let page_size = options.page_size.max(1);
    let linear = context.get_option("_linear") || context.get_option("_horizontal");
    let caret_at_end = context.caret() >= context.input().len();
    let consumed = match action {
        SelectorAction::PreviousCandidate => previous_candidate(context, linear, caret_at_end),
        SelectorAction::NextCandidate => next_candidate(context, linear, caret_at_end),
        SelectorAction::PreviousPage => previous_page(context, page_size),
        SelectorAction::NextPage => next_page(context, options, page_size),
        SelectorAction::Home => home(context),
        SelectorAction::End => end(context),
    };
    if consumed {
        HostResult::Consumed
    } else {
        HostResult::Forward
    }
}

/// 参照 `Selector::PreviousCandidate`：行内布局交 navigator，堆叠布局吞键不环绕。
fn previous_candidate(context: &mut Context, linear: bool, caret_at_end: bool) -> bool {
    if (linear && !caret_at_end) || context.composition.back().is_none() {
        false // 行内布局交 navigator；无段则无菜单
    } else {
        let index = context.composition.back().unwrap().selected_index;
        if index == 0 {
            // 行内布局回退给 navigator；堆叠布局吞键不环绕。
            !linear
        } else {
            context.highlight(index - 1);
            true
        }
    }
}

/// 参照 `Selector::NextCandidate`：末页不再前进但仍吞键。
fn next_candidate(context: &mut Context, linear: bool, caret_at_end: bool) -> bool {
    if linear && !caret_at_end {
        false
    } else {
        let Some(segment) = context.composition.back() else {
            return false;
        };
        if !segment.translated {
            return false;
        }
        let index = segment.selected_index + 1;
        let candidate_count = segment.prepare(index + 1);
        if candidate_count <= index {
            true // 末页不再前进，但吞键
        } else {
            context.highlight(index);
            true
        }
    }
}

/// 参照 `Selector::PreviousPage`：已在首页也照常归零改写高亮，上翻方向不循环。
fn previous_page(context: &mut Context, page_size: usize) -> bool {
    let Some(segment) = context.composition.back() else {
        return false;
    };
    if !segment.translated {
        return false;
    }
    // 参照 `Selector::PreviousPage`（`gear/selector.cc`）：
    // `index = selected_index < page_size ? 0 : selected_index - page_size` ——
    // **已在首页也照常改写高亮**（归 0，不是「不动」）；参照另写
    // `comp.back().tags.insert("paging")`，本仓随其唯一读取方（`when: paging` 判据）
    // 删除后不再写该标签（见 [`paging_action`]）；
    // 参照的 `menu/page_down_cycle` 只在 `NextPage` 被读，上翻方向**不循环**。
    // `saturating_sub` 即参照三元式 `selected < page_size ? 0 : selected - page_size`：
    // 已在首页（含第一页内的任意高亮）时归 0。
    let index = segment.selected_index.saturating_sub(page_size);
    context.highlight(index);
    true
}

/// 参照 `Selector::NextPage`：末页默认吞键不循环，`page_cycle` 时回到首页。
fn next_page(context: &mut Context, options: &HostOptions, page_size: usize) -> bool {
    let Some(segment) = context.composition.back() else {
        return false;
    };
    if !segment.translated {
        return false;
    }
    let index = segment.selected_index + page_size;
    let page_start = (index / page_size) * page_size;
    let candidate_count = segment.prepare(page_start + page_size);
    if candidate_count <= page_start {
        // 已在末页：默认吞键不循环；开启循环则回到首页。
        if options.page_cycle {
            context.highlight(0);
        }
        true
    } else {
        let index = if index >= candidate_count {
            candidate_count - 1
        } else {
            index
        };
        context.highlight(index);
        true
    }
}

/// 参照 `Selector::Home`：无段不吞键；已有高亮时归零。
fn home(context: &mut Context) -> bool {
    if context.composition.back().is_none() {
        false
    } else if context.composition.back().unwrap().selected_index > 0 {
        context.highlight(0);
        true
    } else {
        false // 交给 navigator 移动光标
    }
}

/// 参照 `Selector::End`：光标不在行尾交 navigator，否则等价于 `Home`。
fn end(context: &mut Context) -> bool {
    if context.caret() < context.input().len() {
        false // navigator should handle this
    } else {
        home(context)
    }
}

#[cfg(test)]
mod tests;
