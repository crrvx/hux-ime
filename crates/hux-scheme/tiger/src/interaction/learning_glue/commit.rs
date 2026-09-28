// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! 提交点编排（参照 `tiger_sentence.lua` 的 `learning_submit` / `learning_commit`
//! 与 `submit_early` 的宿主侧）：筛选并**无条件消费** `pending`，把接受的事件
//! 推入 [`LiveLearning::submitted`] 等待宿主持久化。

use super::*;

/// 参照 `learning_submit`：筛选 `pending`、**无条件消费**，返回待持久化事件。
pub fn learning_submit(
    live: &mut LiveLearning,
    selected: Option<&Selected>,
    actual: &str,
    expected: &str,
) -> Vec<Event> {
    let mut accepted = Vec::new();
    let mut remaining = Vec::new();
    // 参照 `learning_submit` 对兜底项无特判：条件成立就按同一规则筛选，
    // 末尾**无条件**消费 pending 并清空 baseline。
    if let Some(selected) = selected
        && !actual.is_empty()
        && actual == expected
        && !live.mode.is_empty()
    {
        // 融合事件只受 `raw_end` 约束，不参与 composed 分支的文本子串匹配。
        // （参照是 `if raw_end … elseif mode == fusion_mode … elseif mode == live.mode and …`，
        // 两个接受分支的结果相同，此处合并为一个条件。）
        let fusion_mode = learning::fusion_mode(&live.mode);
        for event in &live.pending {
            let accepted_by_fusion = event.mode == fusion_mode;
            let accepted_by_diff = event.mode == live.mode
                && event.text_start >= selected.text.len().saturating_sub(expected.len())
                && selected
                    .text
                    .get(event.text_start..event.text_end)
                    .is_some_and(|text| text == event.text.as_str());
            if event.raw_end > selected.raw_length {
                remaining.push(event.clone());
            } else if accepted_by_fusion || accepted_by_diff {
                accepted.push(event.clone());
            }
        }
    }
    live.pending = remaining;
    live.baseline = None;
    accepted
        .into_iter()
        .map(|event| Event {
            time: event.time,
            mode: event.mode,
            code: event.code,
            text: event.text,
            context: event.context,
        })
        .collect()
}

// ---------------------------------------------------------------- 选项同步

/// 参照 commit 通知器（`prepare_learning` 注册）：选中/暂存/提交一并完成，
/// 接受的事件推入 [`LiveLearning::submitted`]（宿主持久化队列）。
///
/// 注意：核心提交路径（`confirm_selection`、自动上屏的 [`LearningCommit`]）已内置调用；
/// 宿主只应在其**自发**的提交（如候选点击）时调用，否则同一 raw 会重复暂存。
pub fn learning_commit(
    decoder: &mut Decoder,
    context: &Context,
    state: &SentenceState,
    live: &mut LiveLearning,
    now: f64,
    commit_text: &str,
) {
    if live.mode.is_empty() || !live.store_ready {
        return;
    }
    let Ok(selection) = learning_selection(decoder, context, state) else {
        return;
    };
    let raw_text = String::from_utf8_lossy(&selection.raw).into_owned();
    if raw_text.is_empty() || live.submitted_raw.as_deref() == Some(raw_text.as_str()) {
        return;
    }
    live.submitted_raw = Some(raw_text);
    learning_stage(
        live,
        state,
        selection.selected.as_ref(),
        &selection.raw,
        selection.first.as_ref(),
        now,
    );
    // 参照：`expected = buffered_text .. selected.text:sub(#committed_text + 1)`。
    let expected = match &selection.selected {
        Some(selected) => {
            let tail = selected
                .text
                .get(state.committed_text.len()..)
                .unwrap_or_default();
            format!("{}{tail}", state.buffered_text)
        }
        None => String::new(),
    };
    let accepted = learning_submit(live, selection.selected.as_ref(), commit_text, &expected);
    live.submitted.extend(accepted);
}

/// 提交点的学习提交参数：解码器 + 学习暂存 + 注入时间
/// （对应参照 `submit_early(env, ...)` 的宿主侧）。
pub struct LearningCommit<'a> {
    pub decoder: &'a mut Decoder,
    pub live: &'a mut LiveLearning,
    pub now: f64,
}

impl LearningCommit<'_> {
    /// 参照 `submit_early` 的非缓冲分支：提交文本，随后同步执行
    /// ①提交通知器（[`learning_commit`]）与 ②按自动上屏选中项的学习提交；
    /// 接受的事件进入 [`LiveLearning::submitted`]。
    pub fn commit_with_learning(
        &mut self,
        context: &mut Context,
        state: &mut SentenceState,
        commit_text: &str,
        selected_text: &str,
        selected_raw_length: usize,
    ) {
        context.direct_commit(commit_text);
        learning_commit(
            self.decoder,
            context,
            state,
            self.live,
            self.now,
            commit_text,
        );
        // 参照：`{text=selected.text, path={raw_length=selected.raw_length}}`
        // （该形态只用于提交筛选，不参与 diff）。
        let auto = Selected {
            text: selected_text.to_string(),
            raw_length: selected_raw_length,
            diff: DiffItem {
                text: selected_text.to_string(),
                path: Vec::new(),
            },
            buffered_fallback: false,
            source_mask: 0,
            fusion_ahead: Vec::new(),
        };
        let accepted = learning_submit(self.live, Some(&auto), commit_text, commit_text);
        self.live.submitted.extend(accepted);
    }
}

/// 宿主链提交点回调（[`hux_core::host::CommitObserver`] 的方案侧实现）：
/// 把 `host` 处理器链的提交接到学习提交上（core 不持有方案状态）。
pub struct HostCommitObserver<'a> {
    pub decoder: &'a mut Decoder,
    pub live: &'a mut LiveLearning,
    pub state: &'a SentenceState,
    pub now: f64,
}

impl hux_core::host::CommitObserver for HostCommitObserver<'_> {
    fn on_commit(&mut self, context: &Context, commit_text: &str) {
        learning_commit(
            self.decoder,
            context,
            self.state,
            self.live,
            self.now,
            commit_text,
        );
    }
}
