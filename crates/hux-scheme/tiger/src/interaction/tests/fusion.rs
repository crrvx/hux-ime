// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! 融合事件（产出侧 + 接受侧）与融合宿主夹具的用例。

use super::*;

/// 融合学习：`learning_stage` 的融合事件构造与 `learning_submit`
/// 的融合放行分支原先在 tiger crate 内**零覆盖**（`fusion_ahead` 处处是 `Vec::new()`）。
/// 下面两条走**真实链路**（`processor` → 未消费交宿主 → `CompositionBuilder::rebuild`
/// → `update_notifier`，与 `TigerScheme::process_key` + `rebuild` 同序），
/// 由真实解码菜单产出 `fusion_ahead`；其余各条驱动 `learning_stage`/`learning_submit`
/// 的真实入参（上限与保留语义无法经按键到达：单次暂存最多并入「选中项之前的可见候选」条，
/// 且每个提交点都会消费 pending）。
///
/// 夹具事实（`goldens/lexicon`）：`zzzz → 𨰻`（Direct，rank 1）、`zz → 哥哥`（Composed）、
/// `abqt → 瘤`（Direct，rank 1）+ `交田`/`疒田`（Composed）。
fn fusion_decoder() -> Decoder {
    use hux_core::learning::{Event, LearningIndex, fusion_event};
    let mut decoder = lexicon_fixture();
    // 与 `decode_learning` 金样同源的一条成对偏好（Composed「哥哥」胜 Direct「𨰻」），
    // 使 `zzzz` 的菜单变成 [哥哥(C), 𨰻(D)] —— Direct 项之前确有 Composed 项。
    let staged = fusion_event("t", b"zzzz", "𨰻", "哥哥", false, 4, 1000.0).expect("fusion event");
    let event = Event {
        time: staged.time,
        mode: staged.mode,
        code: staged.code,
        text: staged.text,
        context: staged.context,
    };
    decoder.set_learning(LearningIndex::build(&[event], 3_456_000.0), "t");
    decoder
}

#[test]
fn fusion_event_records_direct_win_and_is_accepted() {
    use hux_core::learning::{fusion_mode, fusion_pair_code};
    let mut harness = FusionHarness::new(fusion_decoder());
    for repr in ["z", "z", "z", "z"] {
        assert_eq!(harness.press(repr), ProcessorResult::Consume);
    }
    // 菜单 [哥哥(C), 𨰻(D)]：`2` 直选第 2 项（直选走 `select_candidate_at` 的真实路径）。
    assert_eq!(harness.press("2"), ProcessorResult::Consume);
    assert_eq!(harness.context.last_commit_text(), "𨰻");
    assert_eq!(harness.live.submitted.len(), 1, "融合事件必须被提交点接受");
    let event = &harness.live.submitted[0];
    assert_eq!(event.mode, fusion_mode("t"));
    assert_eq!(
        event.text, "D",
        "选中项是 Direct，之前是 Composed ⇒ Direct 胜"
    );
    assert_eq!(event.code, fusion_pair_code(b"zzzz", "𨰻", "哥哥"));
    assert_eq!(event.time, 0.0);
    assert!(harness.live.pending.is_empty(), "提交点无条件消费 pending");
}

#[test]
fn fusion_event_records_composed_win_and_is_accepted() {
    use hux_core::learning::{fusion_mode, fusion_pair_code};
    let mut harness = FusionHarness::new(lexicon_fixture());
    for repr in ["a", "b", "q", "t"] {
        assert_eq!(harness.press(repr), ProcessorResult::Consume);
    }
    // 菜单 [瘤(D), 交田(C), 疒田(C)]：直选第 2 项（交田，Composed），此前是 瘤（Direct）。
    assert_eq!(harness.press("2"), ProcessorResult::Consume);
    assert_eq!(harness.context.last_commit_text(), "交田");
    assert_eq!(harness.live.submitted.len(), 1);
    let event = &harness.live.submitted[0];
    assert_eq!(event.mode, fusion_mode("t"));
    assert_eq!(
        event.text, "C",
        "选中项是 Composed，之前是 Direct ⇒ Composed 胜"
    );
    assert_eq!(event.code, fusion_pair_code(b"abqt", "瘤", "交田"));
}

/// 成对偏好场景的选中项：本身 composed-only（mask 2），此前可见一项 Direct（mask 1）。
fn fusion_selected(text: &str, raw_length: usize) -> Selected {
    Selected {
        text: text.to_string(),
        raw_length,
        diff: diff_item(text),
        buffered_fallback: false,
        source_mask: 2,
        fusion_ahead: vec![FusionAhead {
            text: "甲".to_string(),
            source_mask: 1,
        }],
    }
}

#[test]
fn fusion_event_is_retained_until_the_selected_raw_is_covered() {
    let state = learning_state();
    let mut live = learning_live("t");
    let long = fusion_selected("交交", 6);
    learning_stage(&mut live, &state, Some(&long), b"ababab", None, 100.0);
    assert_eq!(live.pending.len(), 1);
    assert_eq!(
        live.pending[0].raw_end, 6,
        "raw_end = 暂存时选中项的 raw 长度"
    );
    // 提交点选中项更短（raw 4 < 6）⇒ 保留在 pending（不落库、不强化）。
    let short = fusion_selected("交", 4);
    let accepted = learning_submit(&mut live, Some(&short), "交", "交");
    assert!(accepted.is_empty());
    assert_eq!(live.pending.len(), 1, "raw_end 未覆盖 ⇒ 必须保留");
    // 覆盖到 6 ⇒ 接受。
    let accepted = learning_submit(&mut live, Some(&long), "交交", "交交");
    assert_eq!(accepted.len(), 1);
    assert_eq!(accepted[0].text, "C");
    assert!(live.pending.is_empty());
}

#[test]
fn fusion_events_pass_the_filter_but_unrelated_diff_events_are_dropped() {
    use hux_core::learning::fusion_event;
    let mut live = learning_live("m");
    let fused = fusion_event("m", b"abab", "甲", "交", true, 4, 10.0).expect("fusion event");
    // 同一 pending 中的差异事件：mode 相同但偏移不落在选中文本窗口内 ⇒ 丢弃。
    let stray = DiffEvent {
        time: 10.0,
        mode: "m".to_string(),
        code: "ab".to_string(),
        text: "疒".to_string(),
        context: String::new(),
        raw_start: 0,
        raw_end: 1,
        text_start: 9,
        text_end: 10,
    };
    live.pending = vec![fused, stray];
    let selected = fusion_selected("交交", 4);
    let accepted = learning_submit(&mut live, Some(&selected), "交交", "交交");
    assert_eq!(accepted.len(), 1, "融合事件只受 raw_end 约束，不受子串过滤");
    assert_eq!(accepted[0].text, "D");
    assert!(live.pending.is_empty(), "被丢弃的事件不进 remaining");
    // 提交文本不匹配（actual != expected）⇒ 连融合事件一并丢弃（参照的无条件消费）。
    learning_stage(
        &mut live,
        &state_with_lock("ab", "甲"),
        Some(&selected),
        b"abab",
        None,
        20.0,
    );
    assert_eq!(live.pending.len(), 1);
    let accepted = learning_submit(&mut live, Some(&selected), "交", "交交");
    assert!(accepted.is_empty());
    assert!(live.pending.is_empty());
}

#[test]
fn fusion_pending_is_capped_at_256() {
    // 参照 `#pending < 256`：满 256 后新事件一律丢弃（不增长、不 panic）。
    // 单次按键最多并入「选中项之前的可见候选」条（夹具 ≤ 20），故上限只能靠连续暂存到达。
    let state = learning_state();
    let mut live = learning_live("t");
    let mut selected = fusion_selected("交交", 4);
    selected.fusion_ahead = (0..20)
        .map(|index| FusionAhead {
            text: format!("甲{index}"),
            source_mask: 1,
        })
        .collect();
    for _ in 0..13 {
        learning_stage(&mut live, &state, Some(&selected), b"abab", None, 0.0);
    }
    assert_eq!(
        live.pending.len(),
        256,
        "第 13 次暂存越过上限后必须停在 256"
    );
    learning_stage(&mut live, &state, Some(&selected), b"abab", None, 0.0);
    assert_eq!(live.pending.len(), 256, "已满时继续暂存不得增长");
    // 先入者保留（顺序与去重语义不变）。
    assert_eq!(live.pending[0].code, live.pending[0].code.clone());
    assert!(live.pending.iter().all(|event| event.raw_end == 4));
}
