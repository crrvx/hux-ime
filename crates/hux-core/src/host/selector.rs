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
            // 删除后不再写该标签（见 [`paging_action`]，结论按新语义重述）；
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
    use crate::host::{PagingDir, paging_action};
    use crate::punct::PunctTable;

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

    /// 回归场景在新语义下仍须成立：`Page_Up` 停在首页后，紧随的上翻页键
    /// `-` 必须**翻页**（消费、不提交），不得落标点分支把组合提前上屏。
    ///
    /// 与旧语义的差别只在判据来源：原先靠「翻页写入 `paging` 标签」，现在靠「菜单可见」
    /// （[`paging_action`]），故用例在按 `Page_Up` **之前**就断言 `Some(Up)`——
    /// 若有人把标签判据加回来（而标签已无人写），本用例立刻失败。
    #[test]
    fn page_up_key_holds_at_the_first_page() {
        let options = HostOptions::default();
        let mut context = context_with_menu(&["a", "b", "c", "d", "e", "f"], 0);
        assert_eq!(
            paging_action(&context, &options, &key_of("minus")),
            Some(PagingDir::Up),
            "菜单可见即判上翻页（不要求 `paging` 标签）"
        );
        assert_eq!(press(&mut context, "Page_Up"), HostResult::Consumed);
        assert_eq!(
            paging_action(&context, &options, &key_of("minus")),
            Some(PagingDir::Up),
            "首页上翻后判据不变（高亮仍 0，菜单仍在）"
        );
        // 宿主链：`-` 走翻页（消费、不提交、输入不变、高亮留在首页）。
        assert_eq!(press(&mut context, "minus"), HostResult::Consumed);
        assert_eq!(context.last_commit_text(), "");
        assert_eq!(context.input(), b"ab");
        assert_eq!(selected(&context), 0);
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

    /// 新语义（用户决定）：上翻页键**只要菜单可见**就判为翻页并消费，不再要求
    /// 「已翻过页」（参照 `when: paging` 标签已删除）；菜单不可用（无菜单 / `ascii_mode`）时
    /// 不消费——该键继续下落（无标点表时由 `editor` 的可打印字符路径提交组合，
    /// 有标点表时落作标点）。
    #[test]
    fn selector_page_up_requires_a_visible_menu() {
        let mut fresh = context_with_menu(&["a", "b", "c", "d", "e", "f"], 0);
        assert_eq!(
            press(&mut fresh, "minus"),
            HostResult::Consumed,
            "菜单可见时上翻页键生效（首屏亦然）"
        );
        assert_eq!(selected(&fresh), 0, "已在首页 ⇒ 归零高亮（不循环）");
        assert_eq!(fresh.last_commit_text(), "");
        assert_eq!(fresh.input(), b"ab", "翻页不改动输入");

        // 无菜单：不消费（交宿主）。
        let mut idle = Context::new();
        assert_eq!(press(&mut idle, "minus"), HostResult::Forward);

        // `ascii_mode`：两侧翻页键都不拦截（`menu_available` 前置）——键落回后续处理器，
        // 而不是被 `key_binder` 吞成翻页（证据：组合被后续处理器提交，翻页从不提交）。
        let options = HostOptions::default();
        let mut ascii = context_with_menu(&["甲", "乙"], 0);
        ascii.set_option("ascii_mode", true);
        assert_eq!(paging_action(&ascii, &options, &key_of("minus")), None);
        assert_eq!(paging_action(&ascii, &options, &key_of("equal")), None);
        assert_eq!(
            press(&mut ascii, "minus"),
            HostResult::Forward,
            "键交宿主的可打印字符路径（翻页会返回 Consumed）"
        );
        assert_eq!(
            ascii.last_commit_text(),
            "甲",
            "键落回后续处理器（editor 提交组合）；翻页路径不提交"
        );
        assert!(ascii.input().is_empty());
    }

    /// 方案侧「菜单可见 + 标点」分支与宿主 `key_binder` 共用此判据：
    /// 缺省绑定 `=` → Down、`-` → Up，**两侧同前置**（菜单可见；`ascii_mode` 关闭两侧）；
    /// 判据不看 `paging` 标签（本仓语义强化，见函数文档）。
    #[test]
    fn paging_action_is_the_shared_key_binder_predicate() {
        let options = HostOptions::default();
        let mut menu = context_with_menu(&["a", "b", "c", "d", "e", "f"], 0);
        assert_eq!(
            paging_action(&menu, &options, &key_of("equal")),
            Some(PagingDir::Down)
        );
        assert_eq!(
            paging_action(&menu, &options, &key_of("minus")),
            Some(PagingDir::Up),
            "菜单可见即判上翻页（不要求先翻过页）"
        );
        assert_eq!(press(&mut menu, "equal"), HostResult::Consumed);
        assert_eq!(
            paging_action(&menu, &options, &key_of("minus")),
            Some(PagingDir::Up),
            "翻页后判据不变"
        );

        // 无菜单 / `ascii_mode`：两侧一律不成立。
        let idle = Context::new();
        assert_eq!(paging_action(&idle, &options, &key_of("equal")), None);
        assert_eq!(paging_action(&idle, &options, &key_of("minus")), None);
        let mut ascii = context_with_menu(&["a", "b"], 0);
        ascii.set_option("ascii_mode", true);
        assert_eq!(paging_action(&ascii, &options, &key_of("equal")), None);
        assert_eq!(paging_action(&ascii, &options, &key_of("minus")), None);
        // 标签不再参与判据：即便人为置位（本仓已无写入方），`ascii_mode` 下仍不判翻页。
        ascii
            .composition
            .back_mut()
            .expect("段")
            .tags
            .push("paging".to_string());
        assert_eq!(
            paging_action(&ascii, &options, &key_of("minus")),
            None,
            "`paging` 标签已不是判据（旧语义随用户决定退役）"
        );

        // schema 绑定的其它翻页键按 options 生效（`[`/`]`），未绑定的键不判翻页。
        let custom = HostOptions {
            page_size: 2,
            page_up_keys: vec![key_of("bracketleft")],
            page_down_keys: vec![key_of("bracketright")],
            page_cycle: false,
        };
        let mut menu = context_with_menu(&["a", "b", "c", "d", "e", "f"], 0);
        assert_eq!(
            paging_action(&menu, &custom, &key_of("bracketright")),
            Some(PagingDir::Down)
        );
        assert_eq!(
            paging_action(&menu, &custom, &key_of("bracketleft")),
            Some(PagingDir::Up),
            "`[` 与 `-` 同前置（菜单可见即翻页）"
        );
        assert_eq!(
            paging_action(&menu, &custom, &key_of("equal")),
            None,
            "未绑定为翻页键的 `=` 不判翻页（该配置下落标点）"
        );
        assert_eq!(
            press_with(&mut menu, "bracketright", &custom),
            HostResult::Consumed
        );
        assert_eq!(
            paging_action(&menu, &custom, &key_of("bracketleft")),
            Some(PagingDir::Up)
        );
    }

    /// **负向对照（用户决定 B 的另一半）**：`ascii_mode` 打开时翻页键**不**被拦截，
    /// 仍落标点路径（`punctuator` 消费并提交「组合 + 标点」）；同一上下文关掉 `ascii_mode`
    /// 则判为翻页（消费、不提交、输入不变）。
    #[test]
    fn ascii_mode_paging_keys_fall_through_to_punctuation() {
        let table = PunctTable::parse(
            "punctuator:\n  half_shape:\n    \"-\": { commit: － }\n    \"=\": { commit: ＝ }\n",
        )
        .expect("punct table");
        let options = HostOptions::default();
        // `ascii_mode` 打开：`-`/`=` 都不判翻页。
        let mut ascii = context_with_menu(&["甲", "乙"], 0);
        ascii.set_option("ascii_mode", true);
        assert_eq!(
            paging_action(&ascii, &options, &key_of("minus")),
            None,
            "`ascii_mode` 下上翻页键不拦截"
        );
        assert_eq!(
            process(&mut ascii, "minus", Some(&table), &options),
            HostResult::Consumed,
            "`-` 落标点分支"
        );
        assert_eq!(ascii.last_commit_text(), "甲－", "确认组合后落标点");
        assert!(ascii.input().is_empty());
        // 同一形态的上下文关掉 `ascii_mode`：判为翻页（不提交、输入不变）。
        let mut menu = context_with_menu(&["甲", "乙"], 0);
        assert_eq!(
            process(&mut menu, "minus", Some(&table), &options),
            HostResult::Consumed
        );
        assert_eq!(menu.last_commit_text(), "", "翻页不提交");
        assert_eq!(menu.input(), b"ab", "翻页不改动输入");
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
