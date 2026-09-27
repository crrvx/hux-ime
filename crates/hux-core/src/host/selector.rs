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
        SelectorAction::PreviousCandidate => {
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
        SelectorAction::NextCandidate => {
            if linear && !caret_at_end {
                false
            } else {
                let Some(segment) = context.composition.back() else {
                    return HostResult::Forward;
                };
                if !segment.translated {
                    return HostResult::Forward;
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
        SelectorAction::PreviousPage => {
            let Some(segment) = context.composition.back() else {
                return HostResult::Forward;
            };
            if !segment.translated {
                return HostResult::Forward;
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
        SelectorAction::NextPage => {
            let Some(segment) = context.composition.back() else {
                return HostResult::Forward;
            };
            if !segment.translated {
                return HostResult::Forward;
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
        SelectorAction::Home => {
            if context.composition.back().is_none() {
                false
            } else if context.composition.back().unwrap().selected_index > 0 {
                context.highlight(0);
                true
            } else {
                false // 交给 navigator 移动光标
            }
        }
        SelectorAction::End => {
            if context.caret() < context.input().len() {
                false // navigator should handle this
            } else {
                selector_action(SelectorAction::Home, context, options) == HostResult::Consumed
            }
        }
    };
    if consumed {
        HostResult::Consumed
    } else {
        HostResult::Forward
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::test_support::*;

    #[test]
    fn selector_moves_highlight_without_wrapping() {
        let mut context = context_with_menu(&["甲", "乙"], 0);
        assert_eq!(press(&mut context, "Up"), HostResult::Consumed);
        assert_eq!(selected(&context), 0);
        assert_eq!(press(&mut context, "Down"), HostResult::Consumed);
        assert_eq!(selected(&context), 1);
        // 末项：吞键不再前进（无环绕）
        assert_eq!(press(&mut context, "Down"), HostResult::Consumed);
        assert_eq!(selected(&context), 1);
    }

    #[test]
    fn selector_end_at_tail_returns_to_first() {
        let mut context = context_with_menu(&["甲", "乙"], 1);
        assert_eq!(press(&mut context, "End"), HostResult::Consumed);
        assert_eq!(selected(&context), 0);
    }

    #[test]
    fn selector_pages_by_default_page_size() {
        let mut context = context_with_menu(&["a", "b", "c", "d", "e", "f"], 0);
        assert_eq!(press(&mut context, "Page_Down"), HostResult::Consumed);
        assert_eq!(selected(&context), 5);
        assert_eq!(press(&mut context, "Page_Up"), HostResult::Consumed);
        assert_eq!(selected(&context), 0);
    }

    #[test]
    fn selector_page_down_is_noop_on_single_page() {
        let mut small = context_with_menu(&["甲", "乙"], 0);
        assert_eq!(press(&mut small, "Page_Down"), HostResult::Consumed);
        assert_eq!(selected(&small), 0);
    }

    /// 翻页循环（`page_cycle`）：**只作用于下翻**（参照 `menu/page_down_cycle` 仅在
    /// `Selector::NextPage` 被读）；首页上翻恒停在首页并写 `paging` 标签（参照
    /// `Selector::PreviousPage` 无循环分支）。
    #[test]
    fn selector_page_cycle_wraps_next_page_only() {
        let mut options = custom_page_options(2);
        options.page_cycle = true;
        let mut context = context_with_menu(&["a", "b", "c", "d", "e"], 0);
        assert_eq!(
            press_with(&mut context, "period", &options),
            HostResult::Consumed
        );
        assert_eq!(selected(&context), 2);
        assert_eq!(
            press_with(&mut context, "period", &options),
            HostResult::Consumed
        );
        assert_eq!(selected(&context), 4);
        // 末页再下 → 首页（`menu/page_down_cycle`）。
        assert_eq!(
            press_with(&mut context, "period", &options),
            HostResult::Consumed
        );
        assert_eq!(selected(&context), 0);
        // 首页再上：参照不循环，只归零高亮 + 写 `paging` 标签。
        assert_eq!(
            press_with(&mut context, "comma", &options),
            HostResult::Consumed
        );
        assert_eq!(selected(&context), 0, "上翻方向不循环（参照无该分支）");
    }

    /// 参照 `Selector::PreviousPage`：`selected_index < page_size` 时 `index = 0`
    /// ——**已在首页也照常归零高亮**（不是「不动」）。
    ///
    /// 参照另写 `comp.back().tags.insert("paging")`；本仓该标签的**唯一读取方**
    /// （`when: paging` 判据）已随「菜单可见即拦截」的用户决定删除，故不再写
    /// （不留「写了但没人读」的字段——反向断言见本用例）。
    #[test]
    fn selector_page_up_home_resets_highlight_without_a_write_only_tag() {
        // 显式 `Page_Up`（selector keymap，不经翻页键绑定）。
        let options = custom_page_options(2);
        let mut context = context_with_menu(&["a", "b", "c", "d", "e"], 1);
        assert_eq!(
            press_with(&mut context, "Page_Up", &options),
            HostResult::Consumed
        );
        assert_eq!(
            selected(&context),
            0,
            "首页上翻归零高亮（参照 Highlight(0)）"
        );
        assert!(
            !context
                .composition
                .back()
                .is_some_and(|segment| segment.has_tag("paging")),
            "本仓不再写 `paging` 标签（唯一读取方已删除，见 `paging_action`）"
        );
    }

    /// 默认不循环：末页/首页翻页只吞键、不动。
    #[test]
    fn selector_page_does_not_wrap_by_default() {
        let options = custom_page_options(2);
        let mut context = context_with_menu(&["a", "b", "c", "d", "e"], 0);
        for expected in [2, 4, 4] {
            assert_eq!(
                press_with(&mut context, "period", &options),
                HostResult::Consumed
            );
            assert_eq!(selected(&context), expected);
        }
        for expected in [2, 0, 0] {
            assert_eq!(
                press_with(&mut context, "comma", &options),
                HostResult::Consumed
            );
            assert_eq!(selected(&context), expected);
        }
    }

    #[test]
    fn selector_uses_configured_page_keys() {
        let options = custom_page_options(2);
        let mut context = context_with_menu(&["a", "b", "c", "d", "e", "f"], 0);
        assert_eq!(
            press_with(&mut context, "period", &options),
            HostResult::Consumed
        );
        assert_eq!(selected(&context), 2);
        assert_eq!(
            press_with(&mut context, "comma", &options),
            HostResult::Consumed
        );
        assert_eq!(selected(&context), 0);
    }

    #[test]
    fn selector_ignores_unconfigured_page_key() {
        let options = custom_page_options(2);
        let mut context = context_with_menu(&["a", "b", "c", "d", "e", "f"], 0);
        assert_eq!(
            press_with(&mut context, "equal", &options),
            HostResult::Forward
        );
    }

    #[test]
    fn selector_uses_configured_page_size_for_navigation() {
        let mut context = context_with_menu(&["a", "b", "c", "d", "e", "f"], 0);
        assert_eq!(
            press_with(&mut context, "Page_Down", &custom_page_options(2)),
            HostResult::Consumed
        );
        assert_eq!(selected(&context), 2);
        let mut single = context_with_menu(&["a", "b", "c", "d", "e", "f"], 0);
        assert_eq!(
            press_with(&mut single, "Page_Down", &custom_page_options(1)),
            HostResult::Consumed
        );
        assert_eq!(selected(&single), 1);
    }

    #[test]
    fn selector_accepts_multiple_page_keys() {
        let options = HostOptions {
            page_size: 2,
            page_up_keys: vec![
                KeyEvent::from_repr("comma").expect("key"),
                KeyEvent::from_repr("bracketleft").expect("key"),
            ],
            page_down_keys: vec![
                KeyEvent::from_repr("period").expect("key"),
                KeyEvent::from_repr("bracketright").expect("key"),
            ],
            page_cycle: false,
        };
        let mut context = context_with_menu(&["a", "b", "c", "d", "e", "f"], 0);
        assert_eq!(
            press_with(&mut context, "period", &options),
            HostResult::Consumed
        );
        assert_eq!(selected(&context), 2);
        // 第二绑定：`]` 同样下翻一页
        assert_eq!(
            press_with(&mut context, "bracketright", &options),
            HostResult::Consumed
        );
        assert_eq!(selected(&context), 4);
        // 第二绑定：`[` 上翻一页
        assert_eq!(
            press_with(&mut context, "bracketleft", &options),
            HostResult::Consumed
        );
        assert_eq!(selected(&context), 2);
    }

    #[test]
    fn selector_requires_translated_segment() {
        let mut context = context_with_menu(&["甲"], 0);
        context.composition.segments[0].translated = false;
        assert_eq!(press(&mut context, "Down"), HostResult::Forward);
    }

    #[test]
    fn selector_skips_raw_segment() {
        let mut raw = context_with_menu(&["甲"], 0);
        raw.composition.segments[0].tags = vec!["raw".to_string()];
        assert_eq!(press(&mut raw, "Down"), HostResult::Forward);
    }
}
