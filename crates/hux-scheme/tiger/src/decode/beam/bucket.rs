// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! 桶聚合与裁剪：状态入桶、聚合、去重上限与取顶。

use super::*;

impl Decoder {
    pub(super) fn add_state(&mut self, bucket: &mut Bucket, state: State) {
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

    pub(super) fn ensure_aggregated(&mut self, bucket: &mut Bucket) {
        if bucket.aggregated {
            return;
        }
        let items = std::mem::take(&mut bucket.items);
        for item in items {
            self.add_aggregated(bucket, item);
        }
        bucket.aggregated = true;
    }

    pub(in crate::decode) fn dedup_limit(&mut self, mut bucket: Bucket, limit: usize) -> Bucket {
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
