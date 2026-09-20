// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;

// ---------------------------------------------------------------- 学习暂存

/// 参照 `learning_selection` 的选中项：文本 + 路径末节点 raw 长度 + `learning.diff` 路径。
#[derive(Clone, Debug)]
pub struct Selected {
    pub text: String,
    pub raw_length: usize,
    pub diff: DiffItem,
    /// 缓冲兜底项（参照 `{text=committed_text, path={raw_length=...}}`，缺 `text_length`）：
    /// 参照在该形态下 `learning.diff` 报错并被 commit 通知器的 pcall 吞掉，不产出学习事件。
    pub buffered_fallback: bool,
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
            };
            if first.is_none() {
                first = Some(candidate.clone());
            }
            if visible == target {
                selected = Some(candidate);
            }
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
    let baseline = if state.tab_pending {
        live.baseline.as_ref()
    } else {
        submitted_first
    };
    if let Some(baseline) = baseline {
        if selected.buffered_fallback {
            // 参照：兜底项缺 `path.text_length`，`learning.diff` 在 boundaries() 报错、
            // 被 commit 通知器的 pcall 吞掉：不产出事件，且 baseline 不被清空。
            return;
        }
        let lock_floor = state.active_lock().map(|lock| lock.raw.len()).unwrap_or(0);
        let floor = state.committed_raw.len().max(lock_floor);
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
    if let Some(selected) = selected {
        if selected.buffered_fallback {
            // 参照：兜底项在 stage 阶段即中止，提交不执行（pending/baseline 均不动）。
            return Vec::new();
        }
        if !actual.is_empty() && actual == expected && !live.mode.is_empty() {
            for event in &live.pending {
                if event.raw_end > selected.raw_length {
                    remaining.push(event.clone());
                } else if event.mode == live.mode
                    && event.text_start >= selected.text.len().saturating_sub(expected.len())
                    && selected
                        .text
                        .get(event.text_start..event.text_end)
                        .is_some_and(|text| text == event.text.as_str())
                {
                    accepted.push(event.clone());
                }
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
