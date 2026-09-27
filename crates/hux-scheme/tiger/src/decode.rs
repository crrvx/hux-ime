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
//!
//! 本文件只保留模块文档、共享类型与门面 re-export；实现按子系统下放到 `decode/`：
//! `beam`（`Decoder` 主 impl：锁播种、beam 扩展、桶聚合、评分与候选发射）、`fusion`
//! （跨来源融合）、`evidence`（`Decoder` 第二个 impl：早提交证据）、`reachability`
//! （显式 rank / 前缀约束下的可达性与资格判定）。两个 `impl Decoder` 的分工见各自
//! 模块文档：主 impl 推进解码状态并发射候选，第二个 impl 只在发射后按候选池与
//! arena 纯计算早提交证据。

use crate::lexical::{self, LexicalModel};
use crate::lexicon::{CodeEntry, LEXICAL_FILE, Lexicon, LexiconOptions, Supplement};
use crate::ngram::{BOS_CHAR as BOS, EOS_CHAR as EOS, MobileModel};
use anyhow::Result;
use hashbrown::{HashMap, HashSet};
use hux_core::collections::{Map, Set};
use hux_core::learning::{DiffItem, DiffPathNode, LearningIndex};
use hux_core::punct::{PairState, PunctTable};
use hux_core::session::Candidate;
use std::path::PathBuf;

mod beam;
mod evidence;
mod fusion;
mod reachability;

#[cfg(test)]
mod tests;

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
/// 学习候选补充上限：截断时在已选结果外最多补入这么多个有学习潜力的候选。
const TRUNCATED_LEARNING_ADDITION_LIMIT: usize = 4;
/// 早提交最低份额（参照 `early_commit_minimum_share`）：**唯一来源**，
/// 交互层（`interaction::early_commit`）引用它。
pub(crate) const EARLY_COMMIT_MINIMUM_SHARE: f64 = 0.99;
/// 已闭合边界的早提交份额阈值（参照 `early_commit_closed_boundary_share`）。
///
/// 与 [`RankingPriorParameters::empty_code_strong_share`]（同为 `0.99999`）**语义不同**：
/// 这里判「单字边界是否已闭合」，那里判「空码候选是否强置信」；同值属巧合，改值即改行为。
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
    ///
    /// 与 `EARLY_COMMIT_CLOSED_BOUNDARY_SHARE`（同为 `0.99999`）**语义不同**：本项由
    /// `interaction::early_commit` 读取，判「空码候选是否强置信」；同值属巧合，改值即改行为。
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
    /// 冻结标记：只由 `dedup_limit` 置位、也只被它的提前返回消费；`add_state` 的调用点
    /// 都作用于尚未裁剪的桶（`new_states` 的全新桶 / `consumed_end` 恒大于当前展开位），故不会遇到冻结桶。
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

// ---------------------------------------------------------------- 门面 re-export

pub use reachability::has_complete_candidate;
pub(crate) use reachability::has_selection_suffix;
