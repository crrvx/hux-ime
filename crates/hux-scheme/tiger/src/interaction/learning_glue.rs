// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;

// ---------------------------------------------------------------- 学习暂存

/// 融合竞争者（参照 `selected._fusion_ahead` 中的一项）：判定来源与文本即可。
#[derive(Clone, Debug)]
pub struct FusionAhead {
    pub text: String,
    /// 来源标记（`decode::SOURCE_*`）。
    pub source_mask: u8,
}

/// 参照 `learning_selection` 的选中项：文本 + 路径末节点 raw 长度 + `learning.diff` 路径。
#[derive(Clone, Debug)]
pub struct Selected {
    pub text: String,
    pub raw_length: usize,
    pub diff: DiffItem,
    /// 缓冲兜底项（参照 `{text=committed_text, path={raw_length=...}}`，缺 `text_length`）：
    /// 参照在该形态下 `learning.diff` 报错并被 commit 通知器的 pcall 吞掉，不产出学习事件。
    pub buffered_fallback: bool,
    /// 来源标记（参照 `source_mask`）：只有 composed-only 项参与差异学习。
    pub source_mask: u8,
    /// 选中该项时，此前通过过滤的可见候选（参照 `_fusion_ahead`；不含自身）。
    pub fusion_ahead: Vec<FusionAhead>,
}

impl Selected {
    /// 参照缓冲兜底 `{text=committed_text, path={raw_length=#committed_raw}}`。
    /// 参照兜底节点缺 `text_length`（diff 会因比较 nil 报错并被 pcall 吞掉）；
    /// 此处补成良构节点，行为契约为「缓冲空闲时以已确认前缀为选中项」。
    pub fn buffered(committed_raw: &str, committed_text: &str) -> Self {
        let raw_length = committed_raw.len();
        let text_length = committed_text.len();
        Self {
            text: committed_text.to_string(),
            raw_length,
            diff: DiffItem {
                text: committed_text.to_string(),
                path: vec![DiffPathNode {
                    raw_length,
                    text_length,
                }],
            },
            buffered_fallback: true,
            // 兜底项没有来源标记与竞争者（参照兜底表两个字段皆缺）。
            source_mask: 0,
            fusion_ahead: Vec::new(),
        }
    }
}

/// 参照 `learning_selection` 的三元返回（`selected`、`first`、`raw`）。
#[derive(Debug, Default)]
pub struct LearningSelection {
    pub selected: Option<Selected>,
    pub first: Option<Selected>,
    pub raw: Vec<u8>,
}

/// 交互层学习暂存（对应参照 `env._tiger_learning` 的暂存字段；存储/索引归 K3）。
#[derive(Clone, Debug, Default)]
pub struct LiveLearning {
    pub mode: String,
    pub pending: Vec<DiffEvent>,
    pub baseline: Option<Selected>,
    pub submitted_raw: Option<String>,
    pub hide_owned: bool,
    /// 参照 `learned.store and learned.store.db`（K3 学习库就绪后置位）。
    pub store_ready: bool,
    /// 提交点接受的学习事件（`learning::Event`）：核心提交路径与宿主
    /// [`learning_commit`] 调用均入此队列，等待宿主持久化（K3 排空后落库）。
    pub submitted: Vec<Event>,
}

/// 参照 `learning_selection`：按当前段选中项从可见候选中取学习目标。
pub fn learning_selection(
    decoder: &mut Decoder,
    context: &Context,
    state: &SentenceState,
) -> anyhow::Result<LearningSelection> {
    let live = live_input(context);
    let mut raw = state.committed_raw.as_bytes().to_vec();
    raw.extend_from_slice(&live);
    let allow_duplicate_single = set_allow_duplicate_single(context);
    decoder.set_allow_duplicate_single(allow_duplicate_single);
    let target = context
        .composition
        .back()
        .map(|segment| segment.selected_index)
        .unwrap_or(0);
    let lock = state.active_lock().map(|lock| DecodeLock {
        raw: &lock.raw,
        text: &lock.text,
        boundaries: &lock.boundaries,
    });
    let raw_text = String::from_utf8_lossy(&raw).into_owned();
    let decoded = decoder.decode_with_lock(&raw_text, false, &state.committed_text, lock)?;
    let mut first: Option<Selected> = None;
    let mut selected: Option<Selected> = None;
    let mut seen: Vec<FusionAhead> = Vec::new();
    let mut visible = 0usize;
    for item in &decoded.items {
        if implicit_rank_allowed(
            item,
            &raw,
            state.continuation_after_auto_commit,
            allow_duplicate_single,
        ) && item.text.starts_with(&state.committed_text)
            && item.text.len() > state.committed_text.len()
        {
            let (raw_length, diff) = decoder.path_summary(item);
            let candidate = Selected {
                text: item.text.clone(),
                raw_length,
                diff,
                buffered_fallback: false,
                source_mask: item.source_mask,
                fusion_ahead: Vec::new(),
            };
            if first.is_none() {
                first = Some(candidate.clone());
            }
            if visible == target {
                // 参照在此**不 break**：`seen` 是「此前通过过滤的候选」。
                selected = Some(Selected {
                    fusion_ahead: seen.clone(),
                    ..candidate
                });
            }
            seen.push(FusionAhead {
                text: item.text.clone(),
                source_mask: item.source_mask,
            });
            visible += 1;
        }
    }
    if selected.is_none() && live.is_empty() && !state.buffered_text.is_empty() {
        selected = Some(Selected::buffered(
            &state.committed_raw,
            &state.committed_text,
        ));
    }
    Ok(LearningSelection {
        selected,
        first,
        raw,
    })
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
    let baseline = if state.tab_pending {
        live.baseline.as_ref()
    } else {
        submitted_first
    };
    // 与 composed 自学习分离：直接项之间、直接 vs composed 的差异不再是纠错证据
    // （参照 `candidate_is_composed_only(baseline) and candidate_is_composed_only(selected)`）。
    if let Some(baseline) = baseline
        && candidate_is_composed_only(baseline.source_mask)
        && candidate_is_composed_only(selected.source_mask)
    {
        if selected.buffered_fallback {
            // 兜底项没有来源标记（mask 0），上面的 composed 门本已排除它；
            // 这条守卫保留为显式契约：参照的兜底项缺 `path.text_length`，
            // `learning.diff` 会在 boundaries() 报错（旧版由 pcall 吞掉）。
            return;
        }
        let lock_floor = state.active_lock().map(|lock| lock.raw.len()).unwrap_or(0);
        let floor = state.committed_raw.len().max(lock_floor);
        // 参照 `7b220ce`：删除「稳定确认」增量路线（`learning.reinforce`），
        // 未按 Tab 的首选重复确认不再计入等级（等级只由人工纠错推进）。
        let events = learning::diff(
            raw,
            Some(&baseline.diff),
            Some(&selected.diff),
            floor,
            &live.mode,
            now,
        );
        for event in events {
            if live.pending.len() < 256 {
                live.pending.push(event);
            }
        }
    }
    live.baseline = None;
}

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
        context_commit(context, commit_text);
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
