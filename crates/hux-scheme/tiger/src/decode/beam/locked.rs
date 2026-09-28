// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! 锁播种：已确认前缀的边缘解析与播种（`resolve_locked_edge`、`seed_locked`）。
//! 播种按边界逐个推进，每个边界装配一个种子状态后压入 arena。

use super::score::EdgeScore;
use super::*;

/// 锁播种的进度：arena 中已播种节点下标与已消费的已确认文本长度。
struct SeedCursor {
    /// 当前种子节点的 arena 下标。
    index: usize,
    /// 已消费的已确认文本长度（字符偏移）。
    text_length: usize,
}

/// 锁播种的只读配置：码形证据系数与主码稀有单字保护开关。
struct SeedSetup {
    /// 每个原始键的码形证据奖励（无模型时为 0）。
    code_reward_per_key: f64,
    /// 是否保护主码稀有单字（`canonical_isolation_factor < 1.0`）。
    protect_primary_rare: bool,
}

/// 种子节点的标量快照（读取后即可释放对 arena 的借用）。
struct SeedSnapshot {
    /// 节点下标。
    index: usize,
    /// 累积分数。
    score: f64,
    /// 已消费的原始输入长度。
    raw_length: usize,
    /// 码形证据分。
    code_score: f64,
    /// 前二字符。
    prev2: char,
    /// 前一字符。
    prev1: char,
    /// 前缀补充状态。
    supplement_state: usize,
    /// 前缀补充分。
    supplement_score: f64,
    /// 学习分。
    learning_score: f64,
    /// 边数。
    edge_count: usize,
}

impl SeedSnapshot {
    /// 读取 `state` 的标量字段（`index` 为其 arena 下标）。
    fn read(state: &State, index: usize) -> Self {
        Self {
            index,
            score: state.score,
            raw_length: state.raw_length,
            code_score: state.code_score,
            prev2: state.prev2,
            prev1: state.prev1,
            supplement_state: state.supplement_state,
            supplement_score: state.supplement_score,
            learning_score: state.learning_score,
            edge_count: state.edge_count,
        }
    }

    /// 由种子节点开始逐字符累加。
    fn advance(&self) -> EdgeScore {
        EdgeScore::start(self.score, self.prev2, self.prev1, self.supplement_state)
    }
}

/// 反解出的已确认边：字符序列、主码稀有单字保护标记、码长与码形证据奖励。
struct LockedEdge {
    /// 边字符。
    chars: Vec<char>,
    /// 是否按主码稀有单字保护。
    primary_single: bool,
    /// 受保护的码长（不保护时为 `None`）。
    code_length: Option<usize>,
    /// 已确认边的码形证据奖励（与普通扩展一致地累计，只进排序分，不进 mass）。
    code_reward: Option<f64>,
}

/// 由 `resolve_locked_edge` 的结果与边界文本还原边字符与保护元数据。
fn locked_edge(
    resolved: &Option<(Vec<char>, bool, usize, u64)>,
    edge_text: &str,
    protect_primary_rare: bool,
    code_reward_per_key: f64,
) -> LockedEdge {
    let chars: Vec<char> = match resolved {
        Some((chars, _, _, _)) => chars.clone(),
        None => edge_text.chars().collect(),
    };
    let (primary_single, code_length) = match resolved {
        Some((_, primary_single, code_length, selected_rank)) => (
            protect_primary_rare && chars.len() == 1 && (*primary_single || *selected_rank > 0),
            protect_primary_rare.then_some(*code_length),
        ),
        None => (false, None),
    };
    let code_reward = match resolved {
        Some((_, primary_single, code_length, selected_rank))
            if code_reward_per_key > 0.0
                && *selected_rank == 0
                && *primary_single
                && chars.len() == 1 =>
        {
            Some(code_reward_per_key * *code_length as f64)
        }
        _ => None,
    };
    LockedEdge {
        chars,
        primary_single,
        code_length,
        code_reward,
    }
}

/// 参照 `sub` 会把越界端点截到串尾；先夹取再按字节取。
/// 返回（本边文本、到 `text_length` 为止的已确认文本）。
fn locked_texts<'a>(
    lock: &'a DecodeLock<'_>,
    from: usize,
    text_length: usize,
) -> (&'a str, String) {
    let text_end = text_length.min(lock.text.len());
    let edge_text = lock.text.get(from..text_end).unwrap_or("");
    let text = lock
        .text
        .get(..text_length)
        .unwrap_or(lock.text)
        .to_string();
    (edge_text, text)
}

/// 单个已确认边界播种出的中间结果。
struct SeedParts {
    /// 已消费的原始输入长度。
    raw_length: usize,
    /// 已消费的已确认文本长度（字符偏移）。
    text_length: usize,
    /// 到 `text_length` 为止的已确认文本。
    text: String,
    /// 反解出的边。
    edge: LockedEdge,
    /// 已重放的累积分。
    accum: EdgeScore,
}

impl Decoder {
    /// 参照 `decode` 的 locked 播种：按 `boundaries` 重建已确认前缀的路径与分数
    /// （不重搜索、不允许边跨过锁），成功后把种子放入 `states[#prefix]`。
    /// 参照 `ranking_prior.resolve_locked_edge`：从已确认的 raw/text 边界反解码表边。
    /// 返回（边字符、主码单字标记、码长、选中名次）。
    fn resolve_locked_edge(
        &self,
        raw: &[u8],
        raw_start: usize,
        raw_end: usize,
        text: &str,
    ) -> Option<(Vec<char>, bool, usize, u64)> {
        for &code_length in &self.lexicon.lengths {
            let code_end = raw_start + code_length;
            if code_end > raw_end {
                continue;
            }
            let Ok(code) = std::str::from_utf8(&raw[raw_start..code_end]) else {
                continue;
            };
            let Some(candidates) = self.lexicon.codes.get(code) else {
                continue;
            };
            let (selected_rank, consumed_end) = parse_selector(raw, code_end);
            if consumed_end != raw_end {
                continue;
            }
            if selected_rank > 0 {
                if let Some(candidate) = candidates.get(selected_rank as usize - 1)
                    && candidate.text == text
                {
                    return Some((
                        candidate.text.chars().collect(),
                        candidate.primary_single,
                        code_length,
                        selected_rank,
                    ));
                }
            } else {
                // 整段菜单可以不写选择器而锁定非首候选。
                for candidate in candidates {
                    if candidate.text == text {
                        return Some((
                            candidate.text.chars().collect(),
                            candidate.primary_single,
                            code_length,
                            0,
                        ));
                    }
                }
            }
        }
        None
    }

    pub(super) fn seed_locked(
        &mut self,
        raw: &[u8],
        states: &mut [Bucket],
        prefix: &[u8],
        lock: &DecodeLock<'_>,
    ) -> Result<bool> {
        let mut cursor = SeedCursor {
            index: 0,
            text_length: 0,
        };
        let setup = SeedSetup {
            code_reward_per_key: if self.model.is_some() {
                self.ranking_prior.canonical_code_reward
            } else {
                0.0
            },
            protect_primary_rare: self.ranking_prior.canonical_isolation_factor < 1.0,
        };
        for boundary in parse_boundaries(lock.boundaries) {
            self.seed_boundary(raw, lock, boundary, &mut cursor, &setup)?;
        }
        let accepted = {
            let seed = &self.arena[cursor.index];
            seed.raw_length == prefix.len() && seed.text.as_str() == lock.text
        };
        if !accepted {
            return Ok(false);
        }
        states[0] = Bucket::default();
        let bucket = &mut states[prefix.len()];
        bucket.items.push(cursor.index);
        if bucket.items.len() >= AGGREGATE_DURING_EXPANSION_THRESHOLD {
            self.ensure_aggregated(bucket);
        }
        Ok(true)
    }

    /// 播种一个已确认边界：反解边、重放字符累加，并把新种子压入 arena。
    fn seed_boundary(
        &mut self,
        raw: &[u8],
        lock: &DecodeLock<'_>,
        boundary: (usize, usize),
        cursor: &mut SeedCursor,
        setup: &SeedSetup,
    ) -> Result<()> {
        let (raw_length, text_length) = boundary;
        let (edge_text, text) = locked_texts(lock, cursor.text_length, text_length);
        let seed = SeedSnapshot::read(&self.arena[cursor.index], cursor.index);
        // 参照 `resolve_locked_edge`：反解该已确认边，恢复码形证据与保护元数据。
        let resolved = self.resolve_locked_edge(raw, seed.raw_length, raw_length, edge_text);
        // 文本级退格可能缩短已确认多字边而保留 raw 边界（如 团圆/cd → 团/cd）：
        // 此类旧锁以中立码证据重放，不再整段拒绝（参照 12d2ecc 修复）。
        let edge = locked_edge(
            &resolved,
            edge_text,
            setup.protect_primary_rare,
            setup.code_reward_per_key,
        );
        let accum = self.advance_chars(&edge.chars, seed.advance())?;
        let parts = SeedParts {
            raw_length,
            text_length,
            text,
            edge,
            accum,
        };
        let state = self.seed_state(raw, &seed, &parts);
        cursor.index = self.arena.len();
        self.arena.push(state);
        cursor.text_length = text_length;
        Ok(())
    }

    /// 装配一个已确认边的种子状态（码形证据、补充分、mass 与学习奖励）。
    fn seed_state(&mut self, raw: &[u8], seed: &SeedSnapshot, parts: &SeedParts) -> State {
        // 码形证据：与普通扩展一致地累计（只进排序分，不进 mass）。
        let mut code_score = seed.code_score;
        if let Some(reward) = parts.edge.code_reward {
            code_score += reward;
        }
        let supplement_score = seed.supplement_score + parts.accum.supplement_added;
        let mass_score = parts.accum.score - supplement_score - seed.learning_score;
        let (learned, potential, learning_early_bonus) = self.edge_learning(
            raw,
            &parts.text,
            parts.raw_length,
            seed.index,
            seed.learning_score,
        );
        State {
            score: parts.accum.score + learned - seed.learning_score,
            mass_score,
            code_score,
            text: parts.text.clone(),
            prev2: parts.accum.prev2,
            prev1: parts.accum.prev1,
            max_rank: 1,
            supplement_state: parts.accum.supplement_state,
            supplement_score,
            previous: Some(seed.index),
            edge_chars: parts.edge.chars.clone(),
            text_length: parts.text_length,
            raw_length: parts.raw_length,
            edge_count: seed.edge_count + 1,
            learning_score: learned,
            learning_potential: potential,
            learning_early_commit_bonus: learning_early_bonus,
            // 参照锁定重放的种子表没有 `source_mask`/`direct_rank` 字段：
            // 既不 Direct 也不 Composed-only，合并时让位给另一个来源。
            source_mask: SOURCE_UNSET,
            direct_rank: f64::INFINITY,
            edge_primary_single: parts.edge.primary_single,
            edge_code_length: parts.edge.code_length,
            ..State::neutral()
        }
    }
}
