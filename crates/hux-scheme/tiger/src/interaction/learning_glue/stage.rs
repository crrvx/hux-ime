// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! 学习事件的暂存：把 `before -> selected` 的融合事件与差异事件并入 `pending`
//! （参照 `learning_stage`）。

use super::*;

/// 参照 `learning_stage` 的融合段：跨来源偏好是成对的，只记录一条方向事件。
fn stage_fusion_events(live: &mut LiveLearning, selected: &Selected, raw: &[u8], now: f64) {
    // 跨来源偏好是成对的：选中较低的 Direct 而非更靠前的 Composed（或反之）
    // 只记录 `Direct > Composed`（或反向）一条，不改任一来源的内部顺序。
    // 参照把它放在函数最前，先于基线与兜底判定。
    for ahead in &selected.fusion_ahead {
        let event = if candidate_is_direct(selected.source_mask)
            && candidate_is_composed_only(ahead.source_mask)
        {
            learning::fusion_event(
                &live.mode,
                raw,
                &selected.text,
                &ahead.text,
                true,
                selected.raw_length,
                now,
            )
        } else if candidate_is_composed_only(selected.source_mask)
            && candidate_is_direct(ahead.source_mask)
        {
            learning::fusion_event(
                &live.mode,
                raw,
                &ahead.text,
                &selected.text,
                false,
                selected.raw_length,
                now,
            )
        } else {
            None
        };
        if let Some(event) = event
            && live.pending.len() < 256
        {
            live.pending.push(event);
        }
    }
}

/// 参照 `learning_stage` 的差异段：两侧皆 composed-only 时才记账，返回待入队事件。
fn diff_events(
    state: &SentenceState,
    mode: &str,
    baseline: &Selected,
    selected: &Selected,
    raw: &[u8],
    now: f64,
) -> Vec<DiffEvent> {
    if selected.buffered_fallback {
        // 兜底项没有来源标记（mask 0），上面的 composed 门本已排除它；
        // 这条守卫保留为显式契约：参照的兜底项缺 `path.text_length`，
        // `learning.diff` 会在 boundaries() 报错（旧版由 pcall 吞掉）。
        return Vec::new();
    }
    let lock_floor = state.active_lock().map(|lock| lock.raw.len()).unwrap_or(0);
    let floor = state.committed_raw.len().max(lock_floor);
    // 参照 `7b220ce`：删除「稳定确认」增量路线（`learning.reinforce`），
    // 未按 Tab 的首选重复确认不再计入等级（等级只由人工纠错推进）。
    learning::diff(
        raw,
        Some(&baseline.diff),
        Some(&selected.diff),
        floor,
        mode,
        now,
    )
}

/// 把事件并入 `pending`（上限 256 条；与参照一致，超出即丢弃）。
fn push_pending(live: &mut LiveLearning, events: Vec<DiffEvent>) {
    for event in events {
        if live.pending.len() < 256 {
            live.pending.push(event);
        }
    }
}

/// 参照 `learning_stage`：把 `before -> selected` 的差异事件并入 `pending`。
pub fn learning_stage(
    live: &mut LiveLearning,
    state: &SentenceState,
    selected: Option<&Selected>,
    raw: &[u8],
    submitted_first: Option<&Selected>,
    now: f64,
) {
    if live.mode.is_empty() {
        return;
    }
    let Some(selected) = selected else {
        return;
    };
    stage_fusion_events(live, selected, raw, now);
    let baseline = if state.tab_pending {
        live.baseline.clone()
    } else {
        submitted_first.cloned()
    };
    // 与 composed 自学习分离：直接项之间、直接 vs composed 的差异不再是纠错证据
    // （参照 `candidate_is_composed_only(baseline) and candidate_is_composed_only(selected)`）。
    if let Some(baseline) = &baseline
        && candidate_is_composed_only(baseline.source_mask)
        && candidate_is_composed_only(selected.source_mask)
    {
        let events = diff_events(state, &live.mode, baseline, selected, raw, now);
        push_pending(live, events);
    }
    live.baseline = None;
}
