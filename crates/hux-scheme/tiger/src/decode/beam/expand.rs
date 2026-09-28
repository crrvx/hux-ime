// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! beam 区间扩展：按位置推进候选状态并聚合进桶。
//! 一趟扩展拆成「位置 → 码形边 → 来源状态 × 候选 → 新状态」四层。

use super::super::reachability::eligible_candidates;
use super::score::EdgeScore;
use super::state::StateView;
use super::*;

/// 一趟扩展的只读上下文：原始输入、整串长度、过滤阈值与奖励系数。
struct ExpandContext<'a> {
    /// 原始输入字节。
    raw: &'a [u8],
    /// 整串长度（原 `length`）。
    length: usize,
    /// 消费终点不高于此值的边被过滤。
    minimum_consumed_end: isize,
    /// 每个原始键的码形证据奖励（无模型时为 0）。
    code_reward_per_key: f64,
    /// 是否保护主码稀有单字（`canonical_isolation_factor < 1.0`）。
    protect_primary_rare: bool,
}

/// `position` 处的一条码形边：解析结果与可发射候选。
struct Edge {
    /// 边起点（原始输入下标）。
    position: usize,
    /// 边覆盖的码长。
    code_length: usize,
    /// 边消费到的原始输入终点（含选择器）。
    consumed_end: usize,
    /// 选择器解析出的名次。
    selected_rank: u64,
    /// 该边覆盖整串输入。
    whole_input_edge: bool,
    /// 通过可达性过滤的候选。
    eligible: Vec<Eligible>,
}

/// 扩展的来源状态：arena 下标、是否根节点与只读视图。
struct SourceState<'a> {
    /// arena 下标。
    index: usize,
    /// 是否为根节点（`previous` 为空）。
    is_root: bool,
    /// 状态的只读视图。
    view: &'a StateView,
}

impl SourceState<'_> {
    /// 由来源状态开始逐字符累加。
    fn advance(&self) -> EdgeScore {
        EdgeScore::start(
            self.view.score,
            self.view.prev2,
            self.view.prev1,
            self.view.supplement_state,
        )
    }

    /// 由新分数与补充奖励得到 mass 分（保持原扣减顺序）。
    fn mass_score(&self, score: f64, supplement_added: f64, whole_input_bonus: f64) -> f64 {
        self.view.mass_score + score - self.view.score - supplement_added - whole_input_bonus
    }
}

impl Decoder {
    pub(super) fn expand_range(
        &mut self,
        raw: &[u8],
        states: &mut [Bucket],
        from_pos: usize,
        length: usize,
        minimum_consumed_end: isize,
    ) -> Result<()> {
        let lengths = self.lexicon.lengths.clone();
        // 码形证据只随路径累计、不进 Beam 分数（参照 `expand_range` 顶部）。
        let code_reward_per_key = if self.model.is_some() {
            self.ranking_prior.canonical_code_reward
        } else {
            0.0
        };
        let protect_primary_rare = self.ranking_prior.canonical_isolation_factor < 1.0;
        let ctx = ExpandContext {
            raw,
            length,
            minimum_consumed_end,
            code_reward_per_key,
            protect_primary_rare,
        };
        for position in from_pos..ctx.length {
            self.expand_position(states, position, &lengths, &ctx)?;
        }
        Ok(())
    }

    /// 在 `position` 处按码长推进一轮扩展。
    fn expand_position(
        &mut self,
        states: &mut [Bucket],
        position: usize,
        lengths: &[usize],
        ctx: &ExpandContext<'_>,
    ) -> Result<()> {
        let limit = beam_limit_at(position);
        states[position] = self.dedup_limit(std::mem::take(&mut states[position]), limit);
        if states[position].items.is_empty() {
            return Ok(());
        }
        let current: Vec<usize> = states[position].items.clone();
        let current_truncated = states[position].truncated;
        for &code_length in lengths {
            if position + code_length > ctx.length {
                continue;
            }
            let Some(edge) = self.build_edge(position, code_length, ctx) else {
                continue;
            };
            self.expand_edge(states, &edge, &current, current_truncated, ctx)?;
        }
        Ok(())
    }

    /// 解析 `position` 处长为 `code_length` 的码形边（不可用或未通过过滤时为 `None`）。
    fn build_edge(
        &self,
        position: usize,
        code_length: usize,
        ctx: &ExpandContext<'_>,
    ) -> Option<Edge> {
        let code_bytes = &ctx.raw[position..position + code_length];
        let Ok(code) = std::str::from_utf8(code_bytes) else {
            return None;
        };
        let candidates = self.lexicon.codes.get(code)?;
        let (selected_rank, consumed_end) = parse_selector(ctx.raw, position + code_length);
        let whole_input_edge = position == 0 && consumed_end == ctx.length;
        if (consumed_end as isize) <= ctx.minimum_consumed_end
            || (ctx.length > 1 && consumed_end - position < 2)
        {
            return None;
        }
        let eligible: Vec<Eligible> = eligible_candidates(
            candidates,
            selected_rank,
            whole_input_edge,
            self.allow_duplicate_single,
        )
        .into_iter()
        .map(Eligible::new)
        .collect();
        if eligible.is_empty() {
            return None;
        }
        Some(Edge {
            position,
            code_length,
            consumed_end,
            selected_rank,
            whole_input_edge,
            eligible,
        })
    }

    /// 把一条码形边接到 `current` 中的每条已有状态上，逐候选生成新状态。
    fn expand_edge(
        &mut self,
        states: &mut [Bucket],
        edge: &Edge,
        current: &[usize],
        current_truncated: bool,
        ctx: &ExpandContext<'_>,
    ) -> Result<()> {
        if current_truncated {
            states[edge.consumed_end].truncated = true;
        }
        for &item_index in current {
            let item_is_root = self.arena[item_index].previous.is_none();
            let view = self.arena[item_index].view();
            let source = SourceState {
                index: item_index,
                is_root: item_is_root,
                view: &view,
            };
            for candidate in &edge.eligible {
                let state = self.expand_candidate(edge, &source, candidate, ctx)?;
                self.add_state(&mut states[edge.consumed_end], state);
            }
        }
        Ok(())
    }

    /// 由一条来源状态与一个候选边拼出新状态（累加顺序与拆分前一致）。
    fn expand_candidate(
        &mut self,
        edge: &Edge,
        source: &SourceState<'_>,
        candidate: &Eligible,
        ctx: &ExpandContext<'_>,
    ) -> Result<State> {
        let (code_reward_added, whole_input_bonus) = edge_rewards(edge, candidate, ctx);
        let mut accum = self.advance_chars(&candidate.chars, source.advance())?;
        accum.apply_rank_and_bonus(edge.selected_rank, candidate.log_rank, whole_input_bonus);
        let score = accum.score;
        let text = source.view.text.clone() + &candidate.text;
        let mass_score = source.mass_score(
            score,
            accum.supplement_added,
            whole_input_bonus.unwrap_or(0.0),
        );
        // 整串直出边（Direct）：不参与学习（保留路径既有学习分，
        // 不另计奖励，也不置 `learning_affected`）；参照 `expand_range`。
        let direct_edge = source.is_root && edge.position == 0 && edge.whole_input_edge;
        let (learned, potential, learning_early_bonus) =
            self.candidate_learning(ctx, &text, edge, source, direct_edge);
        Ok(State {
            score: score + learned - source.view.learning_score,
            mass_score,
            code_score: source.view.code_score + code_reward_added,
            text_length: text.len(),
            text,
            prev2: accum.prev2,
            prev1: accum.prev1,
            max_rank: source.view.max_rank.max(candidate.rank),
            supplement_state: accum.supplement_state,
            supplement_score: source.view.supplement_score + accum.supplement_added,
            previous: Some(source.index),
            edge_chars: candidate.chars.clone(),
            raw_length: edge.consumed_end,
            edge_count: source.view.edge_count + 1,
            learning_score: learned,
            learning_potential: potential,
            learning_early_commit_bonus: learning_early_bonus,
            source_mask: if direct_edge {
                SOURCE_DIRECT
            } else {
                SOURCE_COMPOSED
            },
            direct_rank: if direct_edge {
                candidate.rank as f64
            } else {
                f64::INFINITY
            },
            edge_primary_single: ctx.protect_primary_rare
                && candidate.chars.len() == 1
                && (candidate.primary_single || edge.selected_rank > 0),
            edge_code_length: ctx.protect_primary_rare.then_some(edge.code_length),
            ..State::neutral()
        })
    }

    /// 一条候选边的学习奖励三元组（直出边沿用路径既有学习分）。
    fn candidate_learning(
        &mut self,
        ctx: &ExpandContext<'_>,
        text: &str,
        edge: &Edge,
        source: &SourceState<'_>,
        direct_edge: bool,
    ) -> (f64, f64, f64) {
        if direct_edge {
            // 直出边的 `potential` 恒为 0，故此处与 `edge_learning` 的共同判断等价。
            if source.view.learning_score > 0.0 {
                self.learning_affected = true;
            }
            return (
                source.view.learning_score,
                0.0,
                source.view.learning_early_commit_bonus,
            );
        }
        self.edge_learning(
            ctx.raw,
            text,
            edge.consumed_end,
            source.index,
            source.view.learning_score,
        )
    }
}

/// 一条候选边的奖励：主码单字码形证据与整串直出奖励（`None` 表示不适用）。
fn edge_rewards(edge: &Edge, candidate: &Eligible, ctx: &ExpandContext<'_>) -> (f64, Option<f64>) {
    // 主码单字边：按覆盖的原始键数累计码形证据（不入 beam 分）。
    let mut code_reward_added = 0.0;
    if ctx.code_reward_per_key > 0.0
        && edge.selected_rank == 0
        && candidate.primary_single
        && candidate.chars.len() == 1
    {
        code_reward_added = ctx.code_reward_per_key * edge.code_length as f64;
    }
    let whole_input_bonus = if edge.whole_input_edge
        && edge.selected_rank == 0
        && candidate.optimal_single
        && candidate.is_single
    {
        Some(WHOLE_INPUT_SINGLE_CHARACTER_REWARD)
    } else {
        None
    };
    (code_reward_added, whole_input_bonus)
}
