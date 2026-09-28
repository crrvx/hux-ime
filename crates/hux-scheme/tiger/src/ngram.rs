// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! TCSKNM02 分页 KN 语言模型读取，对应参照实现 `lua/tiger_sentence_ngram.lua`。
//!
//! 模型格式：TCSKNM02（mobile）。加载器仅接受该格式。
//! 语义保真要点：两级稀疏索引、按字节分页的 LRU 页缓存、列式上下文缓存、
//! FIFO 索引缓存、`cache_status` 计数（`#keys` 语义）。

use anyhow::{Context, Result, bail};
use hashbrown::HashMap;
use hux_core::cache::Columns;
use memmap2::Mmap;
use std::fs::File;
use std::path::Path;

mod binary;
mod kind;
mod state;
mod status;
#[cfg(test)]
mod tests;

use binary::{le_f32, le_u32, le_u64, pack2, read_at, scalar, validate_limits};
use kind::{BigramSlot, BigramState, KindState};
use state::{Counters, CtxSlot, PageCache, open_index};

pub use status::CacheStatus;

/// 句首哨兵字符：与 `BOS` 同源，供需要字符/码点的调用方使用。
pub const BOS_CHAR: char = '\u{2}';
/// 句尾哨兵字符。
pub const EOS_CHAR: char = '\u{3}';
/// 句首哨兵码点值（`char` 到 `u32` 的常量派生）。
pub const BOS_CODE: u32 = BOS_CHAR as u32;
/// 句尾哨兵码点值。
pub const EOS_CODE: u32 = EOS_CHAR as u32;

const MOBILE_HEADER_SIZE: usize = 104;
const MOBILE_CACHE_BYTES: usize = 8 * 1024 * 1024;
const CONTEXT_CACHE_ENTRIES: usize = 16384;
const INDEX_PAGE_RECORDS: usize = 256; // 每页 4 KiB 稀疏索引
const INDEX_CACHE_PAGES: usize = 64;
const DEFAULT_BIGRAM_ENTRIES: usize = 8192;

/// 模型文件头 magic 的长度（三阶 `TCSKNM02` / 五阶 `TCSKNM03`）。
pub(crate) const MAGIC_LEN: usize = 8;
/// 三阶文件头 magic：本读取器**唯一**接受的格式。
const MOBILE_MAGIC_3: [u8; MAGIC_LEN] = *b"TCSKNM02";
/// 五阶文件头 magic：读取器尚不支持装载，仅供格式标签辨识。
const MOBILE_MAGIC_5: [u8; MAGIC_LEN] = *b"TCSKNM03";

/// 模型文件头格式（按 magic 判定，与读取器是否支持装载无关）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ModelFormat {
    /// 三阶 `TCSKNM02`。
    Mobile3,
    /// 五阶 `TCSKNM03`。
    Mobile5,
    /// 认不出的文件头（含过短 / 空文件头）。
    Unknown,
}

/// 按文件头 magic 判格式：**判定单点**，装载校验与状态展示标签共用它。
pub(crate) fn detect_format(magic: &[u8]) -> ModelFormat {
    if magic.starts_with(&MOBILE_MAGIC_3) {
        ModelFormat::Mobile3
    } else if magic.starts_with(&MOBILE_MAGIC_5) {
        ModelFormat::Mobile5
    } else {
        ModelFormat::Unknown
    }
}

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

pub struct MobileModel {
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
}

impl MobileModel {
    /// 装载 `TCSKNM02` 模型并建立缓存。
    pub fn load(path: impl AsRef<Path>, limits: Option<Limits>) -> Result<Self> {
        let path = path.as_ref().to_path_buf();
        let display = path.display().to_string();
        let (map, file_size) = open_map(&path, &display)?;
        match detect_format(&map) {
            ModelFormat::Mobile3 => {}
            _ => bail!("not a mobile TCSKNM02 model: {display}"),
        }
        let header = parse_header(&map, file_size, &display)?;

        let limits = limits.unwrap_or_default();
        validate_limits(limits).with_context(|| format!("invalid cache limits: {display}"))?;
        let (unigram_values, unknown) =
            read_unigrams(&map, header.uni_count, header.uni_off, &display)?;

        let bi_index = open_index(
            &map,
            header.bi_index_off,
            header.bi_index_count,
            limits.index_pages,
        )?;
        let tri_index = open_index(
            &map,
            header.tri_index_off,
            header.tri_index_count,
            limits.index_pages,
        )?;
        let resident_index_bytes = bi_index.resident_len() + tri_index.resident_len();

        Ok(Self {
            file_size,
            map,
            unigram_values,
            unknown,
            bi: KindState {
                kind: b'b',
                index: bi_index,
                context: Columns::new(limits.context_entries),
                slots: vec![CtxSlot::default(); limits.context_entries],
                index_stride: header.index_stride,
                context_count: header.bi_ctx_count,
                section_end: header.bi_index_off,
            },
            tri: KindState {
                kind: b't',
                index: tri_index,
                context: Columns::new(limits.context_entries),
                slots: vec![CtxSlot::default(); limits.context_entries],
                index_stride: header.index_stride,
                context_count: header.tri_ctx_count,
                section_end: header.tri_index_off,
            },
            pages: PageCache::new(limits.page_bytes),
            counters: Counters::default(),
            bigram: BigramState::new(limits.bigram_entries),
            limits,
            resident_index_bytes,
        })
    }

    pub fn bytes(&self) -> u64 {
        self.file_size
    }

    pub fn configure_cache(&mut self, limits: Limits) -> Result<()> {
        validate_limits(limits)?;
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
        self.logp_codes(scalar(prev2), scalar(prev1), scalar(target))
    }

    /// 与 `logp` 相同，但直接接收码点（解码热路径）。
    pub fn logp_codes(&mut self, first: u32, second: u32, third: u32) -> Result<f64> {
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
        self.has_observed_bigram_codes(scalar(prev), scalar(target))
    }

    /// 与 `has_observed_bigram` 相同，但直接接收码点。
    pub fn has_observed_bigram_codes(&mut self, prev: u32, target: u32) -> Result<bool> {
        let (_, _, observed) = self.lookup_bigram(prev, target)?;
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

/// `load` 需要的 `TCSKNM02` 头部字段（已校验）。
struct MobileHeader {
    index_stride: usize,
    uni_count: usize,
    uni_off: u64,
    bi_ctx_count: usize,
    bi_index_count: usize,
    bi_index_off: u64,
    tri_ctx_count: usize,
    tri_index_count: usize,
    tri_index_off: u64,
}

/// 打开并只读映射模型文件，返回映射与文件长度。
fn open_map(path: &Path, display: &str) -> Result<(Mmap, u64)> {
    let file = File::open(path).with_context(|| format!("cannot open n-gram: {display}"))?;
    let file_size = file
        .metadata()
        .with_context(|| format!("cannot stat n-gram: {display}"))?
        .len();
    // SAFETY: 只读映射；模型生命周期内假定文件不被并发改写。
    let map =
        unsafe { Mmap::map(&file) }.with_context(|| format!("cannot map n-gram: {display}"))?;
    Ok((map, file_size))
}

/// 校验并读出移动端 n-gram 头部；畸形头部在此拒绝。
fn parse_header(map: &[u8], file_size: u64, display: &str) -> Result<MobileHeader> {
    if map.len() < MOBILE_HEADER_SIZE {
        bail!("truncated mobile n-gram: {display}");
    }
    let version = le_u32(map, 8);
    let header_size = le_u32(map, 12);
    let declared_size = le_u64(map, 16);
    let index_stride = le_u32(map, 24) as usize;
    let uni_count = le_u32(map, 32) as usize;
    let uni_off = le_u64(map, 40);
    let bi_ctx_count = le_u32(map, 48) as usize;
    let bi_index_count = le_u32(map, 52) as usize;
    let bi_blocks_off = le_u64(map, 56);
    let bi_index_off = le_u64(map, 64);
    // 32 位目标（android 为优先平台）不得静默截断：先按 `u64` 语义校验，再转 `usize`。
    let tri_ctx_count_u64 = le_u64(map, 72);
    let tri_ctx_count = usize::try_from(tri_ctx_count_u64)
        .context("trigram context count exceeds the address space")?;
    let tri_index_count = le_u32(map, 80) as usize;
    let tri_blocks_off = le_u64(map, 88);
    let tri_index_off = le_u64(map, 96);
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
    // 上下文数必须能被文件本身容纳（每条至少 8 字节）：畸形头部在 32 位目标上
    // 会先截断成看似合理的值，此处按 `u64` 直接拒绝。
    if tri_ctx_count_u64 > file_size / 8 {
        bail!("implausible trigram context count: {tri_ctx_count_u64} for {file_size} bytes");
    }
    Ok(MobileHeader {
        index_stride,
        uni_count,
        uni_off,
        bi_ctx_count,
        bi_index_count,
        bi_index_off,
        tri_ctx_count,
        tri_index_count,
        tri_index_off,
    })
}

/// 读入 unigram 段：码点 → 概率，并给出未知字的兜底概率。
fn read_unigrams(
    map: &[u8],
    count: usize,
    offset: u64,
    display: &str,
) -> Result<(HashMap<i64, f64>, f64)> {
    let unigram_bytes = count
        .checked_mul(8)
        .context("unigram section size overflow")?;
    let unigrams = read_at(map, offset, unigram_bytes)?;
    if unigrams.len() < 8 {
        bail!("truncated mobile unigram section: {display}");
    }
    let unknown = le_f32(unigrams, 4) as f64;
    let mut unigram_values = HashMap::with_capacity(count);
    for position in (0..unigrams.len()).step_by(8) {
        let key = i32::from_le_bytes(
            unigrams[position..position + 4]
                .try_into()
                .expect("bounds checked"),
        ) as i64;
        let probability = le_f32(unigrams, position + 4) as f64;
        // NaN 语义：Lua `math.max(nan, x)` 返回 `nan`，Rust 的
        // `f64::max` 返回非 NaN 操作数 ⇒ 二者相反。此处**不做** `max`，正常数据
        // （f32 概率）不可达 NaN，故只注明口径差异、不引入无金样支撑的分支。
        unigram_values.insert(key, probability);
    }
    Ok((unigram_values, unknown))
}
