// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! 有界 FIFO 缓存，对应参照实现 `lua/tiger_sentence_cache.lua`。
//!
//! 两种形态与 Lua 一一对应：
//! * [`Fifo`] —— 通用 key→value 记忆化（`M.new` / `M.put`）；
//! * [`Columns`] —— 定宽列记录槽位分配器（`M.new_columns` / `M.put_columns`），
//!   列数据由调用方持有。
//!
//! 语义要点：缓存的 `false`/零值是真实值，不是未命中；淘汰严格按插入序（FIFO），
//! 槽位循环复用。

use hashbrown::HashMap;
use std::hash::Hash;

/// `M.new` + `M.put`：key→value 的 FIFO 记忆化。
#[derive(Clone)]
pub struct Fifo<K, V> {
    values: HashMap<K, V>,
    /// 槽位 i（0 基）保存写入槽位 i+1 的 key；`len()` 即 Lua 的 `#keys`。
    keys: Vec<Option<K>>,
    /// 下一个新 key 使用的槽位（1 基，对应 Lua `cache.next`）。
    next: usize,
    limit: usize,
}

impl<K: Clone + Eq + Hash, V> Fifo<K, V> {
    pub fn new(limit: usize) -> Self {
        assert!(limit >= 1, "invalid cache limit");
        Self {
            values: HashMap::new(),
            keys: Vec::new(),
            next: 1,
            limit,
        }
    }

    pub fn get(&self, key: &K) -> Option<&V> {
        self.values.get(key)
    }

    /// 写入并返回引用；已存在的 key 原地更新，不占新槽位。
    pub fn put(&mut self, key: K, value: V) -> &V {
        if let Some(slot) = self.values.get_mut(&key) {
            *slot = value;
            return self.values.get(&key).expect("just inserted");
        }
        let slot = self.next;
        if let Some(Some(old)) = self.keys.get(slot - 1).cloned() {
            self.values.remove(&old);
        }
        if slot > self.keys.len() {
            self.keys.push(Some(key.clone()));
        } else {
            self.keys[slot - 1] = Some(key.clone());
        }
        self.values.insert(key.clone(), value);
        self.next = slot % self.limit + 1;
        self.values.get(&key).expect("just inserted")
    }

    /// Lua `#keys`：已占用槽位数（达到上限后恒为 limit）。
    // 只按 Lua `#keys` 语义暴露长度：调用方（`ngram` 的统计）都只问长度，
    // 因此不提供 `is_empty`，与 clippy 的 len/is_empty 配对约定有意不同。
    #[allow(clippy::len_without_is_empty)]
    pub fn len(&self) -> usize {
        self.keys.len()
    }

    pub fn values(&self) -> impl Iterator<Item = &V> {
        self.values.values()
    }

    pub fn clear(&mut self) {
        self.values.clear();
        self.keys.clear();
        self.next = 1;
    }
}

/// `M.new_columns` / `M.put_columns`：只负责槽位分配，列数据由调用方按
/// `slot - 1` 索引。`false` 一类的“已判定不存在”值由调用方用 `Option` 表达。
pub struct Columns<K> {
    map: HashMap<K, usize>,
    keys: Vec<Option<K>>,
    next: usize,
    limit: usize,
}

impl<K: Clone + Eq + Hash> Columns<K> {
    pub fn new(limit: usize) -> Self {
        assert!(limit >= 1, "invalid cache limit");
        Self {
            map: HashMap::new(),
            keys: Vec::new(),
            next: 1,
            limit,
        }
    }

    /// 已缓存 key 的槽位（1 基）。
    pub fn slot(&self, key: &K) -> Option<usize> {
        self.map.get(key).copied()
    }

    /// 返回已有槽位，或分配新槽位（必要时按 FIFO 淘汰最旧槽位）。
    pub fn claim(&mut self, key: K) -> usize {
        if let Some(slot) = self.map.get(&key) {
            return *slot;
        }
        let slot = self.next;
        if let Some(Some(old)) = self.keys.get(slot - 1).cloned() {
            self.map.remove(&old);
        }
        if slot > self.keys.len() {
            self.keys.push(Some(key.clone()));
        } else {
            self.keys[slot - 1] = Some(key.clone());
        }
        self.map.insert(key, slot);
        self.next = slot % self.limit + 1;
        slot
    }

    /// Lua `#keys`。
    // 同 `Fifo::len`：只问长度，不提供 `is_empty`。
    #[allow(clippy::len_without_is_empty)]
    pub fn len(&self) -> usize {
        self.keys.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Fifo 命中已存在的 key 时原地换值：值更新、槽位不推进也不新增。
    #[test]
    fn fifo_updates_existing_key_in_place() {
        let mut cache = Fifo::new(2);
        cache.put(10u32, "a");
        cache.put(20u32, "b");
        assert_eq!(cache.get(&10), Some(&"a"), "未满员时写入即可读回：10 → a");
        assert_eq!(cache.len(), 2, "两个不同 key 各占一槽");
        // 已存在的 key 原地更新，不推进槽位。
        cache.put(10u32, "a2");
        assert_eq!(cache.get(&10), Some(&"a2"), "原地更新换值不换槽：10 → a2");
        assert_eq!(cache.len(), 2, "原地更新不得新增槽位");
    }

    /// Fifo 满员后写入新 key 顶掉最旧槽位，容量恒等于 limit：淘汰只换内容，不改变槽位数。
    #[test]
    fn fifo_evicts_oldest_slot() {
        let mut cache = Fifo::new(2);
        cache.put(10u32, "a");
        cache.put(20u32, "b");
        // 新 key 顶掉最旧槽位（10）。
        cache.put(30u32, "c");
        assert_eq!(
            cache.get(&10),
            None,
            "limit=2 时第 3 个 key 必须顶掉最旧的 10"
        );
        assert_eq!(cache.get(&20), Some(&"b"), "未被淘汰的 20 仍在缓存");
        assert_eq!(cache.get(&30), Some(&"c"), "刚写入的 30 必须命中");
        assert_eq!(cache.len(), 2, "淘汰只换内容，槽位数恒等于 limit");
    }

    /// Columns 的 claim 对同一 key 幂等（回原槽且不推进游标）；槽位用尽后循环复用，被顶掉的旧 key 随即失效。
    #[test]
    fn columns_claim_and_evict() {
        let mut columns = Columns::new(2);
        assert_eq!(columns.claim(7u64), 1, "首次 claim 分到 1 号槽");
        assert_eq!(
            columns.claim(7u64),
            1,
            "已分配 key 重复 claim 必须回到原槽且不推进游标"
        );
        assert_eq!(columns.claim(8u64), 2, "新 key 依次占用 2 号槽");
        assert_eq!(columns.len(), 2, "两个 key 占满两槽");
        assert_eq!(
            columns.claim(9u64),
            1,
            "槽位循环复用：第 3 个 key 回到 1 号槽"
        ); // 槽位循环：顶掉 7
        assert_eq!(columns.slot(&7), None, "被顶掉的 7 必须查不到");
        assert_eq!(columns.slot(&9), Some(1), "1 号槽此时由 9 占用");
    }
}
