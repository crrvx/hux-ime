// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! 稀疏索引与分页缓存：页级计数器、`Index`、`PageCache`、上下文槽。

use anyhow::Result;
use hashbrown::HashMap;
use hux_core::cache::Fifo;
use std::collections::VecDeque;
use std::rc::Rc;

use super::INDEX_PAGE_RECORDS;
use super::binary::read_at;

#[derive(Default)]
pub(super) struct Counters {
    pub(super) page_misses: u64,
    pub(super) page_bytes: u64,
    pub(super) index_misses: u64,
    pub(super) index_bytes_read: u64,
}

/// 稀疏索引：常驻（≤1 页）或“目录 + 分页 FIFO 缓存”。
pub(super) struct Index {
    pub(super) offset: u64,
    pub(super) count: usize,
    pub(super) pages: usize,
    pub(super) cache: Fifo<u32, Rc<Vec<u8>>>,
    pub(super) resident: Option<Rc<Vec<u8>>>,
    pub(super) directory: Option<Rc<Vec<u8>>>,
}

impl Index {
    pub(super) fn resident_len(&self) -> u64 {
        match (&self.resident, &self.directory) {
            (Some(data), _) | (_, Some(data)) => data.len() as u64,
            _ => 0,
        }
    }

    pub(super) fn cached_bytes(&self) -> u64 {
        self.cache.values().map(|data| data.len() as u64).sum()
    }
}

pub(super) fn open_index(
    map: &[u8],
    offset: u64,
    count: usize,
    index_limit: usize,
) -> Result<Index> {
    let pages = count.div_ceil(INDEX_PAGE_RECORDS);
    let mut index = Index {
        offset,
        count,
        pages,
        cache: Fifo::new(index_limit),
        resident: None,
        directory: None,
    };
    if count <= INDEX_PAGE_RECORDS {
        index.resident = Some(Rc::new(read_at(map, offset, count * 16)?.to_vec()));
    } else {
        // 只常驻每页首个 key，替换 Lua 参照里的整段索引字符串。
        let mut keys = Vec::with_capacity(pages * 8);
        for page in 0..pages {
            let at = offset + (page * INDEX_PAGE_RECORDS * 16) as u64;
            keys.extend_from_slice(read_at(map, at, 8)?);
        }
        index.directory = Some(Rc::new(keys));
    }
    Ok(index)
}

pub(super) struct PageCache {
    pub(super) map: HashMap<(u8, u32), Rc<Vec<u8>>>,
    pub(super) order: VecDeque<(u8, u32)>,
    pub(super) bytes: usize,
    pub(super) limit: usize,
}

impl PageCache {
    pub(super) fn new(limit: usize) -> Self {
        Self {
            map: HashMap::new(),
            order: VecDeque::new(),
            bytes: 0,
            limit,
        }
    }

    pub(super) fn get(&mut self, kind: u8, page: u32) -> Option<Rc<Vec<u8>>> {
        let data = self.map.get(&(kind, page))?.clone();
        if let Some(position) = self.order.iter().position(|&entry| entry == (kind, page)) {
            self.order.remove(position);
        }
        self.order.push_back((kind, page));
        Some(data)
    }

    pub(super) fn insert(&mut self, kind: u8, page: u32, data: Rc<Vec<u8>>) {
        if self.map.insert((kind, page), data.clone()).is_none() {
            self.bytes += data.len();
        }
        self.order.push_back((kind, page));
        while self.bytes > self.limit && self.order.len() > 1 {
            let victim = self.order.pop_front().expect("non-empty");
            if let Some(bytes) = self.map.remove(&victim) {
                self.bytes -= bytes.len();
            }
        }
    }
}

#[derive(Clone, Copy)]
pub(super) struct CtxSlot {
    pub(super) page: Option<u32>,
    pub(super) position: usize,
    pub(super) count: u32,
    pub(super) lambda: f64,
}

impl Default for CtxSlot {
    fn default() -> Self {
        Self {
            page: None,
            position: 0,
            count: 0,
            lambda: 1.0,
        }
    }
}
