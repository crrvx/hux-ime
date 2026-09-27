// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! 学习奖励：沿 arena 链物化奖励节点后委托 core 计算，并按边汇总奖励三元组。

use super::*;

/// 参照 `learning.reward`，但沿解码状态链（arena）读取节点。
/// 返回 `(best, potential, early_bonus)`（参照三元返回）。
pub(super) fn learning_reward(
    index: &mut LearningIndex,
    mode: &str,
    arena: &[State],
    raw: &[u8],
    text: &str,
    finish: usize,
    start: usize,
) -> (f64, f64, f64) {
    // 算法只在 core 维护一份（`learning::reward`）：此处把 arena 的路径物化为其链表示。
    let mut chain = Vec::new();
    let mut current = Some(start);
    while let Some(position) = current {
        let state = &arena[position];
        chain.push(hux_core::learning::RewardNode {
            learning_score: state.learning_score,
            learning_early_commit_bonus: state.learning_early_commit_bonus,
            text_length: state.text_length,
            raw_length: state.raw_length,
        });
        current = if state.raw_length > 0 {
            state.previous
        } else {
            None
        };
    }
    hux_core::learning::reward(index, mode, raw, text, finish, &chain)
}

impl Decoder {
    /// 计算一条边的学习奖励并标记 `learning_affected`；无学习接线时返回 `fallback`。
    pub(super) fn edge_learning(
        &mut self,
        raw: &[u8],
        text: &str,
        consumed_end: usize,
        previous: usize,
        fallback: f64,
    ) -> (f64, f64, f64) {
        let (learned, potential, bonus) = match &mut self.learning {
            Some(wiring) => learning_reward(
                &mut wiring.index,
                &wiring.mode,
                &self.arena,
                raw,
                text,
                consumed_end,
                previous,
            ),
            None => (fallback, 0.0, 0.0),
        };
        if learned > 0.0 || potential > 0.0 {
            self.learning_affected = true;
        }
        (learned, potential, bonus)
    }
}
