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
pub struct Fifo<K, V> {
    values: HashMap<K, V>,
    /// 槽位 i（0 基）保存写入槽位 i+1 的 key；`len()` 即 Lua 的 `#keys`。
    keys: Vec<Option<K>>,
    /// 下一个新 key 使用的槽位（1 基，对应 Lua `cache.next`）。
    next: usize,
    limit: usize,
}

impl<K: Copy + Eq + Hash, V> Fifo<K, V> {
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
        if let Some(Some(old)) = self.keys.get(slot - 1).copied() {
            self.values.remove(&old);
        }
        if slot > self.keys.len() {
            self.keys.push(Some(key));
        } else {
            self.keys[slot - 1] = Some(key);
        }
        self.values.insert(key, value);
        self.next = slot % self.limit + 1;
        self.values.get(&key).expect("just inserted")
    }

    /// Lua `#keys`：已占用槽位数（达到上限后恒为 limit）。
    pub fn len(&self) -> usize {
        self.keys.len()
    }

    pub fn is_empty(&self) -> bool {
        self.keys.is_empty()
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

impl<K: Copy + Eq + Hash> Columns<K> {
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
        if let Some(Some(old)) = self.keys.get(slot - 1).copied() {
            self.map.remove(&old);
        }
        if slot > self.keys.len() {
            self.keys.push(Some(key));
        } else {
            self.keys[slot - 1] = Some(key);
        }
        self.map.insert(key, slot);
        self.next = slot % self.limit + 1;
        slot
    }

    /// Lua `#keys`。
    pub fn len(&self) -> usize {
        self.keys.len()
    }

    pub fn is_empty(&self) -> bool {
        self.keys.is_empty()
    }

    pub fn clear(&mut self) {
        self.map.clear();
        self.keys.clear();
        self.next = 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fifo_evicts_in_insertion_order_and_updates_in_place() {
        let mut cache = Fifo::new(2);
        cache.put(10u32, "a");
        cache.put(20u32, "b");
        assert_eq!(cache.get(&10), Some(&"a"));
        assert_eq!(cache.len(), 2);
        // 已存在的 key 原地更新，不推进槽位。
        cache.put(10u32, "a2");
        assert_eq!(cache.get(&10), Some(&"a2"));
        assert_eq!(cache.len(), 2);
        // 新 key 顶掉最旧槽位（10）。
        cache.put(30u32, "c");
        assert_eq!(cache.get(&10), None);
        assert_eq!(cache.get(&20), Some(&"b"));
        assert_eq!(cache.get(&30), Some(&"c"));
        assert_eq!(cache.len(), 2);
    }

    #[test]
    fn columns_claim_and_evict() {
        let mut columns = Columns::new(2);
        assert_eq!(columns.claim(7u64), 1);
        assert_eq!(columns.claim(7u64), 1);
        assert_eq!(columns.claim(8u64), 2);
        assert_eq!(columns.len(), 2);
        assert_eq!(columns.claim(9u64), 1); // 槽位循环：顶掉 7
        assert_eq!(columns.slot(&7), None);
        assert_eq!(columns.slot(&9), Some(1));
    }
}
