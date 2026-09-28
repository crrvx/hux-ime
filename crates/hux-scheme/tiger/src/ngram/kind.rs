// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! 单个 n-gram 阶的上下文查找：`KindState`、失配链与二元缓存。

use anyhow::{Result, anyhow};
use hux_core::cache::{Columns, Fifo};
use std::rc::Rc;

use super::INDEX_PAGE_RECORDS;
use super::binary::{le_f32, le_u32, le_u64, read_at};
use super::state::{Counters, CtxSlot, Index, PageCache};

/// 一个 n-gram 阶的索引 + 上下文列缓存（kind = `b'b'` / `b't'`）。
pub(super) struct KindState {
    pub(super) kind: u8,
    pub(super) index: Index,
    pub(super) context: Columns<u64>,
    pub(super) slots: Vec<CtxSlot>,
    pub(super) index_stride: usize,
    pub(super) context_count: usize,
    pub(super) section_end: u64,
}

impl KindState {
    pub(super) fn reset_caches(&mut self, context_limit: usize, index_limit: usize) {
        self.context = Columns::new(context_limit);
        self.slots = vec![CtxSlot::default(); context_limit];
        self.index.cache = Fifo::new(index_limit);
    }

    fn index_page(
        &mut self,
        map: &[u8],
        counters: &mut Counters,
        page: usize,
    ) -> Result<Rc<Vec<u8>>> {
        if let Some(resident) = self.index.resident.clone() {
            return Ok(resident);
        }
        if let Some(cached) = self.index.cache.get(&(page as u32)) {
            return Ok(cached.clone());
        }
        let first = page * INDEX_PAGE_RECORDS;
        let records = INDEX_PAGE_RECORDS.min(self.index.count.saturating_sub(first));
        let data =
            Rc::new(read_at(map, self.index.offset + (first * 16) as u64, records * 16)?.to_vec());
        counters.index_misses += 1;
        counters.index_bytes_read += data.len() as u64;
        self.index.cache.put(page as u32, data.clone());
        Ok(data)
    }

    fn index_offset(&mut self, map: &[u8], counters: &mut Counters, record: usize) -> Result<u64> {
        let page = record / INDEX_PAGE_RECORDS;
        let data = self.index_page(map, counters, page)?;
        Ok(le_u64(&data, (record % INDEX_PAGE_RECORDS) * 16 + 8))
    }

    /// 最后一个 `key <= target` 的记录下标；无匹配返回 -1。
    fn find_page(&mut self, map: &[u8], counters: &mut Counters, key: u64) -> Result<i64> {
        let mut page = 0usize;
        if let Some(directory) = self.index.directory.clone() {
            let (mut low, mut high) = (0usize, self.index.pages);
            while low < high {
                let middle = low + (high - low) / 2;
                if le_u64(&directory, middle * 8) <= key {
                    low = middle + 1;
                } else {
                    high = middle;
                }
            }
            if low == 0 {
                return Ok(-1);
            }
            page = low - 1;
        }
        let first = page * INDEX_PAGE_RECORDS;
        let data = self.index_page(map, counters, page)?;
        let records = INDEX_PAGE_RECORDS.min(self.index.count - first);
        let (mut low, mut high) = (0usize, records);
        while low < high {
            let middle = low + (high - low) / 2;
            if le_u64(&data, middle * 16) <= key {
                low = middle + 1;
            } else {
                high = middle;
            }
        }
        Ok((first + low) as i64 - 1)
    }

    fn get_page(
        &mut self,
        map: &[u8],
        pages: &mut PageCache,
        counters: &mut Counters,
        page: usize,
    ) -> Result<Rc<Vec<u8>>> {
        if let Some(data) = pages.get(self.kind, page as u32) {
            return Ok(data);
        }
        let offset = self.index_offset(map, counters, page)?;
        let next_offset = if page + 1 < self.index.count {
            self.index_offset(map, counters, page + 1)?
        } else {
            self.section_end
        };
        let length = next_offset
            .checked_sub(offset)
            .and_then(|length| usize::try_from(length).ok())
            .ok_or_else(|| anyhow!("truncated mobile n-gram"))?;
        let data = Rc::new(read_at(map, offset, length)?.to_vec());
        counters.page_misses += 1;
        counters.page_bytes += data.len() as u64;
        pages.insert(self.kind, page as u32, data.clone());
        Ok(data)
    }

    fn remember_absent(&mut self, key: u64) {
        let slot = self.context.claim(key);
        self.slots[slot - 1] = CtxSlot::default();
    }

    pub(super) fn lookup_context(
        &mut self,
        map: &[u8],
        pages: &mut PageCache,
        counters: &mut Counters,
        key: u64,
        target: u32,
    ) -> Result<(f64, f64, bool)> {
        if let Some(hit) = self.cached_successors(map, pages, counters, key, target)? {
            return Ok(hit);
        }
        let page = self.find_page(map, counters, key)?;
        if page < 0 {
            self.remember_absent(key);
            return Ok((1.0, 0.0, false));
        }
        let data = self.get_page(map, pages, counters, page as usize)?;
        self.scan_page(&data, page as u32, key, target)
    }

    /// 命中上下文缓存时的后继查询（未命中返回 `None`）。
    fn cached_successors(
        &mut self,
        map: &[u8],
        pages: &mut PageCache,
        counters: &mut Counters,
        key: u64,
        target: u32,
    ) -> Result<Option<(f64, f64, bool)>> {
        let Some(slot) = self.context.slot(&key) else {
            return Ok(None);
        };
        let entry = self.slots[slot - 1];
        let Some(page) = entry.page else {
            return Ok(Some((1.0, 0.0, false)));
        };
        let data = self.get_page(map, pages, counters, page as usize)?;
        Ok(Some(lookup_successor(
            &data,
            entry.position,
            entry.count,
            entry.lambda,
            target,
        )))
    }

    /// 在页内顺序扫描上下文记录；命中后写入缓存槽。
    fn scan_page(
        &mut self,
        data: &[u8],
        page: u32,
        key: u64,
        target: u32,
    ) -> Result<(f64, f64, bool)> {
        let mut position = 0usize;
        let remaining = self.index_stride.min(
            self.context_count
                .saturating_sub(page as usize * self.index_stride),
        );
        for _ in 0..remaining {
            // 模型内部偏移/计数来自文件：畸形数据在此报错而非越界 panic。
            if position + 16 > data.len() {
                return Err(anyhow!("truncated mobile n-gram"));
            }
            let context_key = le_u64(data, position);
            let lambda = le_f32(data, position + 8) as f64;
            let successor_count = le_u32(data, position + 12);
            position += 16;
            let successors_end = (successor_count as usize)
                .checked_mul(8)
                .and_then(|length| position.checked_add(length))
                .filter(|end| *end <= data.len());
            let Some(successors_end) = successors_end else {
                return Err(anyhow!("truncated mobile n-gram"));
            };
            if context_key == key {
                let slot = self.context.claim(key);
                self.slots[slot - 1] = CtxSlot {
                    page: Some(page),
                    position,
                    count: successor_count,
                    lambda,
                };
                return Ok(lookup_successor(
                    data,
                    position,
                    successor_count,
                    lambda,
                    target,
                ));
            }
            if context_key > key {
                self.remember_absent(key);
                return Ok((1.0, 0.0, false));
            }
            position = successors_end;
        }
        self.remember_absent(key);
        Ok((1.0, 0.0, false))
    }
}

/// 后继记录二分查找（`<I4 target, f4 prob>`，共 8 字节）。
fn lookup_successor(
    data: &[u8],
    position: usize,
    count: u32,
    lambda: f64,
    target: u32,
) -> (f64, f64, bool) {
    let count = count as usize;
    let (mut low, mut high) = (0usize, count);
    while low < high {
        let middle = low + (high - low) / 2;
        if le_u32(data, position + middle * 8) < target {
            low = middle + 1;
        } else {
            high = middle;
        }
    }
    if low < count {
        let at = position + low * 8;
        if le_u32(data, at) == target {
            return (lambda, le_f32(data, at + 4) as f64, true);
        }
    }
    (lambda, 0.0, false)
}

#[derive(Clone, Copy)]
pub(super) struct BigramSlot {
    pub(super) lambda: f64,
    pub(super) probability: f64,
    pub(super) observed: bool,
}

impl Default for BigramSlot {
    fn default() -> Self {
        Self {
            lambda: 1.0,
            probability: 0.0,
            observed: false,
        }
    }
}

pub(super) struct BigramState {
    pub(super) cache: Columns<u64>,
    pub(super) slots: Vec<BigramSlot>,
    pub(super) hits: u64,
    pub(super) misses: u64,
}

impl BigramState {
    pub(super) fn new(limit: usize) -> Self {
        Self {
            cache: Columns::new(limit),
            slots: vec![BigramSlot::default(); limit],
            hits: 0,
            misses: 0,
        }
    }

    pub(super) fn reset(&mut self, limit: usize) {
        self.cache = Columns::new(limit);
        self.slots = vec![BigramSlot::default(); limit];
        // Lua trim_caches 不清零 bigram_hits/bigram_misses。
    }
}
