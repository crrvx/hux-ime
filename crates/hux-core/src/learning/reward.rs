// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! 奖励链：`reward` 路径链与早提交成熟度/贡献映射。

use super::model::{LearningIndex, RewardNode};
use super::text::{character_count, context};

/// 参照 `M.early_commit_maturity`：把纠错等级分映射到 `0..1` 的成熟度。
///
/// `9`（L1，首次同上下文纠错）→ 0、`11`（L2）→ 0.5、`13`（L3 及以上）→ 1
/// （等级语义见模块文档）。
pub fn early_commit_maturity(score: f64) -> f64 {
    ((score - 9.0) / 4.0).clamp(0.0, 1.0)
}

/// 参照 `M.early_commit_contribution`：单条学习奖励对早提交置信度的有界贡献。
pub fn early_commit_contribution(score: f64) -> f64 {
    (score.max(0.0) * early_commit_maturity(score) * 0.075).min(0.75)
}

/// 参照 `M.reward`：返回 `(best, potential, early_bonus)`。
pub fn reward(
    index: &mut LearningIndex,
    mode: &str,
    raw: &[u8],
    text: &str,
    finish: usize,
    chain: &[RewardNode],
) -> (f64, f64, f64) {
    let seed = chain.first();
    let mut best = seed.map(|node| node.learning_score).unwrap_or(0.0);
    let mut potential = 0.0f64;
    let mut early_bonus = seed
        .map(|node| node.learning_early_commit_bonus)
        .unwrap_or(0.0);
    // 参照 `previous.learning_early_commit_bonus`：整轮迭代都取**链首**（种子）的奖励，
    // 而不是当前 `start` 节点的。
    let seed_bonus = early_bonus;
    if index.codes.is_empty() || mode.is_empty() {
        return (best, potential, early_bonus);
    }
    // 参照以 `while true` 遍历：链走完后还会以 nil start（t=0, r=0）再处理一轮，
    // 等价于对整串做一次根节点评分。
    let mut position = 0usize;
    loop {
        let (node, t, r, node_score) = match chain.get(position) {
            Some(node) => (true, node.text_length, node.raw_length, node.learning_score),
            None => (false, 0, 0, 0.0),
        };
        let fragment = text.get(t..).unwrap_or("");
        if character_count(fragment) > 16 {
            break;
        }
        // 参照 `raw:sub(r + 1, finish)`：两端都做 Lua 式截断。
        let start = r.min(raw.len());
        let end = finish.min(raw.len());
        let code = if start < end {
            std::str::from_utf8(&raw[start..end]).unwrap_or("")
        } else {
            ""
        };
        // 参照 `text:sub(1, t)`：末端截断到文本长度。
        let prefix = text.get(..t.min(text.len())).unwrap_or("");
        let ctx = context(prefix);
        let reward = index.score(mode, code, fragment, &ctx);
        let candidate = node_score + reward;
        let candidate_bonus = seed_bonus.max(early_commit_contribution(reward));
        if candidate > best || (candidate == best && candidate_bonus > early_bonus) {
            best = candidate;
        }
        early_bonus = early_bonus.max(candidate_bonus);
        potential = potential.max(index.prefix_score(mode, code, fragment, &ctx));
        if !node || r == 0 {
            break;
        }
        position += 1;
    }
    (best, potential, early_bonus)
}
