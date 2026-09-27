// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tab 锁确认路径：基线写入、学习暂存分支与提交点通知器的交互。

use super::*;

/// 回归（既有缺陷）：Tab 锁确认路径必须**在清 `state.tab_pending` 之前** stage 学习事件。
///
/// 参照 `processor` 的 Tab 确认分支顺序是
/// `learning_stage(env, state, selected, full_before)` → `state.tab_pending = false`；
/// `learning_stage` 用该标志选基线（`tab_pending and live.baseline or submitted_first`），
/// 且稳定确认（`reinforce`）路线要求 `!tab_pending`。本仓曾只用分支末尾的提交点补 stage，
/// 于是基线取成 `submitted_first` 并可能误走 reinforce——金样覆盖不到（探针
/// `store_ready == false` 使该路径短路），故这里经 `processor` 走端到端。
#[test]
fn processor_tab_confirm_stages_against_live_baseline() {
    let mode = "sentence-v2|rules=|optimal=1500|dup=1";
    let mut h = Harness::new();
    h.live.mode = mode.to_string();
    h.live.store_ready = true;
    for repr in ["a", "b", "a", "b"] {
        assert_eq!(h.press(repr), ProcessorResult::Consume);
    }
    assert_eq!(h.context.input(), b"abab");
    // 真实会话里菜单由 translator 建立（交交 / 交疒 = 两条 2 码边，均为 composed-only）。
    h.push_segment(b"abab", &["交交", "交疒"]);
    // Tab：写基线（本菜单首个可见候选）并把高亮移到下一项
    assert_eq!(h.press("Tab"), ProcessorResult::Consume);
    assert!(h.state.tab_pending);
    let baseline = h.live.baseline.clone().expect("Tab 按下时应写入基线");
    assert_eq!(baseline.text, "交交");
    // 高亮已在第 2 项：此刻按同一解码取到的 selected 就是确认分支将选中的候选。
    let selection = learning_selection(&mut h.decoder, &h.context, &h.state).expect("selection");
    let selected = selection.selected.clone().expect("第 2 个候选");
    assert_eq!(selected.text, "交疒");
    let expected = learning::diff(
        b"abab",
        Some(&baseline.diff),
        Some(&selected.diff),
        0,
        mode,
        0.0,
    );
    assert_eq!(expected.len(), 1, "夹具前提：交交 / 交疒 仅末段不同");
    // 字母确认走 Tab 确认分支（候选 raw 长度超过已确认前缀）
    assert_eq!(h.press("c"), ProcessorResult::Consume);
    assert!(!h.state.tab_pending);
    assert!(
        h.live.baseline.is_none(),
        "stage 必须已按 tab_pending 分支消费基线"
    );
    // 早提交选项关闭 ⇒ 不触发提交点通知器：pending 只能来自 Tab 分支里的 stage。
    assert_eq!(h.live.pending.len(), expected.len());
    for (got, want) in h.live.pending.iter().zip(expected.iter()) {
        assert_eq!(got, want);
    }
    // 融合事件不参与：DiffEvent 的模式仍是实时模式，而非 `fusion-v1|…`。
    assert_eq!(h.live.pending[0].mode, mode);
}
