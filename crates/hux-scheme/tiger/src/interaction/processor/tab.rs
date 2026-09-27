// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tab 待确认分支：追加字母后重解码取高亮候选，建锁并按选项早提交。

use super::super::*;
use super::{KeyDispatch, learning_commit};

impl KeyDispatch<'_, '_> {
    /// Tab 待确认落定：命中候选则建锁、清早提交证据，再追加该字母。
    /// 未命中（无可选候选或未越过已提交前缀）返回 `None` 交常规录入。
    pub(super) fn confirm_pending_tab(
        &mut self,
        ch: char,
        full_before: &[u8],
    ) -> anyhow::Result<Option<ProcessorResult>> {
        let Some(candidate) = self.pending_tab_candidate(full_before)? else {
            return Ok(None);
        };
        if candidate.raw_length <= self.state.committed_raw.len() {
            return Ok(None);
        }
        let commit = self.lock_tab_candidate(&candidate, full_before);
        self.clear_tab_evidence();
        if let Some(commit) = commit {
            self.submit_tab_lock(&candidate, &commit);
        }
        let mut restored = full_before[self.state.committed_raw.len()..].to_vec();
        restored.extend_from_slice(ch.to_string().as_bytes());
        restore_composition_input(self.context, &restored);
        Ok(Some(ProcessorResult::Consume))
    }

    /// 追加该字母后重解码，取菜单高亮位（`selected_index`）对应的候选。
    fn pending_tab_candidate(&mut self, full_before: &[u8]) -> anyhow::Result<Option<Selected>> {
        let target = self
            .context
            .composition
            .back()
            .map(|segment| segment.selected_index)
            .unwrap_or(0);
        let lock = self.state.active_lock().map(|lock| DecodeLock {
            raw: &lock.raw,
            text: &lock.text,
            boundaries: &lock.boundaries,
        });
        let raw_text = String::from_utf8_lossy(full_before).into_owned();
        let decoded =
            self.decoder
                .decode_with_lock(&raw_text, false, &self.state.committed_text, lock)?;
        let mut candidate: Option<Selected> = None;
        let mut seen: Vec<FusionAhead> = Vec::new();
        let mut visible = 0usize;
        for item in &decoded.items {
            if self.pending_tab_accepts(item, full_before) {
                if visible == target {
                    let (raw_length, diff) = self.decoder.path_summary(item);
                    candidate = Some(Selected {
                        text: item.text.clone(),
                        raw_length,
                        diff,
                        buffered_fallback: false,
                        source_mask: item.source_mask,
                        // 参照：命中即 `break`，`_fusion_ahead` = 此前通过过滤的候选。
                        fusion_ahead: seen,
                    });
                    break;
                }
                seen.push(FusionAhead {
                    text: item.text.clone(),
                    source_mask: item.source_mask,
                });
                visible += 1;
            }
        }
        Ok(candidate)
    }

    /// 该解码项是否计入可见候选位（早提交过滤 + 已提交前缀 + 确有新增）。
    fn pending_tab_accepts(&self, item: &Evaluated, full_before: &[u8]) -> bool {
        implicit_rank_allowed(
            item,
            full_before,
            self.state.continuation_after_auto_commit,
            self.allow_duplicate_single,
        ) && item.text.starts_with(&self.state.committed_text)
            && item.text.len() > self.state.committed_text.len()
    }

    /// 暂存学习并给候选建锁，返回需要早提交的文本（未开 `OPTION_EARLY_COMMIT` 时为 `None`）。
    fn lock_tab_candidate(&mut self, candidate: &Selected, full_before: &[u8]) -> Option<String> {
        // 参照 `processor` 的 Tab 确认分支：**先** stage、**再**清 `tab_pending`。
        // `learning_stage` 以该标志选择基线（`tab_pending and live.baseline or
        // submitted_first`），且 `reinforce_eligible` 要求 `!tab_pending`；
        // 清标志后再 stage 会把基线取成 `submitted_first` 并误走 reinforce 路线。
        // 参照此处不传 `submitted_first`（nil）；清理后分支即 `return`，本调用不与
        // 提交点的 `learning_commit` 重复（后者对应参照的 commit 通知器，参照同样会走）。
        learning_stage(
            self.live,
            self.state,
            Some(candidate),
            full_before,
            None,
            self.env.now,
        );
        self.state.tab_pending = false;
        let boundaries: String = candidate
            .diff
            .path
            .iter()
            .map(|node| format!("{},{};", node.raw_length, node.text_length))
            .collect();
        let locked_raw = String::from_utf8_lossy(&full_before[..candidate.raw_length]).into_owned();
        self.state.locks.push(Lock {
            raw: locked_raw.clone(),
            text: candidate.text.clone(),
            boundaries,
        });
        if self.context.get_option(OPTION_EARLY_COMMIT) {
            let commit = candidate.text[self.state.committed_text.len()..].to_string();
            self.state.committed_text = candidate.text.clone();
            self.state.committed_raw = locked_raw;
            Some(commit)
        } else {
            None
        }
    }

    /// 清早提交证据并落盘（Tab 确认后的公共收尾）。
    fn clear_tab_evidence(&mut self) {
        reset_early_evidence(self.state);
        self.state.empty_code_pending = None;
        self.state.suspended = false;
        self.state.continuation_after_auto_commit = false;
        self.state.save(self.context);
    }

    /// 把锁定文本交给提交通知器（`submit_early` 命中时）。
    fn submit_tab_lock(&mut self, candidate: &Selected, commit: &str) {
        if let Some(text) = submit_early(self.context, self.state, commit) {
            learning_commit(self.decoder, self.live, self.env.now).commit_with_learning(
                self.context,
                self.state,
                &text,
                &candidate.text,
                candidate.raw_length,
            );
        }
    }
}
