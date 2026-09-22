// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! 不透明的哈希容器：公开 API 只暴露本模块的 `Map` / `Set`，
//! 底层哈希库与其哈希器**不属对外契约**，跨 crate 调用方无需匹配该库的版本。
//!
//! 语义与直接用底层容器**逐位一致**：同一实现、同一默认哈希器（含其按实例取随机种子的
//! 迭代序语义——依赖顺序处一律先排序，替换前后皆然）、同一增删查与 `Debug` 输出。
//! 故此处**不**改用 `std::collections`：那会换掉哈希器（短键更慢）并改变 `Debug` 之外的
//! 一切与哈希有关的可观测细节，属行为变化而非风格改动。
//!
//! 方法按**实际调用方**逐个增补；
//! 无调用者的方法不暴露，避免公开面重新膨胀。迭代器一律以 `impl Iterator` 返回，
//! 故底层迭代器类型也不出现在公开 API 里。

use std::borrow::Borrow;
use std::fmt;
use std::hash::Hash;
use std::ops::Index;

/// 键值映射（内部为带默认哈希器的 `hashbrown::HashMap`）。
#[derive(Clone)]
pub struct Map<K, V> {
    entries: hashbrown::HashMap<K, V>,
}

/// 元素集合（内部为带默认哈希器的 `hashbrown::HashSet`）。
#[derive(Clone)]
pub struct Set<T> {
    entries: hashbrown::HashSet<T>,
}

impl<K, V> Map<K, V> {
    /// 空映射。
    pub fn new() -> Self {
        Self {
            entries: hashbrown::HashMap::new(),
        }
    }
}

impl<K: Hash + Eq, V> Map<K, V> {
    /// 按键取值。
    pub fn get<Q>(&self, key: &Q) -> Option<&V>
    where
        K: Borrow<Q>,
        Q: Hash + Eq + ?Sized,
    {
        self.entries.get(key)
    }

    /// 缺省插入并返回值的可变引用（等价底层容器的 `entry(key).or_default()`，
    /// 但不暴露底层 `Entry` 类型）。
    pub fn entry_or_default(&mut self, key: K) -> &mut V
    where
        V: Default,
    {
        self.entries.entry(key).or_default()
    }

    /// 键是否存在。
    pub fn contains_key<Q>(&self, key: &Q) -> bool
    where
        K: Borrow<Q>,
        Q: Hash + Eq + ?Sized,
    {
        self.entries.contains_key(key)
    }

    /// 插入，返回被替换的旧值。
    pub fn insert(&mut self, key: K, value: V) -> Option<V> {
        self.entries.insert(key, value)
    }

    /// 按底层迭代序遍历键。
    pub fn keys(&self) -> impl Iterator<Item = &K> {
        self.entries.keys()
    }

    /// 按底层迭代序遍历键值对。
    pub fn iter(&self) -> impl Iterator<Item = (&K, &V)> {
        self.entries.iter()
    }

    /// 元素个数。
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// 清空。
    pub fn clear(&mut self) {
        self.entries.clear();
    }
}

impl<T: Hash + Eq> Set<T> {
    /// 空集合。
    pub fn new() -> Self {
        Self {
            entries: hashbrown::HashSet::new(),
        }
    }

    /// 元素是否存在。
    pub fn contains<Q>(&self, value: &Q) -> bool
    where
        T: Borrow<Q>,
        Q: Hash + Eq + ?Sized,
    {
        self.entries.contains(value)
    }

    /// 插入，返回是否为新元素。
    pub fn insert(&mut self, value: T) -> bool {
        self.entries.insert(value)
    }

    /// 元素个数。
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

impl<K, V> Default for Map<K, V> {
    fn default() -> Self {
        Self::new()
    }
}

// `Set::new` 的 clippy 伙伴（`new_without_default`）；其余无调用者的方法不暴露。
impl<T: Hash + Eq> Default for Set<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<K: Hash + Eq, V> FromIterator<(K, V)> for Map<K, V> {
    fn from_iter<I: IntoIterator<Item = (K, V)>>(iter: I) -> Self {
        Self {
            entries: iter.into_iter().collect(),
        }
    }
}

impl<K: Hash + Eq, V, const N: usize> From<[(K, V); N]> for Map<K, V> {
    fn from(entries: [(K, V); N]) -> Self {
        entries.into_iter().collect()
    }
}

impl<K: Hash + Eq, Q: Hash + Eq + ?Sized, V> Index<&Q> for Map<K, V>
where
    K: Borrow<Q>,
{
    type Output = V;

    fn index(&self, key: &Q) -> &V {
        &self.entries[key]
    }
}

// `Debug` 直接转发底层实现：输出格式与替换前**逐字一致**（金样/快照不受影响）。
impl<K: fmt::Debug, V: fmt::Debug> fmt::Debug for Map<K, V> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.entries.fmt(formatter)
    }
}

impl<T: fmt::Debug> fmt::Debug for Set<T> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.entries.fmt(formatter)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn map_matches_wrapped_container_semantics() {
        let mut map = Map::new();
        assert!(map.is_empty());
        assert_eq!(map.insert("甲".to_string(), 1usize), None);
        assert_eq!(map.insert("甲".to_string(), 2), Some(1));
        assert_eq!(map.get("甲"), Some(&2));
        assert!(map.contains_key("甲"));
        assert_eq!(map["甲"], 2);
        assert_eq!(map.len(), 1);
        map.clear();
        assert!(map.is_empty());
    }

    #[test]
    fn iteration_and_debug_follow_the_wrapped_container() {
        let entries = [
            ("甲".to_string(), 1usize),
            ("乙".to_string(), 2),
            ("丙".to_string(), 3),
        ];
        let map: Map<String, usize> = entries.clone().into_iter().collect();
        let raw: hashbrown::HashMap<String, usize> = entries.into_iter().collect();
        // 迭代**序**不是契约（底层默认哈希器按实例取随机种子，两个实例序不同；
        // 本仓依赖顺序的读取处一律先按键排序）。此处断言迭代**内容**与单元素
        // `Debug` 逐字一致（后者即底层实现的格式）。
        let mut keys: Vec<&String> = map.keys().collect();
        keys.sort();
        let mut raw_keys: Vec<&String> = raw.keys().collect();
        raw_keys.sort();
        assert_eq!(keys, raw_keys, "键集合一致");
        let mut pairs: Vec<(&String, &usize)> = map.iter().collect();
        pairs.sort();
        let mut raw_pairs: Vec<(&String, &usize)> = raw.iter().collect();
        raw_pairs.sort();
        assert_eq!(pairs, raw_pairs, "键值对集合一致");

        let single: Map<String, usize> = [("甲".to_string(), 1usize)].into_iter().collect();
        let raw_single: hashbrown::HashMap<String, usize> =
            [("甲".to_string(), 1usize)].into_iter().collect();
        assert_eq!(format!("{single:?}"), format!("{raw_single:?}"));

        let mut set = Set::new();
        let mut raw_set = hashbrown::HashSet::new();
        set.insert("甲".to_string());
        raw_set.insert("甲".to_string());
        assert_eq!(format!("{set:?}"), format!("{raw_set:?}"));
    }

    #[test]
    fn set_semantics_and_array_conversion() {
        let mut set = Set::new();
        assert!(set.insert("甲".to_string()));
        assert!(!set.insert("甲".to_string()));
        assert!(set.contains("甲"));
        assert_eq!(set.len(), 1);
        assert!(!set.is_empty());

        let from_array = Map::from([("甲".to_string(), true)]);
        let from_iter: Map<String, bool> = [("甲".to_string(), true)].into_iter().collect();
        assert_eq!(from_array.len(), from_iter.len());
        assert_eq!(from_array.get("甲"), from_iter.get("甲"));
    }
}
