//! TCSKNM02 分页 KN 语言模型读取，对应参照实现 `lua/tiger_sentence_ngram.lua`。
//!
//! K0 范围：TCSKNM02（mobile）；TCSKNM01（legacy）随 K1 补齐。
//! 语义保真要点：两级稀疏索引、按字节分页的 LRU 页缓存、列式上下文缓存、
//! FIFO 索引缓存、`cache_status` 计数（`#keys` 语义）。

use crate::cache::{Columns, Fifo};
use anyhow::{Context, Result, anyhow, bail};
use hashbrown::HashMap;
use memmap2::Mmap;
use std::collections::VecDeque;
use std::fs::File;
use std::path::{Path, PathBuf};
use std::rc::Rc;

pub const BOS: &str = "\u{2}";
pub const EOS: &str = "\u{3}";
/// 42-bit 三元组/二元组打包位移（2^21）。
pub const SHIFT: u64 = 2_097_152;

const MOBILE_HEADER_SIZE: usize = 104;
pub const MOBILE_CACHE_BYTES: usize = 8 * 1024 * 1024;
pub const CONTEXT_CACHE_ENTRIES: usize = 16384;
const INDEX_PAGE_RECORDS: usize = 256; // 每页 4 KiB 稀疏索引
pub const INDEX_CACHE_PAGES: usize = 64;
const DEFAULT_BIGRAM_ENTRIES: usize = 8192;

/// 模型缓存上限；对应 Lua `load(path, limits)` 的 limits 表。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    pub page_bytes: usize,
    pub context_entries: usize,
    pub bigram_entries: usize,
    pub index_pages: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            page_bytes: MOBILE_CACHE_BYTES,
            context_entries: CONTEXT_CACHE_ENTRIES,
            bigram_entries: DEFAULT_BIGRAM_ENTRIES,
            index_pages: INDEX_CACHE_PAGES,
        }
    }
}

/// 参照 `scalar`：空串→0，BOS/EOS→2/3，其余取首个码位。
pub fn scalar(token: &str) -> u32 {
    if token.is_empty() {
        return 0;
    }
    if token == BOS {
        return 2;
    }
    if token == EOS {
        return 3;
    }
    token.chars().next().map(|c| c as u32).unwrap_or(0)
}

/// 参照 `pack2`：`first * SHIFT + second % SHIFT`。
pub fn pack2(first: u32, second: u32) -> u64 {
    first as u64 * SHIFT + second as u64 % SHIFT
}

fn le_u32(data: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(data[offset..offset + 4].try_into().expect("bounds checked"))
}

fn le_u64(data: &[u8], offset: usize) -> u64 {
    u64::from_le_bytes(data[offset..offset + 8].try_into().expect("bounds checked"))
}

fn le_f32(data: &[u8], offset: usize) -> f32 {
    f32::from_le_bytes(data[offset..offset + 4].try_into().expect("bounds checked"))
}

/// 对照 Lua `read_at`：越界即“truncated mobile n-gram”。
fn read_at(map: &[u8], offset: u64, count: usize) -> Result<&[u8]> {
    let start = usize::try_from(offset).map_err(|_| anyhow!("truncated mobile n-gram"))?;
    let end = start
        .checked_add(count)
        .ok_or_else(|| anyhow!("truncated mobile n-gram"))?;
    map.get(start..end)
        .ok_or_else(|| anyhow!("truncated mobile n-gram"))
}

#[derive(Default)]
struct Counters {
    page_misses: u64,
    page_bytes: u64,
    index_misses: u64,
    index_bytes_read: u64,
}

/// 稀疏索引：常驻（≤1 页）或“目录 + 分页 FIFO 缓存”。
struct Index {
    offset: u64,
    count: usize,
    pages: usize,
    cache: Fifo<u32, Rc<Vec<u8>>>,
    resident: Option<Rc<Vec<u8>>>,
    directory: Option<Rc<Vec<u8>>>,
}

impl Index {
    fn resident_len(&self) -> u64 {
        match (&self.resident, &self.directory) {
            (Some(data), _) | (_, Some(data)) => data.len() as u64,
            _ => 0,
        }
    }

    fn cached_bytes(&self) -> u64 {
        self.cache.values().map(|data| data.len() as u64).sum()
    }
}

fn open_index(map: &[u8], offset: u64, count: usize, index_limit: usize) -> Result<Index> {
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

struct PageCache {
    map: HashMap<(u8, u32), Rc<Vec<u8>>>,
    order: VecDeque<(u8, u32)>,
    bytes: usize,
    limit: usize,
}

impl PageCache {
    fn new(limit: usize) -> Self {
        Self {
            map: HashMap::new(),
            order: VecDeque::new(),
            bytes: 0,
            limit,
        }
    }

    fn get(&mut self, kind: u8, page: u32) -> Option<Rc<Vec<u8>>> {
        let data = self.map.get(&(kind, page))?.clone();
        if let Some(position) = self.order.iter().position(|&entry| entry == (kind, page)) {
            self.order.remove(position);
        }
        self.order.push_back((kind, page));
        Some(data)
    }

    fn insert(&mut self, kind: u8, page: u32, data: Rc<Vec<u8>>) {
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
struct CtxSlot {
    page: Option<u32>,
    position: usize,
    count: u32,
    lambda: f64,
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

/// 一个 n-gram 阶的索引 + 上下文列缓存（kind = `b'b'` / `b't'`）。
struct KindState {
    kind: u8,
    index: Index,
    context: Columns<u64>,
    slots: Vec<CtxSlot>,
    index_stride: usize,
    context_count: usize,
    section_end: u64,
}

impl KindState {
    fn reset_caches(&mut self, context_limit: usize, index_limit: usize) {
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
        let records = INDEX_PAGE_RECORDS.min(self.index.count - first);
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
        let data = Rc::new(read_at(map, offset, (next_offset - offset) as usize)?.to_vec());
        counters.page_misses += 1;
        counters.page_bytes += data.len() as u64;
        pages.insert(self.kind, page as u32, data.clone());
        Ok(data)
    }

    fn remember_absent(&mut self, key: u64) {
        let slot = self.context.claim(key);
        self.slots[slot - 1] = CtxSlot::default();
    }

    fn lookup_context(
        &mut self,
        map: &[u8],
        pages: &mut PageCache,
        counters: &mut Counters,
        key: u64,
        target: u32,
    ) -> Result<(f64, f64, bool)> {
        if let Some(slot) = self.context.slot(&key) {
            let entry = self.slots[slot - 1];
            let Some(page) = entry.page else {
                return Ok((1.0, 0.0, false));
            };
            let data = self.get_page(map, pages, counters, page as usize)?;
            return Ok(lookup_successor(
                &data,
                entry.position,
                entry.count,
                entry.lambda,
                target,
            ));
        }

        let page = self.find_page(map, counters, key)?;
        if page < 0 {
            self.remember_absent(key);
            return Ok((1.0, 0.0, false));
        }
        let data = self.get_page(map, pages, counters, page as usize)?;
        let mut position = 0usize;
        let remaining = self.index_stride.min(
            self.context_count
                .saturating_sub(page as usize * self.index_stride),
        );
        for _ in 0..remaining {
            let context_key = le_u64(&data, position);
            let lambda = le_f32(&data, position + 8) as f64;
            let successor_count = le_u32(&data, position + 12);
            position += 16;
            if context_key == key {
                let slot = self.context.claim(key);
                self.slots[slot - 1] = CtxSlot {
                    page: Some(page as u32),
                    position,
                    count: successor_count,
                    lambda,
                };
                return Ok(lookup_successor(
                    &data,
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
            position += successor_count as usize * 8;
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
struct BigramSlot {
    lambda: f64,
    probability: f64,
    observed: bool,
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

struct BigramState {
    cache: Columns<u64>,
    slots: Vec<BigramSlot>,
    hits: u64,
    misses: u64,
}

impl BigramState {
    fn new(limit: usize) -> Self {
        Self {
            cache: Columns::new(limit),
            slots: vec![BigramSlot::default(); limit],
            hits: 0,
            misses: 0,
        }
    }

    fn reset(&mut self, limit: usize) {
        self.cache = Columns::new(limit);
        self.slots = vec![BigramSlot::default(); limit];
        // Lua trim_caches 不清零 bigram_hits/bigram_misses。
    }
}

/// `cache_status` 快照；字段与 Lua 表一一对应。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CacheStatus {
    pub page_bytes: u64,
    pub page_limit: usize,
    pub resident_index_bytes: u64,
    pub index_cache_bytes: u64,
    pub index_cache_limit: u64,
    pub index_misses: u64,
    pub index_bytes_read: u64,
    pub bigram_entries: usize,
    pub bigram_limit: usize,
    pub context_entries: usize,
    pub context_limit: usize,
    pub bigram_hits: u64,
    pub bigram_misses: u64,
}

impl CacheStatus {
    /// 差分金样使用的规范文本（固定字段序）。
    pub fn canonical(&self) -> String {
        format!(
            "page_bytes={}\tpage_limit={}\tresident_index_bytes={}\tindex_cache_bytes={}\t\
             index_cache_limit={}\tindex_misses={}\tindex_bytes_read={}\tbigram_entries={}\t\
             bigram_limit={}\tcontext_entries={}\tcontext_limit={}\tbigram_hits={}\tbigram_misses={}",
            self.page_bytes,
            self.page_limit,
            self.resident_index_bytes,
            self.index_cache_bytes,
            self.index_cache_limit,
            self.index_misses,
            self.index_bytes_read,
            self.bigram_entries,
            self.bigram_limit,
            self.context_entries,
            self.context_limit,
            self.bigram_hits,
            self.bigram_misses,
        )
    }
}

pub struct MobileModel {
    path: PathBuf,
    file_size: u64,
    map: Mmap,
    unigram_values: HashMap<i64, f64>,
    unknown: f64,
    bi: KindState,
    tri: KindState,
    pages: PageCache,
    counters: Counters,
    bigram: BigramState,
    limits: Limits,
    resident_index_bytes: u64,
    source_index_bytes: u64,
}

impl MobileModel {
    pub fn load(path: impl AsRef<Path>, limits: Option<Limits>) -> Result<Self> {
        let path = path.as_ref().to_path_buf();
        let display = path.display().to_string();
        let file = File::open(&path).with_context(|| format!("cannot open n-gram: {display}"))?;
        let file_size = file
            .metadata()
            .with_context(|| format!("cannot stat n-gram: {display}"))?
            .len();
        // SAFETY: 只读映射；模型生命周期内假定文件不被并发改写。
        let map =
            unsafe { Mmap::map(&file) }.with_context(|| format!("cannot map n-gram: {display}"))?;

        match map.get(..8) {
            Some(b"TCSKNM02") => {}
            Some(b"TCSKNM01") => {
                bail!("legacy TCSKNM01 model is not supported yet: {display}")
            }
            _ => bail!("not a mobile TCSKNM02 model: {display}"),
        }
        if map.len() < MOBILE_HEADER_SIZE {
            bail!("truncated mobile n-gram: {display}");
        }
        let version = le_u32(&map, 8);
        let header_size = le_u32(&map, 12);
        let declared_size = le_u64(&map, 16);
        let index_stride = le_u32(&map, 24) as usize;
        let uni_count = le_u32(&map, 32) as usize;
        let uni_off = le_u64(&map, 40);
        let bi_ctx_count = le_u32(&map, 48) as usize;
        let bi_index_count = le_u32(&map, 52) as usize;
        let bi_blocks_off = le_u64(&map, 56);
        let bi_index_off = le_u64(&map, 64);
        let tri_ctx_count = le_u64(&map, 72) as usize;
        let tri_index_count = le_u32(&map, 80) as usize;
        let tri_blocks_off = le_u64(&map, 88);
        let tri_index_off = le_u64(&map, 96);
        if version != 1 || header_size as usize != MOBILE_HEADER_SIZE {
            bail!("unsupported mobile n-gram version: {display}");
        }
        if index_stride < 16 || bi_blocks_off >= bi_index_off {
            bail!("invalid mobile bigram layout: {display}");
        }
        if bi_index_off >= tri_blocks_off || tri_blocks_off >= tri_index_off {
            bail!("invalid mobile trigram layout: {display}");
        }
        if file_size != declared_size {
            bail!("mobile n-gram size mismatch: {display}");
        }

        let limits = limits.unwrap_or_default();
        let unigrams = read_at(&map, uni_off, uni_count * 8)?;
        if unigrams.len() < 8 {
            bail!("truncated mobile unigram section: {display}");
        }
        let unknown = le_f32(unigrams, 4) as f64;
        let mut unigram_values = HashMap::with_capacity(uni_count);
        for position in (0..unigrams.len()).step_by(8) {
            let key = i32::from_le_bytes(
                unigrams[position..position + 4]
                    .try_into()
                    .expect("bounds checked"),
            ) as i64;
            let probability = le_f32(unigrams, position + 4) as f64;
            unigram_values.insert(key, probability);
        }

        let bi_index = open_index(&map, bi_index_off, bi_index_count, limits.index_pages)?;
        let tri_index = open_index(&map, tri_index_off, tri_index_count, limits.index_pages)?;
        let resident_index_bytes = bi_index.resident_len() + tri_index.resident_len();
        let source_index_bytes =
            (uni_count * 8 + bi_index_count * 16 + tri_index_count * 16) as u64;

        Ok(Self {
            path,
            file_size,
            map,
            unigram_values,
            unknown,
            bi: KindState {
                kind: b'b',
                index: bi_index,
                context: Columns::new(limits.context_entries),
                slots: vec![CtxSlot::default(); limits.context_entries],
                index_stride,
                context_count: bi_ctx_count,
                section_end: bi_index_off,
            },
            tri: KindState {
                kind: b't',
                index: tri_index,
                context: Columns::new(limits.context_entries),
                slots: vec![CtxSlot::default(); limits.context_entries],
                index_stride,
                context_count: tri_ctx_count,
                section_end: tri_index_off,
            },
            pages: PageCache::new(limits.page_bytes),
            counters: Counters::default(),
            bigram: BigramState::new(limits.bigram_entries),
            limits,
            resident_index_bytes,
            source_index_bytes,
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn bytes(&self) -> u64 {
        self.file_size
    }

    pub fn format(&self) -> &'static str {
        "TCSKNM02"
    }

    pub fn resident_index_bytes(&self) -> u64 {
        self.resident_index_bytes
    }

    pub fn source_index_bytes(&self) -> u64 {
        self.source_index_bytes
    }

    pub fn cache_limit_bytes(&self) -> usize {
        self.limits.page_bytes
    }

    pub fn page_misses(&self) -> u64 {
        self.counters.page_misses
    }

    pub fn page_bytes_read(&self) -> u64 {
        self.counters.page_bytes
    }

    pub fn configure_cache(&mut self, limits: Limits) -> Result<()> {
        if limits.page_bytes < 1
            || limits.context_entries < 1
            || limits.bigram_entries < 1
            || limits.index_pages < 1
        {
            bail!("invalid model cache limits");
        }
        if self.limits == limits {
            return Ok(());
        }
        self.limits = limits;
        self.trim_caches();
        Ok(())
    }

    pub fn trim_caches(&mut self) {
        self.pages = PageCache::new(self.limits.page_bytes);
        self.bi
            .reset_caches(self.limits.context_entries, self.limits.index_pages);
        self.tri
            .reset_caches(self.limits.context_entries, self.limits.index_pages);
        self.bigram.reset(self.limits.bigram_entries);
    }

    /// 对应 Lua `model.close()`：清理缓存；mmap 随模型析构释放。
    pub fn close(&mut self) {
        self.trim_caches();
    }

    fn lookup_bigram(&mut self, context: u32, target: u32) -> Result<(f64, f64, bool)> {
        let key = pack2(context, target);
        if let Some(slot) = self.bigram.cache.slot(&key) {
            let entry = self.bigram.slots[slot - 1];
            self.bigram.hits += 1;
            return Ok((entry.lambda, entry.probability, entry.observed));
        }
        self.bigram.misses += 1;
        let (lambda, probability, observed) = {
            let Self {
                map,
                bi,
                pages,
                counters,
                ..
            } = self;
            bi.lookup_context(map, pages, counters, context as u64, target)?
        };
        let slot = self.bigram.cache.claim(key);
        self.bigram.slots[slot - 1] = BigramSlot {
            lambda,
            probability,
            observed,
        };
        Ok((lambda, probability, observed))
    }

    pub fn logp(&mut self, prev2: &str, prev1: &str, target: &str) -> Result<f64> {
        let first = scalar(prev2);
        let second = scalar(prev1);
        let third = scalar(target);
        let unigram = self
            .unigram_values
            .get(&(third as i64))
            .copied()
            .unwrap_or(self.unknown);
        let (bigram_lambda, bigram_probability, _) = self.lookup_bigram(second, third)?;
        let bigram = bigram_probability + bigram_lambda * unigram;
        let (trigram_lambda, trigram_probability, _) = {
            let Self {
                map,
                tri,
                pages,
                counters,
                ..
            } = self;
            tri.lookup_context(map, pages, counters, pack2(first, second), third)?
        };
        let probability = trigram_probability + trigram_lambda * bigram;
        Ok(probability.max(1e-300).ln())
    }

    pub fn has_observed_bigram(&mut self, prev: &str, target: &str) -> Result<bool> {
        let (_, _, observed) = self.lookup_bigram(scalar(prev), scalar(target))?;
        Ok(observed)
    }

    pub fn cache_status(&self) -> CacheStatus {
        CacheStatus {
            page_bytes: self.pages.bytes as u64,
            page_limit: self.limits.page_bytes,
            resident_index_bytes: self.resident_index_bytes,
            index_cache_bytes: self.bi.index.cached_bytes() + self.tri.index.cached_bytes(),
            index_cache_limit: (2 * self.limits.index_pages * INDEX_PAGE_RECORDS * 16) as u64,
            index_misses: self.counters.index_misses,
            index_bytes_read: self.counters.index_bytes_read,
            bigram_entries: self.bigram.cache.len(),
            bigram_limit: self.limits.bigram_entries,
            context_entries: self.bi.context.len() + self.tri.context.len(),
            context_limit: 2 * self.limits.context_entries,
            bigram_hits: self.bigram.hits,
            bigram_misses: self.bigram.misses,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scalar_and_pack2_match_reference_rules() {
        assert_eq!(scalar(""), 0);
        assert_eq!(scalar(BOS), 2);
        assert_eq!(scalar(EOS), 3);
        assert_eq!(scalar("甲"), 0x7532);
        assert_eq!(scalar("不存在"), 0x4e0d); // 首个码位
        assert_eq!(pack2(0x7532, 0x7532), 0x7532 * SHIFT + 0x7532);
        assert_eq!(pack2(1, 5), SHIFT + 5);
        assert_eq!(pack2(0x10FFFF, 0x10FFFF), 0x10FFFF * SHIFT + 0x10FFFF);
    }

    #[test]
    fn rejects_legacy_and_unknown_models() {
        let directory = std::env::temp_dir();
        let write = |name: &str, bytes: &[u8]| {
            let path = directory.join(format!("tigerclaw-{name}-{}.bin", std::process::id()));
            std::fs::write(&path, bytes).expect("write temp model");
            path
        };
        let legacy = write("legacy", b"TCSKNM01-not-really-a-model");
        let error = match MobileModel::load(&legacy, None) {
            Ok(_) => panic!("legacy model accepted"),
            Err(error) => error.to_string(),
        };
        assert!(error.contains("TCSKNM01"), "unexpected error: {error}");
        std::fs::remove_file(&legacy).ok();

        let unknown = write("unknown", b"NOTAMODELBLOB");
        assert!(MobileModel::load(&unknown, None).is_err());
        std::fs::remove_file(&unknown).ok();
    }
}
