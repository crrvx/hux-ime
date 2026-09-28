// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! 学习目标的选取：从可见候选中挑出当前段选中项、首个候选与原始串
//! （参照 `learning_selection` 的取值部分）。

use super::*;
use crate::decode::DecodeOutput;

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

/// 参照 `learning_selection` 的输入准备：待解码原始串、数字直选开关与目标段下标。
struct SelectionPrelude {
    raw: Vec<u8>,
    allow_duplicate_single: bool,
    target: usize,
}

fn selection_prelude(context: &Context, state: &SentenceState) -> SelectionPrelude {
    let live = live_input(context);
    let mut raw = state.committed_raw.as_bytes().to_vec();
    raw.extend_from_slice(&live);
    let allow_duplicate_single = set_allow_duplicate_single(context);
    let target = context
        .composition
        .back()
        .map(|segment| segment.selected_index)
        .unwrap_or(0);
    SelectionPrelude {
        raw,
        allow_duplicate_single,
        target,
    }
}

fn selection_lock(state: &SentenceState) -> Option<DecodeLock<'_>> {
    state.active_lock().map(|lock| DecodeLock {
        raw: &lock.raw,
        text: &lock.text,
        boundaries: &lock.boundaries,
    })
}

/// 参照 `learning_selection` 的菜单扫描：`first` 恒为首个通过过滤的候选，
/// `selected` 只在可见序号命中 `target` 时落定；`seen` 是「此前通过过滤的候选」。
fn select_from_menu(
    decoder: &mut Decoder,
    state: &SentenceState,
    decoded: &DecodeOutput,
    prelude: &SelectionPrelude,
) -> (Option<Selected>, Option<Selected>) {
    let mut first: Option<Selected> = None;
    let mut selected: Option<Selected> = None;
    let mut seen: Vec<FusionAhead> = Vec::new();
    let mut visible = 0usize;
    for item in &decoded.items {
        if implicit_rank_allowed(
            item,
            &prelude.raw,
            state.continuation_after_auto_commit,
            prelude.allow_duplicate_single,
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
            if visible == prelude.target {
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
    (first, selected)
}

/// 参照 `learning_selection`：按当前段选中项从可见候选中取学习目标。
pub fn learning_selection(
    decoder: &mut Decoder,
    context: &Context,
    state: &SentenceState,
) -> anyhow::Result<LearningSelection> {
    let prelude = selection_prelude(context, state);
    decoder.set_allow_duplicate_single(prelude.allow_duplicate_single);
    let raw_text = String::from_utf8_lossy(&prelude.raw).into_owned();
    let decoded = decoder.decode_with_lock(
        &raw_text,
        false,
        &state.committed_text,
        selection_lock(state),
    )?;
    let (first, mut selected) = select_from_menu(decoder, state, &decoded, &prelude);
    if selected.is_none() && live_input(context).is_empty() && !state.buffered_text.is_empty() {
        selected = Some(Selected::buffered(
            &state.committed_raw,
            &state.committed_text,
        ));
    }
    Ok(LearningSelection {
        selected,
        first,
        raw: prelude.raw,
    })
}
