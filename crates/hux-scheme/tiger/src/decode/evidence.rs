// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! 早提交证据：`Decoder` 的第二个 impl——从扩展池收集证据候选并汇总前缀证据。
//!
//! 与主 impl（`beam`）的分工：主 impl 推进解码状态并发射候选；此处只读候选池与
//! arena 纯计算证据，不推进状态。
//!
//! 子模块：`collect` 收集候选池并合并不完整尾码，`prefix` 按前缀汇总两套份额，
//! `proposal` 从前缀证据中择优提议。

mod collect;
mod prefix;
mod proposal;

use self::collect::has_low_confidence_completed_generation;
use self::prefix::{build_prefix_evidence, prefix_lookup};
use self::proposal::select_proposal;
use super::*;

// ---------------------------------------------------------------- 早提交证据

impl Decoder {
    /// 参照 `build_early_commit_evidence`。
    pub(super) fn build_early_commit_evidence(
        &mut self,
        raw: &[u8],
        states: &mut [Bucket],
        completed: &[Evaluated],
        completed_truncated: bool,
        required_text_prefix: &str,
    ) -> Result<Evidence> {
        // 截断池不再早退：保留已算出的质量用于「强证据」策略（强证据只认 BaseShare）。
        let mut pool: Vec<EvidenceCandidate> = Vec::new();
        let mut pool_index: HashMap<usize, HashMap<String, usize>> = HashMap::new();
        let visible =
            self.collect_pool(&mut pool, &mut pool_index, completed, required_text_prefix);
        let (truncated, merged_incomplete_tail) = self.merge_incomplete_tails(
            raw,
            states,
            &mut pool,
            &mut pool_index,
            required_text_prefix,
            completed_truncated,
        )?;

        let prefixes = build_prefix_evidence(&pool, &self.arena);
        let proposal = select_proposal(&prefixes);
        let by_boundary = prefix_lookup(&prefixes);
        Ok(Evidence {
            prefixes,
            by_boundary,
            proposal: proposal.text,
            proposal_share: proposal.share,
            raw_lengths: proposal.raw_lengths,
            neutral_incomplete_tail: visible.is_empty() && merged_incomplete_tail,
            merged_incomplete_tail,
            neutral_low_confidence: has_low_confidence_completed_generation(&visible),
            confidence_truncated: truncated,
        })
    }
}
