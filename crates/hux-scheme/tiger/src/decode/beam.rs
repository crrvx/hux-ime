// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! beam 解码：`Decoder` 的主实现——锁播种、beam 扩展、桶聚合、评分与候选发射。
//!
//! 另含扩展循环与发射阶段专用的工具（`StateView`、`select_top`、`logsumexp`、
//! `beam_limit_at`、`normalize`、`parse_boundaries`、`parse_selector`、`segmented_from_path`）
//! 与学习奖励 `learning_reward`。

use super::fusion::apply_fusion_ordering;
use super::reachability::eligible_candidates;
use super::*;

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
                .map(|directory| directory.join(LEXICAL_FILE))
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

    /// 音反查候选（含虎码注释过滤；上限 [`CANDIDATE_LIMIT`]）。
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
            CANDIDATE_LIMIT,
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

    /// 应用高频上限与字集开关：重建词库索引（同 [`Decoder::apply_high_freq_limit`] 的路径，
    /// 两者一起落位只重建一次）。
    pub fn apply_lexicon_options(&mut self, limit: usize, options: LexiconOptions) {
        self.lexicon.apply_lexicon_options(limit, options);
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
                ..State::neutral()
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
        let root = State::neutral();
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

    pub(super) fn dedup_limit(&mut self, mut bucket: Bucket, limit: usize) -> Bucket {
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
                if added == TRUNCATED_LEARNING_ADDITION_LIMIT {
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
                            ..State::neutral()
                        };
                        self.add_state(&mut states[consumed_end], state);
                    }
                }
            }
        }
        Ok(())
    }

    pub(super) fn evaluate_state(&mut self, index: usize) -> Result<Evaluated> {
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
    /// 中性初值：根状态与两个发射构造点的公共起点（`prev2`/`prev1` 为句首哨兵、
    /// 无来源标记、无隔离项、空文本）；各构造点只覆写自己的字段。
    fn neutral() -> Self {
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

pub(super) fn logsumexp(left: f64, right: f64) -> f64 {
    let maximum = left.max(right);
    maximum + ((left - maximum).exp() + (right - maximum).exp()).ln()
}

pub(super) fn beam_limit_at(raw_length: usize) -> usize {
    if raw_length > LONG_INPUT_FULL_BEAM_LENGTH {
        LONG_INPUT_BEAM_WIDTH
    } else {
        BEAM_WIDTH
    }
}

/// 参照 `normalize`：ASCII 小写化并去除 Lua `%s` 空白（含垂直制表符）。
pub(super) fn normalize(raw: &str) -> Vec<u8> {
    let mut out = Vec::with_capacity(raw.len());
    for byte in raw.bytes() {
        if byte.is_ascii_whitespace() || byte == 0x0b {
            continue;
        }
        out.push(byte.to_ascii_lowercase());
    }
    out
}

pub(super) fn has_letter(raw: &[u8]) -> bool {
    raw.iter().any(|byte| byte.is_ascii_alphabetic())
}

/// 参照 `locked.boundaries:gmatch("(%d+),(%d+);")`（失败起点逐一右移重试）。
pub(super) fn parse_boundaries(value: &str) -> Vec<(usize, usize)> {
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
pub(super) fn parse_selector(raw: &[u8], code_end: usize) -> (u64, usize) {
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
