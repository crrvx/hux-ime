// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! Beam 解码（冷路径），对应参照 `decode_full` / `decode` 的去缓存形态。
//!
//! 覆盖：normalize、rank 选择器、资格过滤、beam 扩展、桶聚合、评分与候选发射、
//! 早提交证据、学习集成、锁播种（`decode_with_lock`）。
//! 暂不含：增量/锁缓存（性能优化）、模型失败回退（guarded_decode）。
//! 参照 `decode()` 的缓存机制（`trailing_selector_span` / `expand_range` 的增量状态复用、
//! `locked_decode_cache`）**当前不做**：收益集中在 >20 字符的长整句，而缓存需与解码 arena 的
//! 路径下标生命周期绑定（改动语义边界），不符合「金样不变 + 按需」的前提。

use crate::lexical::{self, LexicalModel};
use crate::lexicon::{CodeEntry, Lexicon, Supplement};
use crate::ngram::MobileModel;
use anyhow::Result;
use hashbrown::{HashMap, HashSet};
use hux_core::collections::{Map, Set};
use hux_core::learning::{DiffItem, DiffPathNode, LearningIndex};
use hux_core::punct::{PairState, PunctTable};
use hux_core::session::Candidate;
use std::path::PathBuf;

pub const BOS: char = '\u{2}';
pub const EOS: char = '\u{3}';

const BEAM_WIDTH: usize = 200;
const LONG_INPUT_FULL_BEAM_LENGTH: usize = 24;
const LONG_INPUT_BEAM_WIDTH: usize = 48;
/// 候选上限（参照 `candidate_limit`）：**唯一来源**，交互层与音反查层引用它。
pub const CANDIDATE_LIMIT: usize = 20;
const RANK_PENALTY: f64 = 0.03;
const EMITTED_CHARACTER_REWARD: f64 = 2.0;
const WHOLE_INPUT_SINGLE_CHARACTER_REWARD: f64 = 5.0;
const ISOLATION_THRESHOLD: usize = 3000;
const ISOLATION_LAMBDA: f64 = 2.0;
const AGGREGATE_DURING_EXPANSION_THRESHOLD: usize = 128;
/// 早提交最低份额（参照 `early_commit_minimum_share`）：**唯一来源**，
/// 交互层（`interaction::early_commit`）引用它。
pub(crate) const EARLY_COMMIT_MINIMUM_SHARE: f64 = 0.99;
const EARLY_COMMIT_CLOSED_BOUNDARY_SHARE: f64 = 0.99999;

/// 来源标记（参照 `learning.source_direct` / `learning.source_composed`）。
const SOURCE_DIRECT: u8 = 1;
const SOURCE_COMPOSED: u8 = 2;
/// Direct 与 Composed 皆有的聚合标记（参照 `source_union` 的冲突结果）。
const SOURCE_BOTH: u8 = 3;
/// 未标记来源（参照状态表缺 `source_mask` 字段：锁定重放的种子）。
const SOURCE_UNSET: u8 = 0;

/// 参照 `learning.source_union`：合并两条路径的来源标记。
fn source_union(left: u8, right: u8) -> u8 {
    match (left, right) {
        (SOURCE_UNSET, other) => other,
        (value, SOURCE_UNSET) => value,
        (left, right) if left == right => left,
        _ => SOURCE_BOTH,
    }
}

/// 参照 `learning.candidate_is_direct`：`source_mask ∈ {1,3}`。
pub(crate) fn candidate_is_direct(source_mask: u8) -> bool {
    source_mask == SOURCE_DIRECT || source_mask == SOURCE_BOTH
}

/// 参照 `learning.candidate_is_composed_only`：`source_mask == 2`。
pub(crate) fn candidate_is_composed_only(source_mask: u8) -> bool {
    source_mask == SOURCE_COMPOSED
}

/// 排序先验参数（对应参照 `ranking_prior` 表；参照经
/// `M.set_decoder_parameters_for_test` 调整这些值做消融）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RankingPriorParameters {
    /// 逐字主码奖励：`canonical_code_reward × 码长`（仅未显式选重的单字主码边）。
    pub canonical_code_reward: f64,
    /// 紧凑词先验权重（只重排 Top-`lexical_candidate_limit`）。
    pub lexical_prior_weight: f64,
    pub lexical_candidate_limit: usize,
    /// 生僻字保护系数（`< 1.0` 时启用；4 码及以上单字边免罚）。
    pub canonical_isolation_factor: f64,
    pub canonical_isolation_min_code_length: usize,
    /// 补充码表先验转早提交置信度的斜率（`supplement_early_commit_scale`）。
    pub supplement_early_commit_scale: f64,
    /// 该转换的上限（`supplement_early_commit_cap`）。
    pub supplement_early_commit_cap: f64,
    /// 个性化早提交置信度的总上限（`personalized_early_commit_cap`）。
    pub personalized_early_commit_cap: f64,
    /// 空码自动上屏的强置信阈值（`empty_code_strong_share`；比普通强阈值更严）。
    pub empty_code_strong_share: f64,
}

impl Default for RankingPriorParameters {
    fn default() -> Self {
        Self {
            canonical_code_reward: 2.0,
            lexical_prior_weight: 0.1,
            lexical_candidate_limit: 5,
            canonical_isolation_factor: 0.0,
            canonical_isolation_min_code_length: 4,
            supplement_early_commit_scale: 0.05,
            supplement_early_commit_cap: 0.75,
            personalized_early_commit_cap: 0.80,
            empty_code_strong_share: 0.99999,
        }
    }
}

impl RankingPriorParameters {
    /// 参照 `ranking_prior.supplement_early_commit_contribution`。
    pub fn supplement_early_commit_contribution(&self, score: f64) -> f64 {
        self.supplement_early_commit_cap
            .min(score.max(0.0) * self.supplement_early_commit_scale)
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Comparator {
    RankFirst,
    ScoreFirst,
    NoModel,
}

struct State {
    score: f64,
    mass_score: f64,
    /// 码形证据分（只用于最终排序，不进入 mass/置信度；参照 `code_score`）。
    code_score: f64,
    text: String,
    prev2: char,
    prev1: char,
    max_rank: usize,
    supplement_state: usize,
    supplement_score: f64,
    previous: Option<usize>,
    edge_chars: Vec<char>,
    /// 累计文本字节长度（学习奖励与路径摘要使用）。
    text_length: usize,
    raw_length: usize,
    edge_count: usize,
    learning_score: f64,
    learning_potential: f64,
    /// 该边的学习奖励对早提交置信度的贡献（参照 `learning_early_commit_bonus`）。
    learning_early_commit_bonus: f64,
    /// 来源标记（参照 `source_mask`）：0 = 未标记（锁定的重放种子）、
    /// [`SOURCE_DIRECT`] = Direct（整串直出边）、[`SOURCE_COMPOSED`] = Composed、3 = 两者皆有。
    source_mask: u8,
    /// Direct 来源的原始菜单排名（参照 `direct_rank`；非 Direct 为 `f64::INFINITY`）。
    direct_rank: f64,
    isolation_penalty: Option<f64>,
    isolation_last_char: Option<char>,
    /// 最近被隔离字符的权重（`canonical_isolation_factor`；0 表示未隔离）。
    isolation_last_weight: f64,
    /// 该边是否为主码单字边（4 码生僻字保护用）。
    edge_primary_single: bool,
    /// 该边的码长（仅保护边记录；4 码生僻字保护用）。
    edge_code_length: Option<usize>,
}

#[derive(Default)]
struct Bucket {
    items: Vec<usize>,
    aggregated: bool,
    best: HashMap<String, usize>,
    mass: HashMap<String, f64>,
    order: Vec<String>,
    frozen: bool,
    truncated: bool,
}

/// 一个已评分候选（对应 `evaluate_state` 的返回值）。
#[derive(Clone, Debug)]
pub struct Evaluated {
    pub text: String,
    pub score: f64,
    pub confidence_score: f64,
    /// 早提交置信度（`confidence_score` + 补充码表/学习的有界个性化加分）。
    pub early_commit_confidence_score: f64,
    /// 码形证据分（参照 `item.code_score`；只参与排序比较）。
    pub code_score: f64,
    pub max_rank: usize,
    pub supplement_score: f64,
    pub learning_score: f64,
    pub edge_count: usize,
    /// 来源标记（参照 `source_mask`）：Direct / Composed / 两者皆有 / 未标记（0）。
    pub source_mask: u8,
    /// Direct 来源的原始菜单排名（参照 `direct_rank`；非 Direct 为 `f64::INFINITY`）。
    pub direct_rank: f64,
    /// 本次解码 arena 的路径下标；仅对产生它的那次 `decode*` 返回值有效。
    pub path: usize,
    pub segmented: String,
    /// 路径末段的 previous 节点信息（供交互层判定隐式选重）。
    pub previous_raw_length: usize,
    pub previous_text: Option<String>,
}

#[derive(Debug)]
pub struct DecodeOutput {
    pub items: Vec<Evaluated>,
    /// 全量已评分候选（未截断的 beam 输出，对应参照 `_confidence_candidates`）。
    pub confidence_candidates: Vec<Evaluated>,
    pub evidence: Evidence,
    /// 可见顶层候选路径上的全部 (raw_length, text) 前缀（对应参照
    /// `prefix_belongs_to_visible` 的 membership 判定）。
    pub visible_prefixes: Set<(usize, String)>,
    pub learning_affected: bool,
    pub completed_truncated: bool,
}

impl DecodeOutput {
    fn empty() -> Self {
        Self {
            items: Vec::new(),
            confidence_candidates: Vec::new(),
            evidence: Evidence::default_for(false),
            visible_prefixes: Set::new(),
            learning_affected: false,
            completed_truncated: false,
        }
    }
}

/// 交互层锁（对应参照 `{ raw, text, boundaries }`；`boundaries` 形如 `"2,3;4,6;"`）。
#[derive(Clone, Copy, Debug)]
pub struct DecodeLock<'a> {
    pub raw: &'a str,
    pub text: &'a str,
    pub boundaries: &'a str,
}

/// 单条前缀证据（对应参照 `build_prefix_evidence` 的顺序数组元素）。
#[derive(Clone, Debug)]
pub struct PrefixEvidence {
    pub text: String,
    pub raw_length: usize,
    /// 早提交份额（个性化置信度口径；`entry.share`）。
    pub share: f64,
    /// 纯模型（基础置信度）份额（`entry.base_share`；截断池下强证据判定用它）。
    pub base_share: f64,
    pub boundary_share: f64,
    pub boundary_closed: bool,
    pub text_char_count: usize,
}

/// 早提交证据（对应参照 `early_commit_evidence`）。
#[derive(Clone, Debug)]
pub struct Evidence {
    pub prefixes: Vec<PrefixEvidence>,
    /// raw_length → text → `prefixes` 下标（对应参照 `_by_boundary`）。
    pub by_boundary: Map<usize, Map<String, usize>>,
    pub proposal: String,
    pub proposal_share: f64,
    pub raw_lengths: Map<String, usize>,
    pub neutral_incomplete_tail: bool,
    pub merged_incomplete_tail: bool,
    pub neutral_low_confidence: bool,
    pub confidence_truncated: bool,
}

impl Evidence {
    fn default_for(truncated: bool) -> Self {
        Self {
            prefixes: Vec::new(),
            by_boundary: Map::new(),
            proposal: String::new(),
            proposal_share: 0.0,
            raw_lengths: Map::new(),
            neutral_incomplete_tail: false,
            merged_incomplete_tail: false,
            neutral_low_confidence: false,
            confidence_truncated: truncated,
        }
    }

    /// 参照 `find_prefix_evidence`。
    pub fn find(&self, text: &str, raw_length: usize) -> Option<&PrefixEvidence> {
        self.by_boundary
            .get(&raw_length)
            .and_then(|boundary| boundary.get(text))
            .map(|&index| &self.prefixes[index])
    }
}

/// 证据池条目（对应参照 `pool` 中的候选，含碰撞合并后的置信度）。
struct EvidenceCandidate {
    text: String,
    confidence_score: f64,
    /// 合并/新建时冻结的早提交置信度（参照 `early_confidence(entry)`）。
    early_commit_confidence_score: f64,
    path: usize,
}

impl EvidenceCandidate {
    /// 参照 `ranking_prior.early_confidence`。
    fn early_confidence(&self) -> f64 {
        self.early_commit_confidence_score
    }
}

/// 解码器：持有数据与可选 n-gram 模型（`None` = 无模型回退）。
pub struct Decoder {
    lexicon: Lexicon,
    supplement: Supplement,
    model: Option<MobileModel>,
    learning: Option<LearningWiring>,
    rank_of: Option<HashMap<char, usize>>,
    arena: Vec<State>,
    allow_duplicate_single: bool,
    learning_affected: bool,
    ranking_prior: RankingPriorParameters,
    lexical: Option<LexicalModel>,
    lexical_load_error: Option<String>,
    /// 音反查索引（懒加载；缺文件时为 `None`）。
    pinyin: Option<crate::sound_to_char_shape::SoundToCharShapeIndex>,
    pinyin_checked: bool,
    pinyin_load_error: Option<String>,
}

/// 学习接线：索引 + 模式串（参照的 `learning_index`/`learning_mode`）。
struct LearningWiring {
    index: LearningIndex,
    mode: String,
}

impl Decoder {
    pub fn new(lexicon: Lexicon, supplement: Supplement, model: Option<MobileModel>) -> Self {
        let rank_of = lexicon.character_ranks.as_ref().map(|ranks| {
            ranks
                .iter()
                .filter_map(|(text, rank)| text.chars().next().map(|ch| (ch, *rank)))
                .collect()
        });
        // 参照模块初始化：从数据目录加载紧凑词先验（缺省关闭）。
        let (lexical, lexical_load_error) = {
            let paths: Vec<PathBuf> = lexicon
                .dirs()
                .iter()
                .map(|directory| directory.join("tiger_sentence.lexical.bin"))
                .collect();
            lexical::load_first(&paths)
        };
        Self {
            lexicon,
            supplement,
            model,
            learning: None,
            rank_of,
            arena: Vec::new(),
            allow_duplicate_single: true,
            learning_affected: false,
            ranking_prior: RankingPriorParameters::default(),
            lexical,
            lexical_load_error,
            pinyin: None,
            pinyin_checked: false,
            pinyin_load_error: None,
        }
    }

    /// 音反查索引（首次访问时按数据目录懒加载）。
    pub fn pinyin_index(&mut self) -> Option<&crate::sound_to_char_shape::SoundToCharShapeIndex> {
        if !self.pinyin_checked {
            self.pinyin_checked = true;
            let (index, error) = crate::sound_to_char_shape::load_first(self.lexicon.dirs());
            self.pinyin = index;
            self.pinyin_load_error = error;
        }
        self.pinyin.as_ref()
    }

    /// 音反查索引载入错误（有文件但无效时记录）。
    pub fn pinyin_load_error(&self) -> Option<&str> {
        self.pinyin_load_error.as_deref()
    }

    /// 字反查两排提示（上排 = 光标左侧拼音、下排 = 虎码；懒加载索引，缺索引返回 `None`）。
    pub fn char_to_sound_shape_rows(
        &mut self,
        text: &str,
        anchor: usize,
    ) -> Option<(String, String)> {
        self.pinyin_index();
        let index = self.pinyin.as_ref()?;
        Some(crate::char_to_sound_shape::rows(
            index,
            &self.lexicon,
            text,
            anchor,
        ))
    }

    /// 音反查候选（含虎码注释过滤；上限 [`crate::sound_to_char_shape::CANDIDATE_LIMIT`]）。
    ///
    /// 参数较多是因为要透传「索引 / 区间 / 标点表 / 会话态 / 形状」——与
    /// [`crate::sound_to_char_shape::translate`] 同源，故与同文件既有先例一致地豁免。
    #[allow(clippy::too_many_arguments)]
    pub fn sound_to_char_shape_candidates(
        &mut self,
        input: &[u8],
        prefix: char,
        start: usize,
        end: usize,
        punct: Option<&PunctTable>,
        pairs: &mut PairState,
        full_shape: bool,
    ) -> Vec<Candidate> {
        self.pinyin_index();
        let Some(index) = self.pinyin.as_ref() else {
            return Vec::new();
        };
        crate::sound_to_char_shape::translate(
            index,
            &self.lexicon,
            input,
            prefix,
            start,
            end,
            punct,
            pairs,
            full_shape,
            crate::sound_to_char_shape::CANDIDATE_LIMIT,
        )
    }

    pub fn lexicon(&self) -> &Lexicon {
        &self.lexicon
    }

    /// 应用高频字过滤上限：重建词库索引（参照 `M.apply_high_freq_limit`）。
    ///
    /// 解码器不缓存词库派生的查询表（`rank_of` 只依赖字频文件，与上限无关），
    /// 故重建后无需其他失效动作。
    pub fn apply_high_freq_limit(&mut self, limit: usize) {
        self.lexicon.apply_high_freq_limit(limit);
    }

    pub fn model(&self) -> Option<&MobileModel> {
        self.model.as_ref()
    }

    pub fn set_allow_duplicate_single(&mut self, allowed: bool) {
        self.allow_duplicate_single = allowed;
    }

    /// 参照 `M.set_learning_for_test`：接入学习索引与模式串。
    pub fn set_learning(&mut self, index: LearningIndex, mode: &str) {
        self.learning = Some(LearningWiring {
            index,
            mode: mode.to_string(),
        });
        self.learning_affected = false;
    }

    /// 参照 `M.decoder_parameters`（排序先验部分）。
    pub fn ranking_prior_parameters(&self) -> RankingPriorParameters {
        self.ranking_prior
    }

    /// 参照 `M.set_decoder_parameters_for_test`（排序先验部分）。
    pub fn set_ranking_prior_parameters(&mut self, parameters: RankingPriorParameters) {
        self.ranking_prior = parameters;
    }

    /// 参照 `lexicon_state.lexical_model`：紧凑词先验模型（缺省关闭）。
    pub fn lexical_model(&self) -> Option<&LexicalModel> {
        self.lexical.as_ref()
    }

    pub fn set_lexical_model(&mut self, model: Option<LexicalModel>) {
        self.lexical = model;
    }

    /// 参照 `ranking_prior.lexical_load_error`（有文件但无效时记录）。
    pub fn lexical_load_error(&self) -> Option<&str> {
        self.lexical_load_error.as_deref()
    }

    /// 参照 `item.path`：返回路径末节点 raw 长度与 `learning.diff` 所需路径
    /// （`DiffItem.path[0]` 为最外层非根节点）。
    /// 仅对最近一次 `decode*` 返回的项有效（arena 每次解码重建）。
    pub fn path_summary(&self, item: &Evaluated) -> (usize, DiffItem) {
        let raw_length = self.arena[item.path].raw_length;
        let mut nodes = Vec::new();
        let mut current = Some(item.path);
        while let Some(index) = current {
            let node = &self.arena[index];
            if node.raw_length > 0 {
                nodes.push(DiffPathNode {
                    raw_length: node.raw_length,
                    text_length: node.text_length,
                });
            }
            current = node.previous;
        }
        nodes.reverse();
        (
            raw_length,
            DiffItem {
                text: item.text.clone(),
                path: nodes,
            },
        )
    }

    /// 参照 `decode(raw_code, false, nil, nil)` 的冷路径。
    pub fn decode(&mut self, raw_code: &str) -> Result<DecodeOutput> {
        self.decode_with(raw_code, false, "")
    }

    /// 参照 `decode(raw_code, include_early_commit, required_text_prefix, nil)` 的冷路径。
    pub fn decode_with(
        &mut self,
        raw_code: &str,
        include_early_commit: bool,
        required_text_prefix: &str,
    ) -> Result<DecodeOutput> {
        self.decode_with_lock(raw_code, include_early_commit, required_text_prefix, None)
    }

    /// 参照 `decode(raw_code, include_early_commit, required_text_prefix, locked)` 的冷路径。
    pub fn decode_with_lock(
        &mut self,
        raw_code: &str,
        include_early_commit: bool,
        required_text_prefix: &str,
        lock: Option<DecodeLock<'_>>,
    ) -> Result<DecodeOutput> {
        let raw = normalize(raw_code);
        if let Some(lock) = lock {
            let prefix = normalize(lock.raw);
            if prefix.is_empty() || !raw.starts_with(&prefix) {
                return Ok(DecodeOutput::empty());
            }
            self.arena.clear();
            self.learning_affected = false;
            let length = raw.len();
            let mut states = self.new_states(length);
            if !self.seed_locked(&raw, &mut states, &prefix, &lock)? {
                return Ok(DecodeOutput::empty());
            }
            self.expand_range(&raw, &mut states, prefix.len(), length, -1)?;
            return self.emit(
                &raw,
                &mut states,
                length,
                include_early_commit,
                required_text_prefix,
            );
        }
        if raw.is_empty() || !has_letter(&raw) {
            return Ok(DecodeOutput::empty());
        }
        self.arena.clear();
        self.learning_affected = false;
        let length = raw.len();
        let mut states = self.new_states(length);
        self.expand_range(&raw, &mut states, 0, length, -1)?;
        self.emit(
            &raw,
            &mut states,
            length,
            include_early_commit,
            required_text_prefix,
        )
    }

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

    fn seed_locked(
        &mut self,
        raw: &[u8],
        states: &mut [Bucket],
        prefix: &[u8],
        lock: &DecodeLock<'_>,
    ) -> Result<bool> {
        let mut seed_index = 0usize;
        let mut seed_text_length = 0usize;
        let code_reward_per_key = if self.model.is_some() {
            self.ranking_prior.canonical_code_reward
        } else {
            0.0
        };
        let protect_primary_rare = self.ranking_prior.canonical_isolation_factor < 1.0;
        for (raw_length, text_length) in parse_boundaries(lock.boundaries) {
            // 参照 `sub` 会把越界端点截到串尾；先夹取再按字节取。
            let text_end = text_length.min(lock.text.len());
            let edge_text = lock.text.get(seed_text_length..text_end).unwrap_or("");
            let seed = &self.arena[seed_index];
            let seed_score = seed.score;
            let seed_raw_length = seed.raw_length;
            let seed_code_score = seed.code_score;
            let seed_prev2 = seed.prev2;
            let seed_prev1 = seed.prev1;
            let seed_supplement_state = seed.supplement_state;
            let seed_supplement_score = seed.supplement_score;
            let seed_learning_score = seed.learning_score;
            let seed_edge_count = seed.edge_count;
            // 参照 `resolve_locked_edge`：反解该已确认边，恢复码形证据与保护元数据。
            let resolved = self.resolve_locked_edge(raw, seed_raw_length, raw_length, edge_text);
            // 文本级退格可能缩短已确认多字边而保留 raw 边界（如 团圆/cd → 团/cd）：
            // 此类旧锁以中立码证据重放，不再整段拒绝（参照 12d2ecc 修复）。
            let chars: Vec<char> = match &resolved {
                Some((chars, _, _, _)) => chars.clone(),
                None => edge_text.chars().collect(),
            };
            let (edge_primary_single, edge_code_length) = match &resolved {
                Some((_, primary_single, code_length, selected_rank)) => (
                    protect_primary_rare
                        && chars.len() == 1
                        && (*primary_single || *selected_rank > 0),
                    protect_primary_rare.then_some(*code_length),
                ),
                None => (false, None),
            };
            let mut score = seed_score;
            let mut prev2 = seed_prev2;
            let mut prev1 = seed_prev1;
            let mut supplement_state = seed_supplement_state;
            let mut supplement_added = 0.0;
            for &ch in &chars {
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
            // 码形证据：与普通扩展一致地累计（只进排序分，不进 mass）。
            let mut code_score = seed_code_score;
            if let Some((_, primary_single, code_length, selected_rank)) = &resolved
                && code_reward_per_key > 0.0
                && *selected_rank == 0
                && *primary_single
                && chars.len() == 1
            {
                code_score += code_reward_per_key * *code_length as f64;
            }
            let text = lock
                .text
                .get(..text_length)
                .unwrap_or(lock.text)
                .to_string();
            let supplement_score = seed_supplement_score + supplement_added;
            let mass_score = score - supplement_score - seed_learning_score;
            let (learned, potential, learning_early_bonus) = match &mut self.learning {
                Some(wiring) => learning_reward(
                    &mut wiring.index,
                    &wiring.mode,
                    &self.arena,
                    raw,
                    &text,
                    raw_length,
                    seed_index,
                ),
                None => (seed_learning_score, 0.0, 0.0),
            };
            if learned > 0.0 || potential > 0.0 {
                self.learning_affected = true;
            }
            let state = State {
                score: score + learned - seed_learning_score,
                mass_score,
                code_score,
                text,
                prev2,
                prev1,
                max_rank: 1,
                supplement_state,
                supplement_score,
                previous: Some(seed_index),
                edge_chars: chars,
                text_length,
                raw_length,
                edge_count: seed_edge_count + 1,
                learning_score: learned,
                learning_potential: potential,
                learning_early_commit_bonus: learning_early_bonus,
                // 参照锁定重放的种子表没有 `source_mask`/`direct_rank` 字段：
                // 既不 Direct 也不 Composed-only，合并时让位给另一个来源。
                source_mask: SOURCE_UNSET,
                direct_rank: f64::INFINITY,
                edge_primary_single,
                edge_code_length,
                isolation_penalty: None,
                isolation_last_char: None,
                isolation_last_weight: 0.0,
            };
            seed_index = self.arena.len();
            self.arena.push(state);
            seed_text_length = text_length;
        }
        let accepted = {
            let seed = &self.arena[seed_index];
            seed.raw_length == prefix.len() && seed.text.as_str() == lock.text
        };
        if !accepted {
            return Ok(false);
        }
        states[0] = Bucket::default();
        let bucket = &mut states[prefix.len()];
        bucket.items.push(seed_index);
        if bucket.items.len() >= AGGREGATE_DURING_EXPANSION_THRESHOLD {
            self.ensure_aggregated(bucket);
        }
        Ok(true)
    }

    fn new_states(&mut self, length: usize) -> Vec<Bucket> {
        let mut states: Vec<Bucket> = (0..=length).map(|_| Bucket::default()).collect();
        let root = State {
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
        };
        self.add_state(&mut states[0], root);
        states
    }

    fn logp(&mut self, prev2: char, prev1: char, target: char) -> Result<f64> {
        let Some(model) = self.model.as_mut() else {
            return Ok(0.0);
        };
        model.logp_codes(prev2 as u32, prev1 as u32, target as u32)
    }

    fn has_observed_bigram(&mut self, prev: char, target: char) -> Result<bool> {
        let Some(model) = self.model.as_mut() else {
            return Ok(false);
        };
        model.has_observed_bigram_codes(prev as u32, target as u32)
    }

    fn current_comparator(&self) -> Comparator {
        if self.model.is_none() {
            Comparator::NoModel
        } else if self.allow_duplicate_single {
            Comparator::ScoreFirst
        } else {
            Comparator::RankFirst
        }
    }

    fn state_better(cmp: Comparator, left: &Evaluated, right: &Evaluated) -> bool {
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

    fn state_better_raw(cmp: Comparator, left: &State, right: &State) -> bool {
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

    fn duplicate_better(&self, item: usize, previous: usize) -> bool {
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

    fn add_state(&mut self, bucket: &mut Bucket, state: State) {
        if bucket.frozen {
            bucket.frozen = false;
            self.ensure_aggregated(bucket);
        }
        let index = self.arena.len();
        self.arena.push(state);
        if bucket.aggregated {
            self.add_aggregated(bucket, index);
            return;
        }
        bucket.items.push(index);
        if bucket.items.len() >= AGGREGATE_DURING_EXPANSION_THRESHOLD {
            self.ensure_aggregated(bucket);
        }
    }

    fn add_aggregated(&mut self, bucket: &mut Bucket, item: usize) {
        let text = self.arena[item].text.clone();
        let item_mass = self.arena[item].mass_score;
        match bucket.best.get(&text).copied() {
            None => {
                bucket.best.insert(text.clone(), item);
                bucket.mass.insert(text.clone(), item_mass);
                bucket.order.push(text.clone());
            }
            Some(previous) => {
                let mass = bucket.mass.get(&text).copied().unwrap_or(item_mass);
                let combined = logsumexp(mass, item_mass);
                bucket.mass.insert(text.clone(), combined);
                // 同文本多路径的来源合并先算，再写入「当前最佳项」（参照 `add_aggregated`）。
                let source = source_union(
                    self.arena[previous].source_mask,
                    self.arena[item].source_mask,
                );
                let direct_rank = self.arena[previous]
                    .direct_rank
                    .min(self.arena[item].direct_rank);
                if self.duplicate_better(item, previous) {
                    bucket.best.insert(text.clone(), item);
                }
                if let Some(&best) = bucket.best.get(&text) {
                    self.arena[best].source_mask = source;
                    self.arena[best].direct_rank = direct_rank;
                }
            }
        }
        if let Some(&best) = bucket.best.get(&text) {
            let mass = bucket.mass.get(&text).copied().unwrap_or(item_mass);
            self.arena[best].mass_score = mass;
        }
    }

    fn ensure_aggregated(&mut self, bucket: &mut Bucket) {
        if bucket.aggregated {
            return;
        }
        let items = std::mem::take(&mut bucket.items);
        for item in items {
            self.add_aggregated(bucket, item);
        }
        bucket.aggregated = true;
    }

    fn dedup_limit(&mut self, mut bucket: Bucket, limit: usize) -> Bucket {
        if bucket.frozen {
            return bucket;
        }
        self.ensure_aggregated(&mut bucket);
        let mut result: Vec<usize> = bucket
            .order
            .iter()
            .filter_map(|text| bucket.best.get(text).copied())
            .collect();
        let truncated_now = result.len() > limit;
        let truncated = bucket.truncated || truncated_now;
        let mut comparator = self.current_comparator();
        if result
            .iter()
            .any(|&index| self.arena[index].learning_score > 0.0)
        {
            comparator = Comparator::ScoreFirst;
        }
        if truncated_now {
            let mut reserved: Vec<usize> = result
                .iter()
                .copied()
                .filter(|&index| self.arena[index].learning_potential > 0.0)
                .collect();
            reserved.sort_by(|&left, &right| {
                let left_key = self.arena[left].score + self.arena[left].learning_potential;
                let right_key = self.arena[right].score + self.arena[right].learning_potential;
                right_key
                    .partial_cmp(&left_key)
                    .expect("learning scores are finite")
            });
            result = select_top(&self.arena, result, limit, comparator);
            let kept: HashSet<usize> = result.iter().copied().collect();
            let mut added = 0usize;
            for index in reserved {
                if added == 4 {
                    break;
                }
                if !kept.contains(&index) {
                    result.push(index);
                    added += 1;
                }
            }
        } else {
            result.sort_by(|&left, &right| {
                if Decoder::state_better_raw(comparator, &self.arena[left], &self.arena[right]) {
                    std::cmp::Ordering::Less
                } else {
                    std::cmp::Ordering::Greater
                }
            });
        }
        Bucket {
            items: result,
            truncated,
            frozen: true,
            ..Bucket::default()
        }
    }

    fn expand_range(
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
        for position in from_pos..length {
            let limit = beam_limit_at(position);
            states[position] = self.dedup_limit(std::mem::take(&mut states[position]), limit);
            if states[position].items.is_empty() {
                continue;
            }
            let current: Vec<usize> = states[position].items.clone();
            let current_truncated = states[position].truncated;
            for &code_length in &lengths {
                if position + code_length > length {
                    continue;
                }
                let code_bytes = &raw[position..position + code_length];
                let Ok(code) = std::str::from_utf8(code_bytes) else {
                    continue;
                };
                let Some(candidates) = self.lexicon.codes.get(code) else {
                    continue;
                };
                let (selected_rank, consumed_end) = parse_selector(raw, position + code_length);
                let whole_input_edge = position == 0 && consumed_end == length;
                if (consumed_end as isize) <= minimum_consumed_end
                    || (length > 1 && consumed_end - position < 2)
                {
                    continue;
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
                    continue;
                }
                if current_truncated {
                    states[consumed_end].truncated = true;
                }
                for &item_index in &current {
                    let item_is_root = self.arena[item_index].previous.is_none();
                    let item = self.arena[item_index].view();
                    for candidate in &eligible {
                        let mut score = item.score;
                        let mut prev2 = item.prev2;
                        let mut prev1 = item.prev1;
                        let mut supplement_state = item.supplement_state;
                        let mut supplement_added = 0.0;
                        for &ch in &candidate.chars {
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
                        if selected_rank == 0 {
                            score -= RANK_PENALTY * candidate.log_rank;
                        }
                        // 主码单字边：按覆盖的原始键数累计码形证据（不入 beam 分）。
                        let mut code_reward_added = 0.0;
                        if code_reward_per_key > 0.0
                            && selected_rank == 0
                            && candidate.primary_single
                            && candidate.chars.len() == 1
                        {
                            code_reward_added = code_reward_per_key * code_length as f64;
                        }
                        let mut whole_input_bonus = 0.0;
                        if whole_input_edge
                            && selected_rank == 0
                            && candidate.optimal_single
                            && candidate.is_single
                        {
                            whole_input_bonus = WHOLE_INPUT_SINGLE_CHARACTER_REWARD;
                            score += whole_input_bonus;
                        }
                        let text = item.text.clone() + &candidate.text;
                        let mass_score = item.mass_score + score
                            - item.score
                            - supplement_added
                            - whole_input_bonus;
                        // 整串直出边（Direct）：不参与学习（保留路径既有学习分，
                        // 不另计奖励，也不置 `learning_affected`）；参照 `expand_range`。
                        let direct_edge = item_is_root && position == 0 && whole_input_edge;
                        let (learned, potential, learning_early_bonus) = if direct_edge {
                            (item.learning_score, 0.0, item.learning_early_commit_bonus)
                        } else {
                            match &mut self.learning {
                                Some(wiring) => learning_reward(
                                    &mut wiring.index,
                                    &wiring.mode,
                                    &self.arena,
                                    raw,
                                    &text,
                                    consumed_end,
                                    item_index,
                                ),
                                None => (item.learning_score, 0.0, 0.0),
                            }
                        };
                        if learned > 0.0 || potential > 0.0 {
                            self.learning_affected = true;
                        }
                        let state = State {
                            score: score + learned - item.learning_score,
                            mass_score,
                            code_score: item.code_score + code_reward_added,
                            text_length: text.len(),
                            text,
                            prev2,
                            prev1,
                            max_rank: item.max_rank.max(candidate.rank),
                            supplement_state,
                            supplement_score: item.supplement_score + supplement_added,
                            previous: Some(item_index),
                            edge_chars: candidate.chars.clone(),
                            raw_length: consumed_end,
                            edge_count: item.edge_count + 1,
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
                            edge_primary_single: protect_primary_rare
                                && candidate.chars.len() == 1
                                && (candidate.primary_single || selected_rank > 0),
                            edge_code_length: protect_primary_rare.then_some(code_length),
                            isolation_penalty: None,
                            isolation_last_char: None,
                            isolation_last_weight: 0.0,
                        };
                        self.add_state(&mut states[consumed_end], state);
                    }
                }
            }
        }
        Ok(())
    }

    fn evaluate_state(&mut self, index: usize) -> Result<Evaluated> {
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

    fn rank_of_char(&self, ch: char) -> usize {
        self.rank_of
            .as_ref()
            .and_then(|ranks| ranks.get(&ch).copied())
            .unwrap_or(self.lexicon.unknown_character_rank)
    }

    fn emit(
        &mut self,
        raw: &[u8],
        states: &mut [Bucket],
        length: usize,
        include_early_commit: bool,
        required_text_prefix: &str,
    ) -> Result<DecodeOutput> {
        let completed =
            self.dedup_limit(std::mem::take(&mut states[length]), beam_limit_at(length));
        states[length] = completed;
        let candidates: Vec<usize> = states[length].items.clone();
        let completed_truncated = states[length].truncated;
        let mut all = Vec::with_capacity(candidates.len());
        for index in candidates {
            all.push(self.evaluate_state(index)?);
        }
        // 参照 `emit`：无模型 → NoModel；有模型 → prefer_score 时 ScoreFirst，
        // 否则 RankFirst（注意与 `current_state_comparator` 的 allow_dup 分支不同）。
        let mut comparator = if self.model.is_none() {
            Comparator::NoModel
        } else if self.prefer_score_over_lexicon_rank(&all) {
            Comparator::ScoreFirst
        } else {
            Comparator::RankFirst
        };
        if all.iter().any(|item| item.learning_score > 0.0) {
            comparator = Comparator::ScoreFirst;
        }
        let mut order: Vec<usize> = (0..all.len()).collect();
        order.sort_by(|&left, &right| {
            if Decoder::state_better(comparator, &all[left], &all[right]) {
                std::cmp::Ordering::Less
            } else {
                std::cmp::Ordering::Greater
            }
        });
        order.truncate(CANDIDATE_LIMIT);
        let mut items: Vec<Evaluated> = order.iter().map(|&index| all[index].clone()).collect();
        // 参照中 Top-K 与 `_confidence_candidates` 共享同一批表：展示字段（segmented）
        // 需同步回写，保持两个视图一致。
        for (position, item) in items.iter_mut().enumerate() {
            item.segmented = segmented_from_path(raw, &self.arena, item.path);
            all[order[position]].segmented = item.segmented.clone();
        }
        // 词先验：只重排展示 Top-N（不改 mass/置信度，也不改变候选集合）。
        if items.len() > 1
            && let Some(model) = &self.lexical
            && self.ranking_prior.lexical_prior_weight > 0.0
            && self.model.is_some()
        {
            let mut cache = Map::new();
            let limit = self.ranking_prior.lexical_candidate_limit.min(items.len());
            for (position, item) in items.iter_mut().take(limit).enumerate() {
                let lexical_score = model.score_with_cache(&item.text, &mut cache)
                    * self.ranking_prior.lexical_prior_weight;
                item.score += lexical_score;
                all[order[position]].score = item.score;
            }
            items.sort_by(|left, right| {
                if Decoder::state_better(comparator, left, right) {
                    std::cmp::Ordering::Less
                } else {
                    std::cmp::Ordering::Greater
                }
            });
        }
        // 跨来源偏好融合（参照 `learning.apply_fusion_ordering(raw, result)`）：
        // 在词先验重排之后、证据构建之前重排展示 Top-K。
        {
            let (index, mode) = match &mut self.learning {
                Some(wiring) => (Some(&mut wiring.index), wiring.mode.as_str()),
                None => (None, ""),
            };
            apply_fusion_ordering(index, mode, raw, &mut items);
        }
        let mut evidence = Evidence::default_for(completed_truncated);
        if include_early_commit {
            evidence = self.build_early_commit_evidence(
                raw,
                states,
                &all,
                completed_truncated,
                required_text_prefix,
            )?;
        }
        // 可见顶层候选路径上的全部前缀（参照 `prefix_belongs_to_visible`）。
        let mut visible_prefixes: Set<(usize, String)> = Set::new();
        for item in &items {
            let mut current = Some(item.path);
            while let Some(index) = current {
                let text = self.arena[index].text.clone();
                if item.text.starts_with(&text) {
                    visible_prefixes.insert((self.arena[index].raw_length, text));
                }
                current = self.arena[index].previous;
            }
        }
        Ok(DecodeOutput {
            items,
            confidence_candidates: all,
            evidence,
            visible_prefixes,
            learning_affected: self.learning_affected,
            completed_truncated,
        })
    }

    fn prefer_score_over_lexicon_rank(&self, values: &[Evaluated]) -> bool {
        if !self.allow_duplicate_single {
            return false;
        }
        values.iter().any(|item| {
            self.arena[item.path]
                .previous
                .map(|previous| self.arena[previous].raw_length > 0)
                .unwrap_or(false)
        })
    }
}

// ---------------------------------------------------------------- 跨来源融合

/// 参照 `learning.fusion_score`；无学习库（`None`，对应参照的 `learning_index == nil`）
/// 或空模式时恒 0。
fn fusion_score(
    index: &mut Option<&mut LearningIndex>,
    mode: &str,
    raw: &[u8],
    direct: &str,
    composed: &str,
) -> f64 {
    match index {
        None => 0.0,
        Some(index) => index.fusion_score(mode, raw, direct, composed),
    }
}

/// 参照 `learning.apply_fusion_ordering`：保持 Direct 的原始菜单序，
/// 再按成对偏好把 Direct / Composed 两列做**两指针归并**。
///
/// - 分列：`candidate_is_direct` 为真进 Direct 列，否则（含锁定重放的未标记来源）
///   进 Composed 列。仅当候选**全为 Direct** 时用重排后的 Direct 列回写
///   ——这就是「直接序保持」的落点；全为 Composed 时保持既有顺序。
/// - 每步前缀前瞻：Direct 侧 `max fusion_score(raw, direct[i], c)`，
///   Composed 侧 `max −fusion_score(raw, d, composed[i])`。两侧都不 `> 0`，
///   或差值落在 `1e-12` 内（含完全相等）时，回退到 `base`（原始下标）较小者，
///   即保持原交错序。
/// - 无学习库时所有 `fusion_score` 恒 0 ⇒ 归并退化为「全 Direct 时按 `direct_rank`
///   重排、其余保持原序」。
fn apply_fusion_ordering(
    mut index: Option<&mut LearningIndex>,
    mode: &str,
    raw: &[u8],
    candidates: &mut [Evaluated],
) {
    if candidates.len() < 2 {
        return;
    }
    let mut base: HashMap<String, usize> = HashMap::new();
    let mut direct: Vec<usize> = Vec::new();
    let mut composed: Vec<usize> = Vec::new();
    for (position, item) in candidates.iter().enumerate() {
        base.entry(item.text.clone()).or_insert(position);
        if candidate_is_direct(item.source_mask) {
            direct.push(position);
        } else {
            composed.push(position);
        }
    }
    // `table.sort` 的比较器在此处是全序（`direct_rank` 同值回退 `base` 下标），
    // 故与参照的不稳定排序等价。
    direct.sort_by(|&left, &right| {
        let left_rank = candidates[left].direct_rank;
        let right_rank = candidates[right].direct_rank;
        if left_rank != right_rank {
            return left_rank
                .partial_cmp(&right_rank)
                .unwrap_or(std::cmp::Ordering::Equal);
        }
        base_index(&base, &candidates[left].text).cmp(&base_index(&base, &candidates[right].text))
    });
    if direct.is_empty() || composed.is_empty() {
        if composed.is_empty() {
            let reordered: Vec<Evaluated> = direct
                .iter()
                .map(|&position| candidates[position].clone())
                .collect();
            candidates.clone_from_slice(&reordered);
        }
        return;
    }
    let mut merged: Vec<usize> = Vec::with_capacity(candidates.len());
    let (mut di, mut ci) = (0usize, 0usize);
    while di < direct.len() && ci < composed.len() {
        let d = direct[di];
        let c = composed[ci];
        let mut direct_prefix = 0.0f64;
        for &ahead in &direct[di..] {
            direct_prefix = direct_prefix.max(fusion_score(
                &mut index,
                mode,
                raw,
                &candidates[ahead].text,
                &candidates[c].text,
            ));
        }
        let mut composed_prefix = 0.0f64;
        for &ahead in &composed[ci..] {
            composed_prefix = composed_prefix.max(-fusion_score(
                &mut index,
                mode,
                raw,
                &candidates[d].text,
                &candidates[ahead].text,
            ));
        }
        let by_base =
            || base_index(&base, &candidates[d].text) < base_index(&base, &candidates[c].text);
        let take_direct = if direct_prefix > 0.0 || composed_prefix > 0.0 {
            if (direct_prefix - composed_prefix).abs() > 1e-12 {
                direct_prefix > composed_prefix
            } else {
                by_base()
            }
        } else {
            by_base()
        };
        if take_direct {
            merged.push(d);
            di += 1;
        } else {
            merged.push(c);
            ci += 1;
        }
    }
    merged.extend_from_slice(&direct[di..]);
    merged.extend_from_slice(&composed[ci..]);
    let reordered: Vec<Evaluated> = merged
        .iter()
        .map(|&position| candidates[position].clone())
        .collect();
    candidates.clone_from_slice(&reordered);
}

/// 参照 `base[text] or math.huge`：缺失文本排到最后。
fn base_index(base: &HashMap<String, usize>, text: &str) -> usize {
    base.get(text).copied().unwrap_or(usize::MAX)
}

// ---------------------------------------------------------------- 学习奖励

/// 参照 `learning.reward`，但沿解码状态链（arena）读取节点。
/// 返回 `(best, potential, early_bonus)`（参照三元返回）。
fn learning_reward(
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

// ---------------------------------------------------------------- 早提交证据

impl Decoder {
    /// 参照 `add_early_commit_pool_candidate`。
    fn add_pool_candidate(
        &self,
        pool: &mut Vec<EvidenceCandidate>,
        pool_index: &mut HashMap<usize, HashMap<String, usize>>,
        text: String,
        confidence_score: f64,
        early_commit_confidence_score: f64,
        path: usize,
    ) {
        if text.is_empty() {
            return;
        }
        let raw_length = self.arena[path].raw_length;
        let boundary = pool_index.entry(raw_length).or_default();
        match boundary.get(&text).copied() {
            None => {
                pool.push(EvidenceCandidate {
                    text: text.clone(),
                    confidence_score,
                    early_commit_confidence_score,
                    path,
                });
                boundary.insert(text, pool.len() - 1);
            }
            Some(position) => {
                let previous = &pool[position];
                let combined = logsumexp(previous.confidence_score, confidence_score);
                let combined_early =
                    logsumexp(previous.early_confidence(), early_commit_confidence_score);
                // 参照：`best` 按早提交置信度（而非基础置信度）择优。
                let best_path = if early_commit_confidence_score > previous.early_confidence() {
                    path
                } else {
                    previous.path
                };
                pool[position] = EvidenceCandidate {
                    text,
                    confidence_score: combined,
                    early_commit_confidence_score: combined_early,
                    path: best_path,
                };
            }
        }
    }

    /// 参照 `incomplete_code_tail`。
    fn incomplete_code_tail(&self, tail: &[u8]) -> bool {
        if tail.is_empty() || !tail.iter().all(|byte| byte.is_ascii_alphabetic()) {
            return false;
        }
        let Ok(text) = std::str::from_utf8(tail) else {
            return false;
        };
        if !self.lexicon.proper_code_prefixes.contains(text) {
            return false;
        }
        tail.len() < 2 || !self.lexicon.codes.contains_key(text)
    }

    /// 参照 `build_early_commit_evidence`。
    fn build_early_commit_evidence(
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
        let mut visible: Vec<&Evaluated> = Vec::new();
        for candidate in completed {
            if candidate.text.is_empty() {
                continue;
            }
            if !required_text_prefix.is_empty() && !candidate.text.starts_with(required_text_prefix)
            {
                continue;
            }
            visible.push(candidate);
            self.add_pool_candidate(
                &mut pool,
                &mut pool_index,
                candidate.text.clone(),
                candidate.confidence_score,
                candidate.early_commit_confidence_score,
                candidate.path,
            );
        }

        let mut truncated = completed_truncated;
        let mut merged_incomplete_tail = false;
        let maximum_tail_length = self
            .lexicon
            .max_code_len
            .saturating_sub(1)
            .min(raw.len().saturating_sub(1));
        for tail_length in 1..=maximum_tail_length {
            let consumed_length = raw.len() - tail_length;
            let tail = &raw[consumed_length..];
            if !self.incomplete_code_tail(tail) || consumed_length >= states.len() {
                continue;
            }
            let partial = self.dedup_limit(
                std::mem::take(&mut states[consumed_length]),
                beam_limit_at(consumed_length),
            );
            states[consumed_length] = partial;
            let partial_items: Vec<usize> = states[consumed_length].items.clone();
            let partial_truncated = states[consumed_length].truncated;
            let mut added = false;
            for index in partial_items {
                let candidate = self.evaluate_state(index)?;
                if candidate.text.is_empty() {
                    continue;
                }
                if !required_text_prefix.is_empty()
                    && !candidate.text.starts_with(required_text_prefix)
                {
                    continue;
                }
                self.add_pool_candidate(
                    &mut pool,
                    &mut pool_index,
                    candidate.text.clone(),
                    candidate.confidence_score,
                    candidate.early_commit_confidence_score,
                    candidate.path,
                );
                added = true;
            }
            if added {
                merged_incomplete_tail = true;
                truncated = truncated || partial_truncated;
            }
        }

        let prefixes = build_prefix_evidence(&pool, &self.arena);

        let mut proposal = String::new();
        let mut proposal_share = 0.0;
        let mut proposal_raw_length = 0usize;
        let mut proposal_chars = 0usize;
        let mut raw_lengths: Map<String, usize> = Map::new();
        let mut raw_share: HashMap<String, f64> = HashMap::new();
        for prefix in &prefixes {
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
        let by_boundary = prefix_lookup(&prefixes);
        Ok(Evidence {
            prefixes,
            by_boundary,
            proposal,
            proposal_share,
            raw_lengths,
            neutral_incomplete_tail: visible.is_empty() && merged_incomplete_tail,
            merged_incomplete_tail,
            neutral_low_confidence: has_low_confidence_completed_generation(&visible),
            confidence_truncated: truncated,
        })
    }
}

/// 参照 `build_prefix_evidence`：按 (前缀文本, raw 边界) 汇总证据。
/// 基础权重（模型置信度）与早提交权重（个性化置信度）各算一套份额。
fn build_prefix_evidence(pool: &[EvidenceCandidate], arena: &[State]) -> Vec<PrefixEvidence> {
    if pool.is_empty() {
        return Vec::new();
    }
    let base_max = pool
        .iter()
        .map(|candidate| candidate.confidence_score)
        .fold(f64::NEG_INFINITY, f64::max);
    let early_max = pool
        .iter()
        .map(EvidenceCandidate::early_confidence)
        .fold(f64::NEG_INFINITY, f64::max);
    let base_weights: Vec<f64> = pool
        .iter()
        .map(|candidate| (candidate.confidence_score - base_max).exp())
        .collect();
    let early_weights: Vec<f64> = pool
        .iter()
        .map(|candidate| (candidate.early_confidence() - early_max).exp())
        .collect();
    let base_total: f64 = base_weights.iter().sum();
    let early_total: f64 = early_weights.iter().sum();
    if base_total <= 0.0 || early_total <= 0.0 {
        return Vec::new();
    }
    let mut entries: Vec<PrefixEvidence> = Vec::new();
    let mut entry_base_weights: Vec<f64> = Vec::new();
    let mut entry_early_weights: Vec<f64> = Vec::new();
    let mut entry_index: HashMap<(usize, String), usize> = HashMap::new();
    let mut base_boundary_mass: HashMap<usize, f64> = HashMap::new();
    for (position, item) in pool.iter().enumerate() {
        let base_weight = base_weights[position];
        let early_weight = early_weights[position];
        let mut current = Some(item.path);
        while let Some(index) = current {
            let prefix_text = arena[index].text.clone();
            if !prefix_text.is_empty() && prefix_text.len() <= item.text.len() {
                let raw_length = arena[index].raw_length;
                let key = (raw_length, prefix_text.clone());
                let slot = match entry_index.get(&key) {
                    Some(&slot) => slot,
                    None => {
                        let slot = entries.len();
                        entries.push(PrefixEvidence {
                            text: prefix_text,
                            raw_length,
                            share: 0.0,
                            base_share: 0.0,
                            boundary_share: 0.0,
                            boundary_closed: false,
                            text_char_count: 0,
                        });
                        entry_base_weights.push(0.0);
                        entry_early_weights.push(0.0);
                        entry_index.insert(key, slot);
                        slot
                    }
                };
                entry_base_weights[slot] += base_weight;
                entry_early_weights[slot] += early_weight;
                *base_boundary_mass.entry(raw_length).or_insert(0.0) += base_weight;
            }
            current = arena[index].previous;
        }
    }
    for (position, entry) in entries.iter_mut().enumerate() {
        let boundary = base_boundary_mass
            .get(&entry.raw_length)
            .copied()
            .unwrap_or(0.0);
        let boundary_share = boundary / base_total;
        entry.share = entry_early_weights[position] / early_total;
        entry.base_share = entry_base_weights[position] / base_total;
        entry.boundary_share = boundary_share;
        entry.boundary_closed = boundary_share >= EARLY_COMMIT_CLOSED_BOUNDARY_SHARE;
        entry.text_char_count = entry.text.chars().count();
    }
    entries
}

fn prefix_lookup(prefixes: &[PrefixEvidence]) -> Map<usize, Map<String, usize>> {
    let mut lookup: Map<usize, Map<String, usize>> = Map::new();
    for (index, prefix) in prefixes.iter().enumerate() {
        lookup
            .entry_or_default(prefix.raw_length)
            .insert(prefix.text.clone(), index);
    }
    lookup
}

/// 参照 `has_low_confidence_completed_generation`。
fn has_low_confidence_completed_generation(candidates: &[&Evaluated]) -> bool {
    if candidates.is_empty() {
        return false;
    }
    let max_score = candidates
        .iter()
        .map(|candidate| candidate.confidence_score)
        .fold(f64::NEG_INFINITY, f64::max);
    let total: f64 = candidates
        .iter()
        .map(|candidate| (candidate.confidence_score - max_score).exp())
        .sum();
    total > 0.0 && 1.0 / total < EARLY_COMMIT_MINIMUM_SHARE
}

struct Eligible {
    text: String,
    rank: usize,
    optimal_single: bool,
    primary_single: bool,
    is_single: bool,
    chars: Vec<char>,
    log_rank: f64,
}

impl Eligible {
    fn new(entry: &CodeEntry) -> Self {
        let chars: Vec<char> = entry.text.chars().collect();
        Self {
            text: entry.text.clone(),
            rank: entry.rank,
            optimal_single: entry.optimal_single,
            primary_single: entry.primary_single,
            is_single: chars.len() == 1,
            chars,
            log_rank: (entry.rank as f64).ln(),
        }
    }
}

/// 对应 `State` 在扩展循环中的只读视图（避免与 `&mut self` 借用冲突）。
struct StateView {
    score: f64,
    mass_score: f64,
    code_score: f64,
    text: String,
    prev2: char,
    prev1: char,
    max_rank: usize,
    supplement_state: usize,
    supplement_score: f64,
    edge_count: usize,
    learning_score: f64,
    learning_early_commit_bonus: f64,
}

impl State {
    fn view(&self) -> StateView {
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

fn select_top(
    arena: &[State],
    values: Vec<usize>,
    limit: usize,
    comparator: Comparator,
) -> Vec<usize> {
    let mut values = values;
    values.sort_by(|&left, &right| {
        if Decoder::state_better_raw(comparator, &arena[left], &arena[right]) {
            std::cmp::Ordering::Less
        } else {
            std::cmp::Ordering::Greater
        }
    });
    values.truncate(limit);
    values
}

fn logsumexp(left: f64, right: f64) -> f64 {
    let maximum = left.max(right);
    maximum + ((left - maximum).exp() + (right - maximum).exp()).ln()
}

fn beam_limit_at(raw_length: usize) -> usize {
    if raw_length > LONG_INPUT_FULL_BEAM_LENGTH {
        LONG_INPUT_BEAM_WIDTH
    } else {
        BEAM_WIDTH
    }
}

/// 参照 `normalize`：ASCII 小写化并去除 Lua `%s` 空白（含垂直制表符）。
fn normalize(raw: &str) -> Vec<u8> {
    let mut out = Vec::with_capacity(raw.len());
    for byte in raw.bytes() {
        if byte.is_ascii_whitespace() || byte == 0x0b {
            continue;
        }
        out.push(byte.to_ascii_lowercase());
    }
    out
}

fn has_letter(raw: &[u8]) -> bool {
    raw.iter().any(|byte| byte.is_ascii_alphabetic())
}

/// 参照 `locked.boundaries:gmatch("(%d+),(%d+);")`（失败起点逐一右移重试）。
fn parse_boundaries(value: &str) -> Vec<(usize, usize)> {
    let bytes = value.as_bytes();
    let mut result = Vec::new();
    let mut index = 0usize;
    while index < bytes.len() {
        let first_start = index;
        while index < bytes.len() && bytes[index].is_ascii_digit() {
            index += 1;
        }
        if index == first_start || bytes.get(index) != Some(&b',') {
            index = first_start + 1;
            continue;
        }
        let first = &value[first_start..index];
        index += 1;
        let second_start = index;
        while index < bytes.len() && bytes[index].is_ascii_digit() {
            index += 1;
        }
        if index == second_start || bytes.get(index) != Some(&b';') {
            index = first_start + 1;
            continue;
        }
        let second = &value[second_start..index];
        index += 1;
        if let (Ok(raw_length), Ok(text_length)) = (first.parse(), second.parse()) {
            result.push((raw_length, text_length));
        }
    }
    result
}

/// 参照 `parse_selector`：返回 (选中 rank, 消耗到的字节位置)；0 = 无选择器。
fn parse_selector(raw: &[u8], code_end: usize) -> (u64, usize) {
    let next = code_end;
    if next >= raw.len() {
        return (0, code_end);
    }
    match raw[next] {
        b';' => return (2, next + 1),
        b'\'' => return (3, next + 1),
        byte if byte.is_ascii_digit() => {
            let mut digit_end = next;
            while digit_end + 1 < raw.len() && raw[digit_end + 1].is_ascii_digit() {
                digit_end += 1;
            }
            let token = std::str::from_utf8(&raw[next..=digit_end]).unwrap_or("0");
            if token == "0" {
                return (10, digit_end + 1);
            }
            // Lua `tonumber(token)` 对超长数字得到巨大浮点，永不匹配任何 rank；
            // 溢出时取 u64::MAX，避免退化成“无选择器”。
            return (token.parse::<u64>().unwrap_or(u64::MAX), digit_end + 1);
        }
        _ => {}
    }
    (0, code_end)
}

/// 参照 `advance_required_prefix`（字节比较）。
fn advance_required_prefix(required: &str, matched: usize, candidate: &str) -> Option<usize> {
    if matched >= required.len() {
        return Some(matched);
    }
    let required_bytes = required.as_bytes();
    let candidate_bytes = candidate.as_bytes();
    let compare = candidate_bytes.len().min(required.len() - matched);
    if compare == 0 || required_bytes[matched..matched + compare] != candidate_bytes[..compare] {
        return None;
    }
    Some(required.len().min(matched + candidate_bytes.len()))
}

fn has_selection_suffix_bytes(raw: &[u8]) -> bool {
    raw.iter()
        .any(|byte| *byte == b';' || *byte == b'\'' || byte.is_ascii_digit())
}

/// 参照 `has_complete_candidate(raw_code, required_text_prefix, excluded_text,
/// group_eligible_only, locked)`。
pub fn has_complete_candidate(
    lexicon: &Lexicon,
    raw_code: &str,
    required_text_prefix: &str,
    excluded_text: Option<&str>,
    group_eligible_only: bool,
    allow_duplicate_single: bool,
    lock: Option<&DecodeLock<'_>>,
) -> bool {
    let raw = normalize(raw_code);
    if raw.is_empty() || !has_letter(&raw) {
        return false;
    }
    let required = required_text_prefix;
    if required.is_empty() && excluded_text.is_none() && !group_eligible_only && lock.is_none() {
        let mut reachable = vec![false; raw.len() + 1];
        reachable[0] = true;
        for position in 0..raw.len() {
            if !reachable[position] {
                continue;
            }
            for &code_length in &lexicon.lengths {
                if position + code_length > raw.len() {
                    break;
                }
                let Ok(code) = std::str::from_utf8(&raw[position..position + code_length]) else {
                    continue;
                };
                let Some(candidates) = lexicon.codes.get(code) else {
                    continue;
                };
                let (selected_rank, consumed_end) = parse_selector(&raw, position + code_length);
                let whole_input_edge = position == 0 && consumed_end == raw.len();
                if raw.len() > 1 && consumed_end - position < 2 {
                    continue;
                }
                if !eligible_candidates(
                    candidates,
                    selected_rank,
                    whole_input_edge,
                    allow_duplicate_single,
                )
                .is_empty()
                {
                    reachable[consumed_end] = true;
                }
            }
        }
        return reachable[raw.len()];
    }

    let first_ranks_only = group_eligible_only && !has_selection_suffix_bytes(&raw);
    let stride = excluded_text.map(|text| text.len() + 2).unwrap_or(1);
    let mut states: Vec<HashSet<usize>> = (0..=raw.len()).map(|_| HashSet::new()).collect();
    let mut start = 0usize;
    let mut matched = 0usize;
    let mut excluded = 0usize;
    if let Some(lock) = lock {
        // 参照：锁前缀必须同时匹配输入与已确认文本，扫描自锁末端开始。
        let prefix = normalize(lock.raw);
        matched = required.len().min(lock.text.len());
        if !raw.starts_with(&prefix)
            || required.as_bytes().get(..matched) != lock.text.as_bytes().get(..matched)
        {
            return false;
        }
        start = prefix.len();
        if let Some(excluded_text) = excluded_text {
            excluded = if excluded_text.as_bytes().starts_with(lock.text.as_bytes()) {
                lock.text.len()
            } else {
                excluded_text.len() + 1
            };
        }
        if start == raw.len() {
            return matched == required.len()
                && excluded_text
                    .map(|text| excluded != text.len())
                    .unwrap_or(true);
        }
    }
    states[start].insert(matched * stride + excluded);
    for position in start..raw.len() {
        if states[position].is_empty() {
            continue;
        }
        let packed_states: Vec<usize> = states[position].iter().copied().collect();
        for &code_length in &lexicon.lengths {
            let code_end = position + code_length;
            if code_end > raw.len() {
                break;
            }
            let Ok(code) = std::str::from_utf8(&raw[position..code_end]) else {
                continue;
            };
            let Some(candidates) = lexicon.codes.get(code) else {
                continue;
            };
            let (selected_rank, consumed_end) = parse_selector(&raw, code_end);
            let whole_input_edge = position == 0 && consumed_end == raw.len();
            if raw.len() > 1 && consumed_end - position < 2 {
                continue;
            }
            let selected = eligible_candidates(
                candidates,
                selected_rank,
                whole_input_edge,
                allow_duplicate_single,
            );
            for &packed in &packed_states {
                let matched_length = packed / stride;
                for candidate in &selected {
                    let Some(next_matched) =
                        advance_required_prefix(required, matched_length, &candidate.text)
                    else {
                        continue;
                    };
                    if first_ranks_only
                        && candidate.rank != 1
                        && !(allow_duplicate_single && candidate.text.chars().count() == 1)
                    {
                        continue;
                    }
                    let mut next_excluded = packed % stride;
                    if let Some(excluded) = excluded_text
                        && next_excluded <= excluded.len()
                    {
                        let tail = &excluded.as_bytes()[next_excluded..];
                        if tail.starts_with(candidate.text.as_bytes()) {
                            next_excluded += candidate.text.len();
                        } else {
                            next_excluded = excluded.len() + 1;
                        }
                    }
                    if consumed_end == raw.len()
                        && next_matched == required.len()
                        && excluded_text
                            .map(|text| next_excluded != text.len())
                            .unwrap_or(true)
                    {
                        return true;
                    }
                    states[consumed_end].insert(next_matched * stride + next_excluded);
                }
            }
        }
    }
    false
}

/// 参照 `eligible_candidates`。
fn eligible_candidates(
    candidates: &[CodeEntry],
    selected_rank: u64,
    whole_input_edge: bool,
    allow_duplicate_single: bool,
) -> Vec<&CodeEntry> {
    let single = |entry: &CodeEntry| entry.text.chars().count() == 1;
    if candidates.len() == 1 {
        let candidate = &candidates[0];
        if selected_rank > 0 {
            if candidate.rank as u64 == selected_rank {
                return vec![candidate];
            }
        } else if whole_input_edge
            || candidate.rank == 1
            || (allow_duplicate_single && single(candidate))
        {
            return vec![candidate];
        }
    }
    if selected_rank == 0 {
        if whole_input_edge {
            return candidates.iter().collect();
        }
        if allow_duplicate_single {
            return candidates
                .iter()
                .filter(|entry| entry.rank == 1 || single(entry))
                .collect();
        }
    }
    let rank = if selected_rank > 0 { selected_rank } else { 1 };
    candidates
        .iter()
        .filter(|entry| entry.rank as u64 == rank)
        .collect()
}

/// 参照 `segmented_from_path`：按路径边界切分原始输入，以空格连接。
fn segmented_from_path(raw: &[u8], arena: &[State], path: usize) -> String {
    if raw.is_empty() {
        return String::new();
    }
    let mut ends = Vec::new();
    let mut current = Some(path);
    while let Some(index) = current {
        if arena[index].raw_length == 0 {
            break;
        }
        ends.push(arena[index].raw_length);
        current = arena[index].previous;
    }
    let mut pieces = Vec::new();
    let mut start = 0usize;
    for &finish in ends.iter().rev() {
        if finish <= start || finish > raw.len() {
            break;
        }
        pieces.push(String::from_utf8_lossy(&raw[start..finish]).into_owned());
        start = finish;
    }
    pieces.join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture_lexicon() -> Lexicon {
        let dir =
            std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../goldens/lexicon");
        Lexicon::load(std::slice::from_ref(&dir), 1500)
    }

    fn fixture_decoder() -> Decoder {
        let dir =
            std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../goldens/lexicon");
        let lexicon = Lexicon::load(std::slice::from_ref(&dir), 1500);
        let supplement = Supplement::load_default(Some(&dir));
        Decoder::new(lexicon, supplement, None)
    }

    /// 融合排序用例的最小候选（其余字段与排序无关）。
    fn fusion_candidate(text: &str, source_mask: u8, direct_rank: f64) -> Evaluated {
        Evaluated {
            text: text.to_string(),
            score: 0.0,
            confidence_score: 0.0,
            early_commit_confidence_score: 0.0,
            code_score: 0.0,
            max_rank: 1,
            supplement_score: 0.0,
            learning_score: 0.0,
            edge_count: 1,
            source_mask,
            direct_rank,
            path: 0,
            segmented: String::new(),
            previous_raw_length: 0,
            previous_text: None,
        }
    }

    fn fusion_texts(candidates: &[Evaluated]) -> Vec<&str> {
        candidates.iter().map(|item| item.text.as_str()).collect()
    }

    /// 融合排序索引：一条 `fusion_event` 落到 `mode` 的融合分区里。
    fn fusion_index(
        mode: &str,
        raw: &[u8],
        direct: &str,
        composed: &str,
        direct_wins: bool,
    ) -> LearningIndex {
        let event = hux_core::learning::fusion_event(
            mode,
            raw,
            direct,
            composed,
            direct_wins,
            raw.len(),
            1000.0,
        )
        .expect("融合事件");
        LearningIndex::build(
            &[hux_core::learning::Event {
                time: event.time,
                mode: event.mode,
                code: event.code,
                text: event.text,
                context: event.context,
            }],
            1000.0,
        )
    }

    /// 照抄上游 `tools/test_sentence_learning.lua` 的融合用例：
    /// 无事件保持原交错序；一条 `C > A` 的 Direct 偏好只把 Direct 前缀提到 A 之前。
    #[test]
    fn fusion_ordering_matches_reference_cases() {
        let mode = "sentence-v2|test";
        let cases = || {
            vec![
                fusion_candidate("A", SOURCE_COMPOSED, f64::INFINITY),
                fusion_candidate("B", SOURCE_DIRECT, 1.0),
                fusion_candidate("C", SOURCE_DIRECT, 2.0),
            ]
        };
        // 无学习库：两侧前缀分都是 0 ⇒ 回退原始下标顺序。
        let mut items = cases();
        apply_fusion_ordering(None, mode, b"ii", &mut items);
        assert_eq!(fusion_texts(&items), ["A", "B", "C"]);
        // 空模式（未接入学习）同样退化为原序。
        let mut items = cases();
        apply_fusion_ordering(None, "", b"ii", &mut items);
        assert_eq!(fusion_texts(&items), ["A", "B", "C"]);
        // 一条 `Direct C > Composed A`：Direct 列的 B、C 一起前移，A 退到最后。
        let mut index = fusion_index(mode, b"ii", "C", "A", true);
        let mut items = cases();
        apply_fusion_ordering(Some(&mut index), mode, b"ii", &mut items);
        assert_eq!(fusion_texts(&items), ["B", "C", "A"]);
    }

    #[test]
    fn fusion_ordering_preserves_direct_order_and_reverse_preference() {
        let mode = "sentence-v2|test";
        // 全为 Direct：与学习库无关地按 `direct_rank` 重排（「直接序保持」）。
        let mut items = vec![
            fusion_candidate("C", SOURCE_DIRECT, 2.0),
            fusion_candidate("A", SOURCE_DIRECT, f64::INFINITY),
            fusion_candidate("B", SOURCE_DIRECT, 1.0),
        ];
        apply_fusion_ordering(None, mode, b"ii", &mut items);
        assert_eq!(fusion_texts(&items), ["B", "C", "A"]);
        // 反向偏好（Composed 胜）：Composed 列先出。
        let mut index = fusion_index(mode, b"ii", "B", "A", false);
        let mut items = vec![
            fusion_candidate("B", SOURCE_DIRECT, 1.0),
            fusion_candidate("A", SOURCE_COMPOSED, f64::INFINITY),
        ];
        apply_fusion_ordering(Some(&mut index), mode, b"ii", &mut items);
        assert_eq!(fusion_texts(&items), ["A", "B"]);
    }

    #[test]
    fn normalize_strips_whitespace_and_control() {
        assert_eq!(normalize("A B\tC\r\n"), b"abc");
        assert_eq!(normalize("a\u{0b}b"), b"ab");
    }

    #[test]
    fn has_letter_detects_ascii_letters() {
        assert!(has_letter(b"a1"));
        assert!(!has_letter(b"123"));
    }

    #[test]
    fn parse_selector_parses_and_saturates() {
        assert_eq!(parse_selector(b"ab;", 2), (2, 3));
        assert_eq!(parse_selector(b"ab'", 2), (3, 3));
        assert_eq!(parse_selector(b"ab0", 2), (10, 3));
        assert_eq!(parse_selector(b"ab12", 2), (12, 4));
        assert_eq!(parse_selector(b"ab00", 2), (0, 4));
        assert_eq!(parse_selector(b"ab99999999999999999999", 2), (u64::MAX, 22));
        assert_eq!(parse_selector(b"ab", 2), (0, 2));
    }

    #[test]
    fn locked_decode_rebuilds_confirmed_prefix() {
        let dir =
            std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../goldens/lexicon");
        let lexicon = Lexicon::load(std::slice::from_ref(&dir), 1500);
        let supplement = Supplement::load_default(Some(&dir));
        let mut decoder = Decoder::new(lexicon, supplement, None);
        let unlocked = decoder.decode_with("ab", false, "").expect("decode");
        let top = unlocked.items.first().expect("candidates").clone();
        // 路径链 root..top；取第一个非根节点作为局部锁边界。
        let mut chain = Vec::new();
        let mut current = Some(top.path);
        while let Some(index) = current {
            chain.push(index);
            current = decoder.arena[index].previous;
        }
        chain.reverse();
        assert!(chain.len() >= 2, "期望多节点路径");
        let node = chain[1];
        let (raw_length, text_length) = (
            decoder.arena[node].raw_length,
            decoder.arena[node].text_length,
        );
        let locked_text = decoder.arena[node].text.clone();
        let locked_raw = "ab"[..raw_length].to_string();
        let boundaries = format!("{raw_length},{text_length};");
        let lock = DecodeLock {
            raw: &locked_raw,
            text: &locked_text,
            boundaries: &boundaries,
        };
        let locked = decoder
            .decode_with_lock("ab", false, "", Some(lock))
            .expect("locked decode");
        assert!(!locked.items.is_empty());
        for item in &locked.items {
            assert!(item.text.starts_with(&locked_text), "{}", item.text);
        }
    }

    #[test]
    fn locked_decode_honors_full_input_lock() {
        let dir =
            std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../goldens/lexicon");
        let lexicon = Lexicon::load(std::slice::from_ref(&dir), 1500);
        let supplement = Supplement::load_default(Some(&dir));
        let mut decoder = Decoder::new(lexicon, supplement, None);
        let unlocked = decoder.decode_with("ab", false, "").expect("decode");
        let top = unlocked.items.first().expect("candidates").clone();
        let mut chain = Vec::new();
        let mut current = Some(top.path);
        while let Some(index) = current {
            chain.push(index);
            current = decoder.arena[index].previous;
        }
        chain.reverse();
        // 全量锁：以顶层候选路径的全部边界重建，前缀即整段输入。
        let boundaries: String = chain[1..]
            .iter()
            .map(|&index| {
                format!(
                    "{},{};",
                    decoder.arena[index].raw_length, decoder.arena[index].text_length
                )
            })
            .collect();
        let lock = DecodeLock {
            raw: "ab",
            text: &top.text,
            boundaries: &boundaries,
        };
        let locked = decoder
            .decode_with_lock("ab", false, "", Some(lock))
            .expect("locked decode");
        assert!(!locked.items.is_empty());
        assert!(
            locked.items.iter().all(|item| item.text == top.text),
            "{:?}",
            locked
                .items
                .iter()
                .map(|item| &item.text)
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn locked_decode_expands_after_partial_lock() {
        let dir =
            std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../goldens/lexicon");
        let lexicon = Lexicon::load(std::slice::from_ref(&dir), 1500);
        let supplement = Supplement::load_default(Some(&dir));
        let mut decoder = Decoder::new(lexicon, supplement, None);
        // 码表事实：ab → 交（rank 1）、疒（rank 2）；整段输入 >1 字节时单字节尾边被跳过，
        // 故 "abab" 唯一两段路径为 ab+ab。锁住首边后应继续解出 交交/交疒。
        let lock = DecodeLock {
            raw: "ab",
            text: "交",
            boundaries: "2,3;",
        };
        let locked = decoder
            .decode_with_lock("abab", false, "", Some(lock))
            .expect("locked decode");
        assert!(!locked.items.is_empty());
        assert!(locked.items.iter().all(|item| item.text.starts_with("交")));
        assert!(
            locked
                .items
                .iter()
                .any(|item| item.text.chars().count() > 1),
            "扩展应产生多字候选：{:?}",
            locked
                .items
                .iter()
                .map(|item| &item.text)
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn locked_decode_rejects_mismatches() {
        let dir =
            std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../goldens/lexicon");
        let lexicon = Lexicon::load(std::slice::from_ref(&dir), 1500);
        let supplement = Supplement::load_default(Some(&dir));
        let mut decoder = Decoder::new(lexicon, supplement, None);
        let unlocked = decoder.decode_with("ab", false, "").expect("decode");
        let top = unlocked.items.first().expect("candidates").clone();
        let node = decoder.arena[top.path].previous.expect("non-root path");
        let (raw_length, text_length) = (
            decoder.arena[node].raw_length,
            decoder.arena[node].text_length,
        );
        let locked_text = decoder.arena[node].text.clone();
        let locked_raw = "ab"[..raw_length].to_string();
        let boundaries = format!("{raw_length},{text_length};");
        let raw_mismatch = DecodeLock {
            raw: "xy",
            text: &locked_text,
            boundaries: &boundaries,
        };
        assert!(
            decoder
                .decode_with_lock("ab", false, "", Some(raw_mismatch))
                .unwrap()
                .items
                .is_empty()
        );
        let empty_raw = DecodeLock {
            raw: "",
            text: &locked_text,
            boundaries: &boundaries,
        };
        assert!(
            decoder
                .decode_with_lock("ab", false, "", Some(empty_raw))
                .unwrap()
                .items
                .is_empty()
        );
        let short = "0,0;".to_string();
        let short_lock = DecodeLock {
            raw: &locked_raw,
            text: &locked_text,
            boundaries: &short,
        };
        assert!(
            decoder
                .decode_with_lock("ab", false, "", Some(short_lock))
                .unwrap()
                .items
                .is_empty()
        );
        // 边界文本长度与锁文本不一致（"a" != "ab"）
        let text_boundary = format!("{raw_length},1;");
        let text_lock = DecodeLock {
            raw: &locked_raw,
            text: "ab",
            boundaries: &text_boundary,
        };
        assert!(
            decoder
                .decode_with_lock("ab", false, "", Some(text_lock))
                .unwrap()
                .items
                .is_empty()
        );
    }

    #[test]
    fn path_summary_orders_nodes_outermost_first() {
        let dir =
            std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../goldens/lexicon");
        let lexicon = Lexicon::load(std::slice::from_ref(&dir), 1500);
        let supplement = Supplement::load_default(Some(&dir));
        let mut decoder = Decoder::new(lexicon, supplement, None);
        let output = decoder.decode_with("abab", false, "").expect("decode");
        let item = output.items.first().expect("candidates");
        let (raw_length, diff) = decoder.path_summary(item);
        assert_eq!(raw_length, 4);
        assert_eq!(diff.text, item.text);
        assert!(!diff.path.is_empty());
        assert!(
            diff.path
                .windows(2)
                .all(|window| window[0].raw_length < window[1].raw_length)
        );
    }

    #[test]
    fn has_complete_candidate_detects_complete_input() {
        let lexicon = fixture_lexicon();
        // 无锁：abab 完整（ab → 交/疒），带必需前缀亦完整
        assert!(has_complete_candidate(
            &lexicon, "abab", "", None, false, true, None
        ));
        assert!(has_complete_candidate(
            &lexicon, "abab", "交", None, false, true, None
        ));
    }

    #[test]
    fn has_complete_candidate_scans_from_lock_end() {
        let lexicon = fixture_lexicon();
        // 锁 "ab"→交：扫描自锁末端开始
        let lock = DecodeLock {
            raw: "ab",
            text: "交",
            boundaries: "2,3;",
        };
        assert!(has_complete_candidate(
            &lexicon,
            "abab",
            "",
            None,
            false,
            true,
            Some(&lock)
        ));
        assert!(has_complete_candidate(
            &lexicon,
            "abab",
            "交",
            None,
            false,
            true,
            Some(&lock)
        ));
        assert!(has_complete_candidate(
            &lexicon,
            "ab",
            "交",
            None,
            false,
            true,
            Some(&lock)
        ));
    }

    #[test]
    fn has_complete_candidate_rejects_mismatched_lock() {
        let lexicon = fixture_lexicon();
        let lock = DecodeLock {
            raw: "ab",
            text: "交",
            boundaries: "2,3;",
        };
        // 锁前缀与输入不符 / 与已确认文本不符 → false
        let foreign = DecodeLock {
            raw: "cd",
            text: "交",
            boundaries: "2,3;",
        };
        assert!(!has_complete_candidate(
            &lexicon,
            "abab",
            "",
            None,
            false,
            true,
            Some(&foreign)
        ));
        assert!(!has_complete_candidate(
            &lexicon,
            "abab",
            "疒",
            None,
            false,
            true,
            Some(&lock)
        ));
    }

    #[test]
    fn has_complete_candidate_honors_excluded_text() {
        let lexicon = fixture_lexicon();
        let lock = DecodeLock {
            raw: "ab",
            text: "交",
            boundaries: "2,3;",
        };
        // excluded 与锁文本一致：「交」不算新完成，「交交」可以
        assert!(!has_complete_candidate(
            &lexicon,
            "ab",
            "交",
            Some("交"),
            false,
            true,
            Some(&lock)
        ));
        assert!(has_complete_candidate(
            &lexicon,
            "abab",
            "交",
            Some("交"),
            false,
            true,
            Some(&lock)
        ));
    }

    #[test]
    fn ranking_prior_parameters_defaults() {
        let parameters = RankingPriorParameters::default();
        assert_eq!(parameters.canonical_code_reward, 2.0);
        assert_eq!(parameters.lexical_prior_weight, 0.1);
        assert_eq!(parameters.lexical_candidate_limit, 5);
        assert_eq!(parameters.canonical_isolation_min_code_length, 4);
        // 5ce1ca2 新增的早提交先验（参照 `ranking_prior.*` 默认值）。
        assert_eq!(parameters.supplement_early_commit_scale, 0.05);
        assert_eq!(parameters.supplement_early_commit_cap, 0.75);
        assert_eq!(parameters.personalized_early_commit_cap, 0.80);
        assert_eq!(parameters.empty_code_strong_share, 0.99999);
        // 空码强阈值比普通强阈值（0.999）更严。
        assert!(parameters.empty_code_strong_share > 0.999);
        // decoder 默认即内建参数。
        let decoder = fixture_decoder();
        assert_eq!(decoder.ranking_prior_parameters(), parameters);
    }

    #[test]
    fn ranking_prior_parameters_setter_roundtrip() {
        let mut decoder = fixture_decoder();
        decoder.set_ranking_prior_parameters(RankingPriorParameters {
            canonical_code_reward: 1.0,
            ..RankingPriorParameters::default()
        });
        assert_eq!(
            decoder.ranking_prior_parameters().canonical_code_reward,
            1.0
        );
    }

    #[test]
    fn supplement_early_commit_contribution_matches_reference() {
        // 参照 `min(supplement_early_commit_cap, max(0, score) * scale)`。
        let parameters = RankingPriorParameters::default();
        assert_eq!(parameters.supplement_early_commit_contribution(0.0), 0.0);
        assert_eq!(parameters.supplement_early_commit_contribution(-3.0), 0.0);
        assert_eq!(parameters.supplement_early_commit_contribution(4.0), 0.2);
        assert_eq!(parameters.supplement_early_commit_contribution(15.0), 0.75);
        assert_eq!(
            parameters.supplement_early_commit_contribution(1000.0),
            0.75
        );
        // 消融：放大斜率后仍受上限约束。
        let scaled = RankingPriorParameters {
            supplement_early_commit_scale: 0.5,
            ..parameters
        };
        assert_eq!(scaled.supplement_early_commit_contribution(2.0), 0.75);
    }

    #[test]
    fn decode_without_model_has_zero_code_score() {
        let mut decoder = fixture_decoder();
        // 无模型时码形证据不累计，故恒为 0。
        let output = decoder.decode_with("ab", false, "").expect("decode");
        assert!(!output.items.is_empty());
        assert!(output.items.iter().all(|item| item.code_score == 0.0));
    }

    #[test]
    fn lexical_model_setter_roundtrip() {
        let mut decoder = fixture_decoder();
        assert!(decoder.lexical_model().is_none());
        let model = crate::lexical::load(
            &std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../../data/tiger_sentence.lexical.bin"),
        )
        .expect("load lexical model");
        decoder.set_lexical_model(Some(model));
        assert!(decoder.lexical_model().is_some());
    }

    #[test]
    fn locked_decode_replays_opaque_prefix_neutrally() {
        let dir =
            std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../goldens/lexicon");
        let lexicon = Lexicon::load(std::slice::from_ref(&dir), 1500);
        let supplement = Supplement::load_default(Some(&dir));
        let mut decoder = Decoder::new(lexicon, supplement, None);
        // 锁文本与任何码表边都不对应（文本级退格产生的"不透明"锁）：
        // 参照 12d2ecc 起以中立码证据重放，而不是整段拒绝。
        let lock = DecodeLock {
            raw: "ab",
            text: "某某",
            boundaries: "2,6;",
        };
        let locked = decoder
            .decode_with_lock("abab", false, "", Some(lock))
            .expect("locked decode");
        assert!(!locked.items.is_empty());
        assert!(
            locked
                .items
                .iter()
                .all(|item| item.text.starts_with("某某")),
            "{:?}",
            locked
                .items
                .iter()
                .map(|item| &item.text)
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn parse_boundaries_matches_gmatch() {
        assert_eq!(parse_boundaries("2,3;"), vec![(2, 3)]);
        assert_eq!(parse_boundaries("2,3;4,6;"), vec![(2, 3), (4, 6)]);
        assert_eq!(parse_boundaries(""), Vec::<(usize, usize)>::new());
        assert_eq!(parse_boundaries("abc"), Vec::<(usize, usize)>::new());
        assert_eq!(parse_boundaries("2,;"), Vec::<(usize, usize)>::new());
        assert_eq!(parse_boundaries("2,3"), Vec::<(usize, usize)>::new());
        assert_eq!(parse_boundaries("x2,3;"), vec![(2, 3)]);
        // gmatch 语义：失败起点右移重试
        assert_eq!(parse_boundaries("12,34,56;"), vec![(34, 56)]);
        assert_eq!(parse_boundaries("1,2,3;"), vec![(2, 3)]);
    }
}
