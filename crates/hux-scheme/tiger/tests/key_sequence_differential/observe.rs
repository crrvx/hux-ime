// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! 单步重放：按键（处理器链 + 宿主链）、事件泵与实测行组装。

use hux_core::host::{HostOptions, HostResult, process_key as host_process_key};
use hux_core::key::KeyEvent;
use hux_core::punct::PunctTable;
use hux_core::session::{Context, Event};
use hux_scheme_tiger::decode::Decoder;
use hux_scheme_tiger::interaction::{
    CompositionBuilder, LiveLearning, ProcessorEnv, ProcessorResult, SentenceState,
    process_key_event, update_notifier,
};
use hux_test_support::hex;

use crate::golden::Step;
use crate::views::{Observed, RowView};

/// 重放一步并生成实测视图；返回（实测, 是否捕获了学习基线）。
#[allow(clippy::too_many_arguments)]
pub fn observe_step(
    case_name: &str,
    step: &Step,
    decoder: &mut Decoder,
    context: &mut Context,
    state: &mut SentenceState,
    live: &mut LiveLearning,
    builder: &mut CompositionBuilder,
    punct: Option<&PunctTable>,
    host_options: &HostOptions,
    page_size: usize,
    dot_armed: &mut bool,
    step_index: usize,
    store_ready: bool,
) -> (Observed, bool) {
    let (consumed, baseline_capture) = press_key(
        case_name,
        step,
        decoder,
        context,
        state,
        live,
        punct,
        host_options,
        page_size,
        dot_armed,
        step_index,
        store_ready,
    );
    let (committed, commit_invalidated) = pump_events(context);
    builder
        .rebuild(decoder, context, state, commit_invalidated, punct)
        .expect("rebuild");
    // 参照 update 通知器（暂存清理 / 缓冲隐藏）。
    update_notifier(context, state, live);
    let row = build_row(consumed, &committed, context, page_size);
    let observation = Observed {
        repr: step.repr.clone(),
        row,
    };
    (observation, baseline_capture)
}

/// 按下一步按键（处理器链 + 宿主链）并做基线捕获的可证伪断言；返回（是否消费, 是否捕获基线）。
#[allow(clippy::too_many_arguments)]
fn press_key(
    case_name: &str,
    step: &Step,
    decoder: &mut Decoder,
    context: &mut Context,
    state: &mut SentenceState,
    live: &mut LiveLearning,
    punct: Option<&PunctTable>,
    host_options: &HostOptions,
    page_size: usize,
    dot_armed: &mut bool,
    step_index: usize,
    store_ready: bool,
) -> (bool, bool) {
    let mut baseline_capture = false;
    let key = KeyEvent::from_repr(&step.repr).expect("key repr");
    // 可证伪断言：夹具 `tab_learning: true`（⇒ `store_ready`）时，
    // 每次「未处于 tab_pending 的 Tab」都必须走参照的基线捕获分支并留下 baseline。
    let expects_baseline = store_ready
        && !state.tab_pending
        && matches!(step.repr.as_str(), "Tab" | "ISO_Left_Tab" | "Shift+Tab");
    let mut env = ProcessorEnv {
        now: 0.0,
        dot_armed,
        min_retained: 0,
        page_size,
        host_options,
    };
    let result =
        process_key_event(&key, context, state, decoder, live, &mut env).expect("processor");
    if expects_baseline {
        assert!(
            live.baseline.is_some(),
            "{}[{}] {}: store_ready 时首次 Tab 必须捕获学习基线（参照 \
             `if not state.tab_pending and learned.store and learned.store.db`）",
            case_name,
            step_index,
            step.repr
        );
        baseline_capture = true;
    }
    // 参照链：处理器未消费的键交宿主等价物（selector/navigator/express_editor 等）。
    let consumed = match result {
        ProcessorResult::Consume => true,
        ProcessorResult::Forward => {
            host_process_key(&key, context, punct, host_options, None) == HostResult::Consumed
        }
    };
    (consumed, baseline_capture)
}

/// 事件泵：提交与选项事件；返回（累计提交文本, 是否发生过失效提交）。
fn pump_events(context: &mut Context) -> (String, bool) {
    let mut committed = String::new();
    let mut commit_invalidated = false;
    for _ in 0..4 {
        let events = context.drain_events();
        if events.is_empty() {
            break;
        }
        for event in events {
            match event {
                Event::Commit(text) => {
                    commit_invalidated = true;
                    committed.push_str(&text);
                }
                Event::Option(_) => {}
                Event::Update => {}
            }
        }
    }
    (committed, commit_invalidated)
}

/// 组装实测行（与金样同口径的 10 列视图）。
fn build_row(consumed: bool, committed: &str, context: &Context, page_size: usize) -> RowView {
    let input = context.input().to_vec();
    let segment = context.composition.back();
    // 参照 `RimeGetContext`：按当前页上报候选、页码与页内高亮（夹具 `menu/page_size`）。
    let highlight = segment
        .map(|segment| segment.selected_index % page_size)
        .unwrap_or(0);
    // `menu.page_no` 同样按选中项所在页上报；无段（无菜单）时为 0
    // （参照的 `RimeMenu` 零初始化）。
    let page_no = segment
        .map(|segment| segment.selected_index / page_size)
        .unwrap_or(0);
    // 参照在 `_hide_candidate` 下把菜单候选数置 0（高亮照常上报）。
    let hidden = context.get_option("_hide_candidate");
    let page: Vec<&hux_core::session::Candidate> = match segment {
        Some(segment) if !hidden => {
            let start = (segment.selected_index / page_size) * page_size;
            let end = (start + page_size).min(segment.candidates.len());
            if start < end {
                segment.candidates[start..end].iter().collect()
            } else {
                Vec::new()
            }
        }
        _ => Vec::new(),
    };
    let candidates: Vec<String> = page
        .iter()
        .map(|candidate| hex(candidate.text.as_bytes()))
        .collect();
    let comments: Vec<String> = page
        .iter()
        .map(|candidate| hex(candidate.comment.as_bytes()))
        .collect();
    RowView {
        consumed,
        input: hex(&input),
        caret: context.caret(),
        commit: hex(committed.as_bytes()),
        page_no,
        highlight,
        count: candidates.len(),
        candidates,
        comments,
    }
}
