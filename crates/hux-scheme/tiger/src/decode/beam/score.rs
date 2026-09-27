// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! 候选评分：状态求值、字符推进与隔离惩罚。

use super::*;

impl Decoder {
    pub(in crate::decode) fn evaluate_state(&mut self, index: usize) -> Result<Evaluated> {
        let eos_score = self.logp(self.arena[index].prev2, self.arena[index].prev1, EOS)?;
        let path_penalty = self.path_isolation_penalty(index)?;
        let text = self.arena[index].text.clone();
        let code_score = self.arena[index].code_score;
        // 码形与词先验只进排序分；置信度保留旧的**文本级**隔离项，避免
        // 启发式证据制造"高置信早提交"（参照 `evaluate_state`）。
        let ending_adjustment = eos_score - path_penalty + code_score;
        let confidence_ending_adjustment = eos_score - self.isolation_penalty(&text)?;
        let state = &self.arena[index];
        let previous = state.previous;
        let confidence_score = state.mass_score + confidence_ending_adjustment;
        // 直接项（Direct）剥离学习分：整串直出候选按字典序排，学习历史不得改其排序；
        // 学习奖励也不进它的早提交个性化加分（参照 `evaluate_state`）。
        let direct = candidate_is_direct(state.source_mask);
        // 早提交置信度 = 基础置信度 + 有界的个性化加分（补充码表 + 学习），
        // 参照 `evaluate_state` 的 `personalization`。
        let personalization = self.ranking_prior.personalized_early_commit_cap.min(
            self.ranking_prior
                .supplement_early_commit_contribution(state.supplement_score)
                + if direct {
                    0.0
                } else {
                    state.learning_early_commit_bonus
                },
        );
        Ok(Evaluated {
            text: state.text.clone(),
            score: state.score + ending_adjustment
                - if direct { state.learning_score } else { 0.0 },
            confidence_score,
            early_commit_confidence_score: confidence_score + personalization,
            code_score: state.code_score,
            max_rank: state.max_rank.max(1),
            supplement_score: state.supplement_score,
            learning_score: if direct { 0.0 } else { state.learning_score },
            edge_count: state.edge_count,
            source_mask: state.source_mask,
            direct_rank: state.direct_rank,
            path: index,
            segmented: String::new(),
            previous_raw_length: previous.map(|i| self.arena[i].raw_length).unwrap_or(0),
            previous_text: previous.map(|i| self.arena[i].text.clone()),
        })
    }

    /// 参照 `isolation_penalty`：仅按文本的相邻 bigram 判定（置信度专用，
    /// 不被码形证据抬高；参照侧另有按文本缓存，属性能优化，此处不移植）。
    fn isolation_penalty(&mut self, text: &str) -> Result<f64> {
        if self.model.is_none() || !self.lexicon.isolation_enabled || text.is_empty() {
            return Ok(0.0);
        }
        let chars: Vec<char> = text.chars().collect();
        let mut penalty = 0.0;
        for index in 0..chars.len() {
            let rank = self.rank_of_char(chars[index]);
            if rank <= ISOLATION_THRESHOLD {
                continue;
            }
            let left_hit = index > 0 && self.has_observed_bigram(chars[index - 1], chars[index])?;
            let right_hit = index + 1 < chars.len()
                && self.has_observed_bigram(chars[index], chars[index + 1])?;
            if !left_hit && !right_hit {
                penalty += ISOLATION_LAMBDA;
            }
        }
        Ok(penalty)
    }

    fn path_isolation_penalty(&mut self, index: usize) -> Result<f64> {
        if let Some(penalty) = self.arena[index].isolation_penalty {
            return Ok(penalty);
        }
        if self.model.is_none() || !self.lexicon.isolation_enabled {
            return Ok(0.0);
        }
        let previous = self.arena[index].previous;
        let mut penalty = match previous {
            Some(previous) => self.path_isolation_penalty(previous)?,
            None => 0.0,
        };
        let (mut last_char, mut last_weight) = match previous {
            Some(previous) => (
                self.arena[previous].isolation_last_char,
                self.arena[previous].isolation_last_weight,
            ),
            None => (None, 0.0),
        };
        // 4 码及以上主码/显式选重单字边：生僻罚按 `canonical_isolation_factor` 缩放
        // （默认 0.0 即免罚）；其余边系数 1.0（参照 `edge_factor`）。
        let edge_factor = if self.arena[index].edge_primary_single
            && self.arena[index].edge_code_length.unwrap_or(0)
                >= self.ranking_prior.canonical_isolation_min_code_length
        {
            self.ranking_prior.canonical_isolation_factor
        } else {
            1.0
        };
        let edge_chars = self.arena[index].edge_chars.clone();
        for ch in edge_chars {
            let rank = self.rank_of_char(ch);
            let rare = rank > ISOLATION_THRESHOLD;
            let rare_weight = if rare { edge_factor } else { 0.0 };
            let mut linked = false;
            if let Some(last) = last_char
                && (last_weight > 0.0 || rare_weight > 0.0)
            {
                linked = self.has_observed_bigram(last, ch)?;
            }
            if last_weight > 0.0 && linked {
                penalty -= ISOLATION_LAMBDA * last_weight;
            }
            last_weight = if rare && !linked { rare_weight } else { 0.0 };
            if last_weight > 0.0 {
                penalty += ISOLATION_LAMBDA * last_weight;
            }
            last_char = Some(ch);
        }
        self.arena[index].isolation_penalty = Some(penalty);
        self.arena[index].isolation_last_char = last_char;
        self.arena[index].isolation_last_weight = last_weight;
        Ok(penalty)
    }
}

/// 边的累积分：沿候选字符推进的分数、模型上下文与前缀补充状态。
pub(super) struct EdgeScore {
    /// 当前分数。
    pub(super) score: f64,
    /// 前二字符。
    pub(super) prev2: char,
    /// 前一字符。
    pub(super) prev1: char,
    /// 前缀补充状态。
    pub(super) supplement_state: usize,
    /// 本段累计的前缀补充奖励（供 `mass` 扣减）。
    pub(super) supplement_added: f64,
}

impl EdgeScore {
    /// 由路径状态的分数与上下文起算（`supplement_added` 从 0 开始）。
    pub(super) fn start(score: f64, prev2: char, prev1: char, supplement_state: usize) -> Self {
        Self {
            score,
            prev2,
            prev1,
            supplement_state,
            supplement_added: 0.0,
        }
    }

    /// 应用名次惩罚与整串直出奖励（保持原累加顺序）。
    pub(super) fn apply_rank_and_bonus(
        &mut self,
        selected_rank: u64,
        log_rank: f64,
        bonus: Option<f64>,
    ) {
        if selected_rank == 0 {
            self.score -= RANK_PENALTY * log_rank;
        }
        if let Some(bonus) = bonus {
            self.score += bonus;
        }
    }
}

impl Decoder {
    /// 逐字符累加模型分数与发射奖励，并推进前缀补充状态。
    /// 保持原累加顺序（浮点加法不可交换），故不接受合并后的批量求和。
    pub(super) fn advance_chars(&mut self, chars: &[char], from: EdgeScore) -> Result<EdgeScore> {
        let EdgeScore {
            mut score,
            mut prev2,
            mut prev1,
            mut supplement_state,
            mut supplement_added,
        } = from;
        for &ch in chars {
            score += self.logp(prev2, prev1, ch)?;
            score += EMITTED_CHARACTER_REWARD;
            if self.supplement.count > 0 {
                let (state, reward) = self.supplement.advance(supplement_state, ch);
                supplement_state = state;
                score += reward;
                supplement_added += reward;
            }
            prev2 = prev1;
            prev1 = ch;
        }
        Ok(EdgeScore {
            score,
            prev2,
            prev1,
            supplement_state,
            supplement_added,
        })
    }
}
