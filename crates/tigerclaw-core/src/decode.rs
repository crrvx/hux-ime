//! Beam 解码（冷路径），对应参照 `decode_full` / `decode` 的去缓存形态。
//!
//! 本增量范围：normalize、rank 选择器、资格过滤、beam 扩展、桶聚合、评分与候选发射。
//! 暂不含：早提交证据、学习集成、增量/锁缓存、模型失败回退（guarded_decode）。

use crate::lexicon::{CodeEntry, Lexicon, Supplement};
use crate::ngram::MobileModel;
use anyhow::Result;
use hashbrown::{HashMap, HashSet};

pub const BOS: char = '\u{2}';
pub const EOS: char = '\u{3}';

const BEAM_WIDTH: usize = 200;
const LONG_INPUT_FULL_BEAM_LENGTH: usize = 24;
const LONG_INPUT_BEAM_WIDTH: usize = 48;
const CANDIDATE_LIMIT: usize = 20;
const RANK_PENALTY: f64 = 0.03;
const EMITTED_CHARACTER_REWARD: f64 = 2.0;
const WHOLE_INPUT_SINGLE_CHARACTER_REWARD: f64 = 5.0;
const ISOLATION_THRESHOLD: usize = 3000;
const ISOLATION_LAMBDA: f64 = 2.0;
const AGGREGATE_DURING_EXPANSION_THRESHOLD: usize = 128;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Comparator {
    RankFirst,
    ScoreFirst,
    NoModel,
}

struct State {
    score: f64,
    mass_score: f64,
    text: String,
    prev2: char,
    prev1: char,
    max_rank: usize,
    supplement_state: usize,
    supplement_score: f64,
    previous: Option<usize>,
    edge_chars: Vec<char>,
    /// 字节长度；当前增量未读取，供学习/证据增量使用。
    #[allow(dead_code)]
    text_length: usize,
    raw_length: usize,
    edge_count: usize,
    learning_score: f64,
    learning_potential: f64,
    isolation_penalty: Option<f64>,
    isolation_last_char: Option<char>,
    isolation_last_isolated: bool,
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
    pub max_rank: usize,
    pub supplement_score: f64,
    pub learning_score: f64,
    pub edge_count: usize,
    pub path: usize,
    pub segmented: String,
}

#[derive(Debug)]
pub struct DecodeOutput {
    pub items: Vec<Evaluated>,
    pub learning_affected: bool,
    pub completed_truncated: bool,
}

/// 解码器：持有数据与可选 n-gram 模型（`None` = 无模型回退）。
pub struct Decoder {
    lexicon: Lexicon,
    supplement: Supplement,
    model: Option<MobileModel>,
    rank_of: Option<HashMap<char, usize>>,
    arena: Vec<State>,
    allow_duplicate_single: bool,
}

impl Decoder {
    pub fn new(lexicon: Lexicon, supplement: Supplement, model: Option<MobileModel>) -> Self {
        let rank_of = lexicon.character_ranks.as_ref().map(|ranks| {
            ranks
                .iter()
                .filter_map(|(text, rank)| text.chars().next().map(|ch| (ch, *rank)))
                .collect()
        });
        Self {
            lexicon,
            supplement,
            model,
            rank_of,
            arena: Vec::new(),
            allow_duplicate_single: true,
        }
    }

    pub fn lexicon(&self) -> &Lexicon {
        &self.lexicon
    }

    pub fn model(&self) -> Option<&MobileModel> {
        self.model.as_ref()
    }

    pub fn set_allow_duplicate_single(&mut self, allowed: bool) {
        self.allow_duplicate_single = allowed;
    }

    /// 参照 `decode(raw_code, include_early_commit=false, nil, nil)` 的冷路径。
    pub fn decode(&mut self, raw_code: &str) -> Result<DecodeOutput> {
        let raw = normalize(raw_code);
        if raw.is_empty() || !has_letter(&raw) {
            return Ok(DecodeOutput {
                items: Vec::new(),
                learning_affected: false,
                completed_truncated: false,
            });
        }
        self.arena.clear();
        let length = raw.len();
        let mut states = self.new_states(length);
        self.expand_range(&raw, &mut states, 0, length, -1)?;
        self.emit(&raw, &mut states, length)
    }

    fn new_states(&mut self, length: usize) -> Vec<Bucket> {
        let mut states: Vec<Bucket> = (0..=length).map(|_| Bucket::default()).collect();
        let root = State {
            score: 0.0,
            mass_score: 0.0,
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
            isolation_penalty: None,
            isolation_last_char: None,
            isolation_last_isolated: false,
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
                if self.duplicate_better(item, previous) {
                    bucket.best.insert(text.clone(), item);
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
                        // 学习集成未接入：参照的 `learned` 即 previous.learning_score，
                        // 因此净增量为 0；此处保留公式形状。
                        let learned = item.learning_score;
                        let state = State {
                            score: score + learned - item.learning_score,
                            mass_score,
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
                            learning_score: 0.0,
                            learning_potential: 0.0,
                            isolation_penalty: None,
                            isolation_last_char: None,
                            isolation_last_isolated: false,
                        };
                        self.add_state(&mut states[consumed_end], state);
                    }
                }
            }
        }
        Ok(())
    }

    fn evaluate_state(&mut self, index: usize) -> Result<Evaluated> {
        let ending_adjustment = self.logp(self.arena[index].prev2, self.arena[index].prev1, EOS)?
            - self.path_isolation_penalty(index)?;
        let state = &self.arena[index];
        Ok(Evaluated {
            text: state.text.clone(),
            score: state.score + ending_adjustment,
            confidence_score: state.mass_score + ending_adjustment,
            max_rank: state.max_rank.max(1),
            supplement_score: state.supplement_score,
            learning_score: state.learning_score,
            edge_count: state.edge_count,
            path: index,
            segmented: String::new(),
        })
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
        let (mut last_char, mut last_isolated) = match previous {
            Some(previous) => (
                self.arena[previous].isolation_last_char,
                self.arena[previous].isolation_last_isolated,
            ),
            None => (None, false),
        };
        let edge_chars = self.arena[index].edge_chars.clone();
        for ch in edge_chars {
            let rank = self.rank_of_char(ch);
            let rare = rank > ISOLATION_THRESHOLD;
            let mut linked = false;
            if let Some(last) = last_char
                && (last_isolated || rare)
            {
                linked = self.has_observed_bigram(last, ch)?;
            }
            if last_isolated && linked {
                penalty -= ISOLATION_LAMBDA;
            }
            last_isolated = rare && !linked;
            if last_isolated {
                penalty += ISOLATION_LAMBDA;
            }
            last_char = Some(ch);
        }
        self.arena[index].isolation_penalty = Some(penalty);
        self.arena[index].isolation_last_char = last_char;
        self.arena[index].isolation_last_isolated = last_isolated;
        Ok(penalty)
    }

    fn rank_of_char(&self, ch: char) -> usize {
        self.rank_of
            .as_ref()
            .and_then(|ranks| ranks.get(&ch).copied())
            .unwrap_or(self.lexicon.unknown_character_rank)
    }

    fn emit(&mut self, raw: &[u8], states: &mut [Bucket], length: usize) -> Result<DecodeOutput> {
        let completed =
            self.dedup_limit(std::mem::take(&mut states[length]), beam_limit_at(length));
        states[length] = completed;
        let candidates: Vec<usize> = states[length].items.clone();
        let mut evaluated = Vec::with_capacity(candidates.len());
        for index in candidates {
            evaluated.push(self.evaluate_state(index)?);
        }
        // 参照 `emit`：无模型 → NoModel；有模型 → prefer_score 时 ScoreFirst，
        // 否则 RankFirst（注意与 `current_state_comparator` 的 allow_dup 分支不同）。
        let mut comparator = if self.model.is_none() {
            Comparator::NoModel
        } else if self.prefer_score_over_lexicon_rank(&evaluated) {
            Comparator::ScoreFirst
        } else {
            Comparator::RankFirst
        };
        if evaluated.iter().any(|item| item.learning_score > 0.0) {
            comparator = Comparator::ScoreFirst;
        }
        evaluated.sort_by(|left, right| {
            if Decoder::state_better(comparator, left, right) {
                std::cmp::Ordering::Less
            } else {
                std::cmp::Ordering::Greater
            }
        });
        evaluated.truncate(CANDIDATE_LIMIT);
        for item in &mut evaluated {
            item.segmented = segmented_from_path(raw, &self.arena, item.path);
        }
        Ok(DecodeOutput {
            items: evaluated,
            learning_affected: false,
            completed_truncated: states[length].truncated,
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

struct Eligible {
    text: String,
    rank: usize,
    optimal_single: bool,
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
    text: String,
    prev2: char,
    prev1: char,
    max_rank: usize,
    supplement_state: usize,
    supplement_score: f64,
    edge_count: usize,
    learning_score: f64,
}

impl State {
    fn view(&self) -> StateView {
        StateView {
            score: self.score,
            mass_score: self.mass_score,
            text: self.text.clone(),
            prev2: self.prev2,
            prev1: self.prev1,
            max_rank: self.max_rank,
            supplement_state: self.supplement_state,
            supplement_score: self.supplement_score,
            edge_count: self.edge_count,
            learning_score: self.learning_score,
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

/// 参照 `trailing_selector_span`；供后续增量路径（扩展/删减缓存）使用。
#[allow(dead_code)]
fn trailing_selector_span(raw: &[u8]) -> usize {
    let mut index = raw.len();
    while index > 0 {
        let mark = raw[index - 1];
        if mark.is_ascii_digit() || mark == b';' || mark == b'\'' {
            index -= 1;
        } else {
            break;
        }
    }
    raw.len() - index
}

/// 参照 `parse_selector`：返回 (选中 rank, 消耗到的字节位置)；0 = 无选择器。
fn parse_selector(raw: &[u8], code_end: usize) -> (u32, usize) {
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
            return (token.parse::<u32>().unwrap_or(0), digit_end + 1);
        }
        _ => {}
    }
    (0, code_end)
}

/// 参照 `eligible_candidates`。
fn eligible_candidates(
    candidates: &[CodeEntry],
    selected_rank: u32,
    whole_input_edge: bool,
    allow_duplicate_single: bool,
) -> Vec<&CodeEntry> {
    let single = |entry: &CodeEntry| entry.text.chars().count() == 1;
    if candidates.len() == 1 {
        let candidate = &candidates[0];
        if selected_rank > 0 {
            if candidate.rank as u32 == selected_rank {
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
        .filter(|entry| entry.rank as u32 == rank)
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

    #[test]
    fn normalize_matches_reference_rules() {
        assert_eq!(normalize("A B\tC\r\n"), b"abc");
        assert_eq!(normalize("a\u{0b}b"), b"ab");
        assert!(has_letter(b"a1"));
        assert!(!has_letter(b"123"));
        assert_eq!(trailing_selector_span(b"ab12;"), 3);
        assert_eq!(parse_selector(b"ab;", 2), (2, 3));
        assert_eq!(parse_selector(b"ab'", 2), (3, 3));
        assert_eq!(parse_selector(b"ab0", 2), (10, 3));
        assert_eq!(parse_selector(b"ab12", 2), (12, 4));
        assert_eq!(parse_selector(b"ab", 2), (0, 2));
    }
}
