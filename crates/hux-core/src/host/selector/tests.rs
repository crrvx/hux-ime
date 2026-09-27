// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! 候选选择器单测：高亮移动、翻页与循环、页大小与可配置翻页键。

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
