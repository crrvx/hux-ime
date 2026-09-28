// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! 提前上屏的共用提交件：缓冲写回、可见首选约束、共用参数与解码锁打包。

use super::super::*;

/// 参照 `submit_early`：缓冲分支写回缓冲与单锁；否则返回待上屏文本。
pub fn submit_early(
    context: &mut Context,
    state: &mut SentenceState,
    commit: &str,
) -> Option<String> {
    if context.get_option(OPTION_EARLY_COMMIT_TO_PREEDIT) || !state.buffered_text.is_empty() {
        state.buffered_text.push_str(commit);
        state.locks = vec![Lock {
            raw: state.committed_raw.clone(),
            text: state.committed_text.clone(),
            boundaries: format!(
                "{},{};",
                state.committed_raw.len(),
                state.committed_text.len()
            ),
        }];
        state.save(context);
        None
    } else {
        Some(commit.to_string())
    }
}

/// 参照 `auto_commit_matches_visible_top`：置信度（不含末尾排序先验，如词先验/学习重排）
/// 只允许提交与**显示的首选候选**一致的前缀；无显示候选（`None`）时不做该限制
/// （保留不完整尾段合并证据的既有策略）。
pub fn auto_commit_matches_visible_top(visible_top: Option<&str>, text: &str) -> bool {
    visible_top.is_none_or(|top| top.starts_with(text))
}

/// 提前上屏的共用参数（避免 `too_many_arguments`）。
#[derive(Clone, Copy, Debug)]
pub struct EarlyCommitParams {
    pub allow_duplicate_single: bool,
    pub generation: u64,
    pub min_retained: usize,
}

/// 会话锁 → 解码锁（`DecodeLock` 借用 [`Lock`] 的 `raw`/`text`/`boundaries`）。
pub(super) fn decode_lock(lock: &Lock) -> DecodeLock<'_> {
    DecodeLock {
        raw: &lock.raw,
        text: &lock.text,
        boundaries: &lock.boundaries,
    }
}
