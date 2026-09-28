// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! 早提交提议：从前缀证据中择优挑出提议文本与每个前缀的 raw 边界。

use super::*;

/// 前缀证据的择优结果：早提交提议与其 raw 边界选取。
pub(super) struct Proposal {
    /// 提议文本（无合格前缀时为空串）。
    pub(super) text: String,
    /// 提议份额。
    pub(super) share: f64,
    /// 每个前缀文本选中的 raw 长度。
    pub(super) raw_lengths: Map<String, usize>,
}

/// 参照：先按（份额更大，raw 边界更短）为每个前缀文本选定 raw 长度，
/// 再在闭合边界且份额达标的前缀中按（字符数，份额，raw 边界）择优。
pub(super) fn select_proposal(prefixes: &[PrefixEvidence]) -> Proposal {
    let mut proposal = String::new();
    let mut proposal_share = 0.0;
    let mut proposal_raw_length = 0usize;
    let mut proposal_chars = 0usize;
    let mut raw_lengths: Map<String, usize> = Map::new();
    let mut raw_share: HashMap<String, f64> = HashMap::new();
    for prefix in prefixes {
        if !prefix.boundary_closed {
            continue;
        }
        let replace_raw = match raw_lengths.get(&prefix.text) {
            None => true,
            Some(current_length) => {
                let current_share = raw_share.get(&prefix.text).copied().unwrap_or(0.0);
                prefix.share > current_share
                    || (prefix.share == current_share && prefix.raw_length < *current_length)
            }
        };
        if replace_raw {
            raw_lengths.insert(prefix.text.clone(), prefix.raw_length);
            raw_share.insert(prefix.text.clone(), prefix.share);
        }
        if prefix.share >= EARLY_COMMIT_MINIMUM_SHARE {
            let replace = if proposal.is_empty() {
                true
            } else {
                let prefix_chars = prefix.text_char_count;
                if prefix_chars != proposal_chars {
                    prefix_chars > proposal_chars
                } else if prefix.share != proposal_share {
                    prefix.share > proposal_share
                } else {
                    prefix.raw_length < proposal_raw_length
                }
            };
            if replace {
                proposal = prefix.text.clone();
                proposal_share = prefix.share;
                proposal_raw_length = prefix.raw_length;
                proposal_chars = prefix.text_char_count;
            }
        }
    }
    Proposal {
        text: proposal,
        share: proposal_share,
        raw_lengths,
    }
}
