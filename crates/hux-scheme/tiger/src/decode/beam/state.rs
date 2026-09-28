// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! 解码状态链：`State` 的中性初值与只读视图、比较器与模型查询。

use super::*;

impl Decoder {
    pub(super) fn new_states(&mut self, length: usize) -> Vec<Bucket> {
        let mut states: Vec<Bucket> = (0..=length).map(|_| Bucket::default()).collect();
        let root = State::neutral();
        self.add_state(&mut states[0], root);
        states
    }

    pub(super) fn logp(&mut self, prev2: char, prev1: char, target: char) -> Result<f64> {
        let Some(model) = self.model.as_mut() else {
            return Ok(0.0);
        };
        model.logp_codes(prev2 as u32, prev1 as u32, target as u32)
    }

    pub(super) fn has_observed_bigram(&mut self, prev: char, target: char) -> Result<bool> {
        let Some(model) = self.model.as_mut() else {
            return Ok(false);
        };
        model.has_observed_bigram_codes(prev as u32, target as u32)
    }

    pub(super) fn current_comparator(&self) -> Comparator {
        if self.model.is_none() {
            Comparator::NoModel
        } else if self.allow_duplicate_single {
            Comparator::ScoreFirst
        } else {
            Comparator::RankFirst
        }
    }

    pub(super) fn state_better(cmp: Comparator, left: &Evaluated, right: &Evaluated) -> bool {
        match cmp {
            Comparator::RankFirst => {
                if left.max_rank != right.max_rank {
                    return left.max_rank < right.max_rank;
                }
                if left.score == right.score {
                    return left.text < right.text;
                }
                left.score > right.score
            }
            Comparator::ScoreFirst => {
                if left.score == right.score {
                    if left.max_rank != right.max_rank {
                        return left.max_rank < right.max_rank;
                    }
                    return left.text < right.text;
                }
                left.score > right.score
            }
            Comparator::NoModel => {
                if left.max_rank != right.max_rank {
                    return left.max_rank < right.max_rank;
                }
                if left.edge_count != right.edge_count {
                    return left.edge_count < right.edge_count;
                }
                if left.score == right.score {
                    return left.text < right.text;
                }
                left.score > right.score
            }
        }
    }

    pub(super) fn state_better_raw(cmp: Comparator, left: &State, right: &State) -> bool {
        match cmp {
            Comparator::RankFirst => {
                if left.max_rank != right.max_rank {
                    return left.max_rank < right.max_rank;
                }
                if left.score == right.score {
                    return left.text < right.text;
                }
                left.score > right.score
            }
            Comparator::ScoreFirst => {
                if left.score == right.score {
                    if left.max_rank != right.max_rank {
                        return left.max_rank < right.max_rank;
                    }
                    return left.text < right.text;
                }
                left.score > right.score
            }
            Comparator::NoModel => {
                if left.max_rank != right.max_rank {
                    return left.max_rank < right.max_rank;
                }
                if left.edge_count != right.edge_count {
                    return left.edge_count < right.edge_count;
                }
                if left.score == right.score {
                    return left.text < right.text;
                }
                left.score > right.score
            }
        }
    }

    pub(super) fn duplicate_better(&self, item: usize, previous: usize) -> bool {
        let item = &self.arena[item];
        let previous = &self.arena[previous];
        if item.learning_score > 0.0
            || previous.learning_score > 0.0
            || item.learning_potential > 0.0
            || previous.learning_potential > 0.0
        {
            return item.score + item.learning_potential
                > previous.score + previous.learning_potential;
        }
        if item.max_rank != previous.max_rank {
            return item.max_rank < previous.max_rank;
        }
        if item.score != previous.score {
            return item.score > previous.score;
        }
        item.edge_count < previous.edge_count
    }

    pub(super) fn rank_of_char(&self, ch: char) -> usize {
        self.rank_of
            .as_ref()
            .and_then(|ranks| ranks.get(&ch).copied())
            .unwrap_or(self.lexicon.unknown_character_rank)
    }
}

/// 对应 `State` 在扩展循环中的只读视图（避免与 `&mut self` 借用冲突）。
pub(super) struct StateView {
    pub(super) score: f64,
    pub(super) mass_score: f64,
    pub(super) code_score: f64,
    pub(super) text: String,
    pub(super) prev2: char,
    pub(super) prev1: char,
    pub(super) max_rank: usize,
    pub(super) supplement_state: usize,
    pub(super) supplement_score: f64,
    pub(super) edge_count: usize,
    pub(super) learning_score: f64,
    pub(super) learning_early_commit_bonus: f64,
}

impl State {
    /// 中性初值：根状态与两个发射构造点的公共起点（`prev2`/`prev1` 为句首哨兵、
    /// 无来源标记、无隔离项、空文本）；各构造点只覆写自己的字段。
    pub(super) fn neutral() -> Self {
        Self {
            score: 0.0,
            mass_score: 0.0,
            code_score: 0.0,
            text: String::new(),
            prev2: BOS,
            prev1: BOS,
            max_rank: 1,
            supplement_state: 1,
            supplement_score: 0.0,
            previous: None,
            edge_chars: Vec::new(),
            text_length: 0,
            raw_length: 0,
            edge_count: 0,
            learning_score: 0.0,
            learning_potential: 0.0,
            learning_early_commit_bonus: 0.0,
            source_mask: SOURCE_UNSET,
            direct_rank: f64::INFINITY,
            isolation_penalty: None,
            isolation_last_char: None,
            isolation_last_weight: 0.0,
            edge_primary_single: false,
            edge_code_length: None,
        }
    }

    pub(super) fn view(&self) -> StateView {
        StateView {
            score: self.score,
            mass_score: self.mass_score,
            code_score: self.code_score,
            text: self.text.clone(),
            prev2: self.prev2,
            prev1: self.prev1,
            max_rank: self.max_rank,
            supplement_state: self.supplement_state,
            supplement_score: self.supplement_score,
            edge_count: self.edge_count,
            learning_score: self.learning_score,
            learning_early_commit_bonus: self.learning_early_commit_bonus,
        }
    }
}
