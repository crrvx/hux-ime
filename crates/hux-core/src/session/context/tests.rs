// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! 输入上下文单测：实况视图、编辑语义、菜单高亮、选项/属性与提交事件。

use crate::session::*;

fn context_with_menu(texts: &[&str]) -> Context {
    let mut context = Context::new();
    context.set_input(b"ab");
    let mut segment = Segment {
        start: 0,
        end: 2,
        tags: vec!["abc".to_string()],
        prompt: String::new(),
        // 有候选即「已建立菜单」（参照 `menu` 非空）；`highlight` 依此判据。
        translated: true,
        ..Segment::default()
    };
    for text in texts {
        segment
            .candidates
            .push(Candidate::new("sentence", 0, 2, text, ""));
    }
    context.composition.segments.push(segment);
    context.drain_events();
    context
}

/// 提交必须彻底退出组合态；判据同时看组合段与裸输入，避免提交后残留输入被当成仍在输入中。
#[test]
fn is_composing_includes_raw_input() {
    let mut context = Context::new();
    assert!(
        !context.is_composing(),
        "新建上下文不得处于组合态：input 与 composition 都应为空"
    );
    context.push_input(b"a");
    assert!(
        context.is_composing(),
        "有裸输入（尚无组合段）也必须算组合中：IsComposing 判据含 input 非空"
    ); // 无组合但 input 非空（librime 语义）
    assert!(
        context.commit(),
        "存在裸输入时提交必须成功，否则输入无法上屏"
    );
    assert_eq!(
        context.last_commit_text(),
        "a",
        "无候选段时提交文本必须是原始输入切片"
    );
    assert!(
        !context.is_composing(),
        "提交后必须退出组合态：input 与 composition 都应被清空"
    );
}

/// 退格按「自光标向前 pop_n」计数，越界即整条拒绝且不动缓冲区，不允许部分删除。
#[test]
fn pop_input_rejects_out_of_range() {
    let mut context = Context::new();
    context.push_input(b"ab");
    context.set_caret(1);
    assert!(
        !context.pop_input(2),
        "光标前的字节数不足时必须整条拒绝（caret=1 < count=2），不得部分删除"
    ); // caret < count：不改动
    assert_eq!(
        context.input(),
        b"ab",
        "越界退格不得改动输入缓冲区：被拒绝的请求必须是零副作用"
    );
    assert!(context.pop_input(1), "光标前恰好有 1 字节时退格必须成功");
    assert_eq!(
        context.input(),
        b"b",
        "退格应删掉光标前 1 字节并留下其余输入"
    );
}

/// 删除与退格共用越界判据；0 长度仍是有效请求，必须触发一次刷新通知。
#[test]
fn delete_input_rejects_out_of_range() {
    let mut context = Context::new();
    context.push_input(b"ab");
    context.set_caret(1);
    assert!(
        !context.delete_input(2),
        "caret+count 超出输入长度时必须整条拒绝，不得部分删除"
    ); // 超出末尾：不改动
    assert_eq!(
        context.input(),
        b"ab",
        "越界删除不得改动输入缓冲区：被拒绝的请求必须是零副作用"
    );
    context.drain_events();
    assert!(
        context.delete_input(0),
        "0 长度删除是有效请求：必须返回 true，不得当成越界"
    ); // 0 长度：触发更新并返回 true
    assert_eq!(
        context.drain_events(),
        vec![Event::Update],
        "0 长度删除仍须推一次 Update：宿主靠事件刷新视图"
    );
}

/// 「已建菜单」与「有候选」是两件事：菜单在而候选空时要复位选中并通知，未翻译段则完全不碰。
#[test]
fn empty_menu_highlight_resets_selection() {
    let mut context = Context::new();
    context.composition.segments.push(Segment::default());
    context.composition.segments[0].selected_index = 2;
    // 未翻译段（参照 `menu == null`）不改写、不通知。
    assert!(
        !context.highlight(0),
        "段未建立菜单时高亮必须不动作：不得改写 selected_index，也不得推 Update"
    );
    assert_eq!(
        context.composition.segments[0].selected_index, 2,
        "未建菜单的段不得被改写选中索引（应保持原值 2）"
    );
    // 已建立菜单但候选为空（参照 `menu` 存在、`Prepare` 返回 0）：归 0 并在变化时通知。
    context.composition.segments[0].translated = true;
    assert!(
        context.highlight(0),
        "菜单存在而候选为空时必须归 0，并因确有变化返回 true"
    );
    assert_eq!(
        context.composition.segments[0].selected_index, 0,
        "空菜单高亮必须把选中索引夹到 0"
    );
}

/// 无菜单时高亮既不动作也不推事件：事件队列是宿主唯一信号，多推会让宿主重排候选。
#[test]
fn highlight_skips_untranslated_segment_without_update() {
    let mut context = Context::new();
    context.composition.segments.push(Segment {
        selected_index: 3,
        ..Segment::default()
    });
    context.drain_events();
    assert!(!context.highlight(0), "参照 `Highlight` 在无菜单时不动作");
    assert_eq!(
        context.composition.back().unwrap().selected_index,
        3,
        "无菜单时高亮不得改写选中索引（应保持原值 3）"
    );
    assert!(context.drain_events().is_empty(), "无菜单时不得推 Update");
}

/// 光标是字节下标：插入删除都按字节移动，set_caret 超界夹到末尾而不是报错。
#[test]
fn edits_follow_byte_caret_semantics() {
    let mut context = Context::new();
    context.push_input(b"ab");
    context.set_caret(1);
    context.push_input(b"x");
    assert_eq!(context.input(), b"axb", "插入必须落在 caret 处而不是末尾");
    assert_eq!(context.caret(), 2, "插入后 caret 必须前移插入的字节数");
    assert!(context.pop_input(1), "caret 前有 1 字节时退格必须成功");
    assert_eq!(context.input(), b"ab", "退格应删掉刚插入的那个字节");
    assert_eq!(context.caret(), 1, "退格后 caret 必须回到被删字节之前");
    assert!(
        context.delete_input(1),
        "caret 处恰好有 1 字节时删除必须成功"
    );
    assert_eq!(context.input(), b"a", "删除应删掉 caret 处的字节");
    assert!(
        !context.delete_input(1),
        "caret 已在末尾时删除必须失败，不得越过输入长度"
    );
    context.set_input(b"xyz");
    assert_eq!(
        context.caret(),
        3,
        "整体替换输入后 caret 必须移到新输入的末尾"
    );
    context.set_caret(99);
    assert_eq!(
        context.caret(),
        3,
        "set_caret 超界必须夹到输入末尾，而不是报错或越界"
    );
}

/// 高亮越界夹到末位候选，且只有真正变化才返回 true 并通知宿主。
#[test]
fn highlight_clamps_and_reports_changes() {
    let mut context = context_with_menu(&["甲", "乙", "丙"]);
    assert!(
        context.highlight(1),
        "高亮到与当前不同的索引必须返回 true（宿主据此刷新候选）"
    );
    assert_eq!(
        context.composition.back().unwrap().selected_index,
        1,
        "高亮索引必须写入末段的 selected_index"
    );
    assert!(
        !context.highlight(1),
        "索引未变化必须返回 false：不得让宿主做无谓重排"
    );
    assert!(
        context.highlight(99),
        "越界高亮必须夹到末位候选，并因确有变化返回 true"
    );
    assert_eq!(
        context.composition.back().unwrap().selected_index,
        2,
        "越界高亮必须夹到 count-1，不得超出候选数"
    );
}

/// set_option 无条件通知（同值也发），去重交给接收方，宿主据此刷新状态栏。
#[test]
fn set_option_notifies_unconditionally() {
    let mut context = Context::new();
    context.set_option("t", true);
    assert_eq!(
        context.drain_events(),
        vec![Event::Option("t".to_string())],
        "set_option 必须无条件入队一条 Option 事件"
    );
    // 参照 `Context::set_option` 无条件通知：同值再设仍触发。
    context.set_option("t", true);
    assert_eq!(
        context.drain_events(),
        vec![Event::Option("t".to_string())],
        "同值重设仍须通知：去重交给接收方，写入方不得自行吞事件"
    );
}

/// 没有组合段时高亮与确认都必须失败，不能凭空造出候选。
#[test]
fn empty_menu_highlight_fails() {
    let mut context = Context::new();
    assert!(
        !context.highlight(0),
        "没有组合段时高亮必须失败，不得凭空造段"
    );
    context.composition.segments.push(Segment::default());
    assert!(!context.highlight(0), "段未建立菜单时高亮必须失败");
    assert!(
        !context.confirm_current_selection(),
        "无候选且零长度的段确认必须失败，不得凭空造出候选"
    );
}

/// 缓冲模式下 ~ 是标记而非输入：live_* 视图要去掉标记并把光标相应左移，退出缓冲后回归原样。
#[test]
fn buffered_marker_live_views() {
    let mut context = Context::new();
    context.set_buffered(true);
    context.set_input(b"~ab");
    assert_eq!(
        context.live_input(),
        b"ab",
        "缓冲态下 ~ 是私有标记：live_input 必须去掉它"
    );
    context.set_caret(2); // "~a|b"
    assert_eq!(
        context.live_caret(),
        1,
        "live_caret 必须随去掉的 ~ 标记左移一位"
    );
    context.set_buffered(false);
    assert_eq!(
        context.live_input(),
        b"~ab",
        "退出缓冲态后 ~ 不再是标记：live_input 必须原样返回"
    );
    assert_eq!(
        context.live_caret(),
        2,
        "退出缓冲态后 live_caret 必须与 caret 一致（不再减一）"
    );
}

/// 刷新只丢未确认的尾段，已确认段必须保留，并重新追加一个空的开放尾段。
#[test]
fn refresh_pops_open_tail_only() {
    let mut context = context_with_menu(&["甲"]);
    context.composition.segments[0].selected = true;
    assert!(
        !context.refresh_non_confirmed_composition(),
        "只有已确认段时刷新必须返回 false：没有段可回退"
    );
    context.composition.segments.push(Segment::default());
    assert!(
        context.refresh_non_confirmed_composition(),
        "存在未确认尾段时刷新必须回退并返回 true"
    );
    // 已确认段保留 + 追加空尾段（参照 `Segmentation::Forward`）。
    assert_eq!(
        context.composition.segments.len(),
        2,
        "刷新后必须是「已确认段 + 新空尾段」两段"
    );
    assert!(
        context.composition.segments[0].selected,
        "刷新不得清掉已确认段的 selected 标记：用户已上屏的选择会被回退"
    );
    assert_eq!(
        context.composition.segments[1].start, 2,
        "新追加的尾段必须从保留段的末尾开始（Forward 语义）"
    );
    assert_eq!(
        context.composition.segments[1].end, 2,
        "新追加的尾段必须是零长度的开放段"
    );
}

/// 确认把当前高亮位置钉成选中：selected_index 保持、selected 置位。
#[test]
fn confirm_current_selection_accepts_highlight() {
    let mut context = context_with_menu(&["甲", "乙"]);
    context.highlight(1);
    assert!(
        context.confirm_current_selection(),
        "末段有候选时确认必须成功"
    );
    let segment = context.composition.back().unwrap();
    assert_eq!(
        segment.selected_index, 1,
        "确认不得改动高亮位置：selected_index 必须保持确认前的值"
    );
    assert!(
        segment.selected,
        "确认必须把末段标记为已选（status >= kSelected）"
    );
}

/// 提交的事件序是 Commit 在前、Update 在后，随后输入与组合态一并清空。
#[test]
fn commit_emits_text_and_clears() {
    let mut context = context_with_menu(&["甲", "乙"]);
    context.highlight(1);
    assert!(
        context.confirm_current_selection(),
        "末段有候选时确认必须成功，否则提交流程无法前进"
    );
    let expected = context.composition.commit_text(context.input());
    context.drain_events(); // 清掉 highlight/confirm 的残留事件，只断言提交本身
    assert!(context.commit(), "组合非空时提交必须成功");
    assert_eq!(
        context.last_commit_text(),
        expected,
        "last_commit 必须等于提交时的即时 commit_text"
    );
    assert!(!context.is_composing(), "提交后不得残留组合态");
    assert!(
        context.input().is_empty(),
        "提交后输入缓冲区必须清空，避免残留输入被当成仍在输入中"
    );
    let events = context.drain_events();
    assert!(
        matches!(events.first(), Some(Event::Commit(text)) if *text == expected),
        "提交应先派发 Commit：{events:?}"
    );
    assert!(
        matches!(events.get(1), Some(Event::Update)),
        "提交事件序必须是 Commit 在前、Update 在后（宿主靠 Update 刷新视图）"
    );
}

/// 刷新不得动已确认段的选中标记，否则用户已上屏的选择会被回退。
#[test]
fn refresh_keeps_selected_segments() {
    let mut context = context_with_menu(&["甲"]);
    context.composition.segments[0].selected = true;
    context.composition.segments.push(Segment::default());
    assert!(
        context.refresh_non_confirmed_composition(),
        "存在未确认尾段时刷新必须回退并返回 true"
    );
    assert_eq!(
        context.composition.segments.len(),
        2,
        "刷新后必须保留已确认段并追加空尾段，共两段"
    );
    assert!(
        context.composition.segments[0].selected,
        "刷新不得动已确认段的 selected 标记，否则用户已上屏的选择会被回退"
    );
}
