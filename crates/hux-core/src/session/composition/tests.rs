// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! 组合单测：提交文本与脚本文本的取文规则。

use crate::session::*;

/// selected 标记只影响高亮与显示，不参与取文：未标记段的选中候选同样要被取用。
#[test]
fn commit_text_uses_candidate_without_selected_flag() {
    // 未标记 selected 的段同样按选中候选取文本（librime 语义）
    let mut context = Context::new();
    context.set_input(b"abcd");
    context.composition.segments.push(Segment {
        start: 0,
        end: 2,
        candidates: vec![Candidate::new("sentence", 0, 2, "甲", "")],
        ..Segment::default()
    });
    assert_eq!(
        context.composition.commit_text(context.input()),
        "甲cd",
        "未标 selected 的段同样按选中候选取文本：候选 text 参与，段外输入原样追加"
    );
}

/// 无候选段回退为输入切片，末尾未被段覆盖的输入必须原样追加，不得丢字。
#[test]
fn commit_text_appends_uncovered_input() {
    // 无候选段取输入切片；末尾未被覆盖的输入追加
    let mut context = Context::new();
    context.set_input(b"abcd");
    context.composition.segments.push(Segment {
        start: 0,
        end: 2,
        ..Segment::default()
    });
    assert_eq!(
        context.composition.commit_text(context.input()),
        "abcd",
        "无候选段取输入切片，末尾未被段覆盖的输入必须原样追加，不得丢字"
    );
}

/// 显示串取三段优先级（保留选中的候选文字、候选 preedit、原始输入），keep_selection 决定第一段是否参与。
#[test]
fn script_text_prefers_preedit_then_raw_input() {
    // 参照 `Composition::GetScriptText`：① 确认段 + `keep_selection` ⇒ 候选文字；
    // ② 候选 preedit 非空 ⇒ preedit（去掉首个 `\t`）；③ 否则原始输入切片。
    let mut context = Context::new();
    context.set_input(b"abcd");
    let mut segment = Segment {
        start: 0,
        end: 2,
        translated: true,
        ..Segment::default()
    };
    let mut candidate = Candidate::new("sentence", 0, 2, "甲", "");
    candidate.preedit = "xi\tan".to_string();
    segment.candidates.push(candidate);
    context.composition.segments.push(segment);
    assert_eq!(
        context.get_script_text(),
        "xiancd",
        "preedit 优先且去首个 \\t"
    );
    // 段已确认：`keep_selection = true`（`Context::GetScriptText` 的默认实参）取候选文字。
    context.composition.segments[0].selected = true;
    assert_eq!(
        context.get_script_text(),
        "甲cd",
        "确认段且 keep_selection 时脚本文本必须取候选文字，而不是 preedit 或原文"
    );
    // `keep_selection = false`：确认段仍走 preedit 分支（候选 `text` 不参与）。
    assert_eq!(
        context.composition.script_text(context.input(), false),
        "xiancd",
        "keep_selection=false 时确认段仍须走 preedit 分支（去掉首个 \t），候选 text 不参与脚本文本"
    );
    // 候选既无 preedit 也不保留选中：退回原始输入切片。
    context.composition.segments[0].candidates[0]
        .preedit
        .clear();
    assert_eq!(
        context.composition.script_text(context.input(), false),
        "abcd",
        "候选无 preedit 且不保留选中时必须退回原始输入切片"
    );
}

/// phony 段是内部占位，不得出现在显示串里；段未覆盖的尾巴照常追加。
#[test]
fn script_text_skips_phony_segments_and_appends_tail() {
    let mut context = Context::new();
    context.set_input(b"abcd");
    context.composition.segments.push(Segment {
        start: 0,
        end: 2,
        translated: true,
        tags: vec!["phony".to_string()],
        ..Segment::default()
    });
    // `phony` 段不产出原文；末尾未被段覆盖的输入照常追加。
    assert_eq!(
        context.get_script_text(),
        "cd",
        "phony 段是内部占位：不得产出原文，段外尾巴照常追加"
    );
}

/// 提交串由各段拼接：选中段取候选文字，未覆盖段取输入切片，段之间不得重叠或跳字。
#[test]
fn commit_text_spans_selected_and_raw_segments() {
    let mut context = Context::new();
    context.set_input(b"abcd");
    context.composition.segments.push(Segment {
        start: 0,
        end: 2,
        selected: true,
        candidates: vec![Candidate::new("sentence", 0, 2, "甲", "")],
        selected_index: 0,
        tags: Vec::new(),
        prompt: String::new(),
        translated: true,
    });
    context.composition.segments.push(Segment {
        start: 2,
        end: 4,
        ..Segment::default()
    });
    assert_eq!(
        context.composition.commit_text(context.input()),
        "甲cd",
        "选中段取候选文字、其余段取输入切片：段间不得重叠或跳字"
    );
}
