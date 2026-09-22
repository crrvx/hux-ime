// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! TCSKNM03 五阶分页语言模型读取，对应参照实现 `lua/tiger_sentence_fivegram.lua`。
//!
//! 模型格式：固定 256 B 头部 + 4×256 个 40 B 桶目录 + 自描述词表 + 按桶分块的
//! 2/3/4/5-gram 区（每 64 个 context block 一个 16 B 稀疏索引点）。
//! 读取语义是**直接 Beam 打分**（不是重排层）：调用方持有最近 4 个 token ID，
//! [`FivegramModel::step`] 返回 `ln10 × log10 概率`，内部完成 5→1 的 backoff 累加。
//!
//! 语义保真要点：
//! - 量化表达式逐字照抄参照实现（`pmin/1e7 + q*pstep/1e12`），不做 `mul_add`、
//!   不预先约分：实测与 Lua 位等价，金样按 f64 位模式比对；
//! - index cache 是整段索引的 FIFO（复用 `hux_core::cache::Fifo`）；page cache 是
//!   **带字节上限的槽位 FIFO**，三段游标语义与参照实现逐行一致（见 [`PageCache::put`]）；
//! - 不新增参照实现没有的校验：桶目录的 `blocks_bytes` 不读不校验、block 尾部垃圾静默
//!   忽略、page 区间为 0 是空页而非错误；
//! - `logp` 的口径与 TCSKNM02 不同（BOS 不入 history、没有 `1e-300` 下限），故不共用实现；
//! - 装载侧按**文件头 magic** 派发（[`crate::decode::SentenceModel`]）：文件名只决定候选顺序。

use anyhow::{Context, Result, anyhow, bail};
use hashbrown::HashMap;
use hux_core::cache::Fifo;
use memmap2::Mmap;
use std::fs::File;
use std::path::Path;
use std::rc::Rc;

/// 序列起始标记的字符串形式（参照实现里的 `"\2"`）。
pub const BOS: &str = "\u{2}";
/// 序列结束标记的字符串形式（参照实现里的 `"\3"`）。
pub const EOS: &str = "\u{3}";

const HEADER_SIZE: usize = 256;
const BUCKET_META_SIZE: usize = 40;
const BUCKETS: usize = 256;
const INDEX_ENTRY_SIZE: usize = 16;
const DEFAULT_PAGE_BYTES: usize = 8 * 1024 * 1024;
const DEFAULT_INDEX_PAGES: usize = 64;

/// 打分单位：`ln10 × log10 概率`（参照实现 `LN10 = math.log(10)`，位模式 `0x40026bb1bbb55516`）。
const LN10: f64 = std::f64::consts::LN_10;

/// 模型缓存上限；对应参照实现 `load(path, limits)` 的 limits 表（只有两个量）。
///
/// 两个上限都**原样接受**（参照实现不校验）：`index_pages` 只影响 index cache 容量与
/// page 槽位数，`page_bytes` 为 0 时退化为「最多驻留一页」。`index_pages` 为 0 时
/// index cache 容量按参照实现的 `math.max(1, limit)` 取 1。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    pub page_bytes: usize,
    pub index_pages: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            page_bytes: DEFAULT_PAGE_BYTES,
            index_pages: DEFAULT_INDEX_PAGES,
        }
    }
}

/// `cache_status` 快照；**只有 4 个字段**，与 TCSKNM02 的 13 字段口径不同。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CacheStatus {
    pub page_bytes: u64,
    pub page_limit: usize,
    pub page_entries: usize,
    pub index_cache_limit: usize,
}

impl CacheStatus {
    /// 差分金样使用的规范文本（固定字段序）。
    pub fn canonical(&self) -> String {
        format!(
            "page_bytes={}\tpage_limit={}\tpage_entries={}\tindex_cache_limit={}",
            self.page_bytes, self.page_limit, self.page_entries, self.index_cache_limit,
        )
    }
}

/// 解码状态里的语言模型槽位：`lm1` 是**最近**一个 token，`count` 是 history 中的有效 token 数。
///
/// 槽位语义与参照实现一致：`step` 回传的 `lm1..lm3` 是**旧的** `lm1..lm3`，调用方把它们
/// 依次塞进 `lm2..lm4` 槽位（`lm4` 由旧 `lm3` 顶入、且不回传）。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LmHistory {
    pub lm1: u16,
    pub lm2: u16,
    pub lm3: u16,
    pub lm4: u16,
    pub count: u8,
}

impl LmHistory {
    /// 搜索起点：`(bos_id, 0, 0, 0, 1)`，对应参照实现 `begin_search_history`。
    pub fn begin(bos_id: u16) -> Self {
        Self {
            lm1: bos_id,
            lm2: 0,
            lm3: 0,
            lm4: 0,
            count: 1,
        }
    }
}

/// `step` 的返回值；字段顺序即参照实现的六元返回 `score, id, lm1, lm2, lm3, count`。
#[derive(Clone, Copy, Debug)]
pub struct StepOutcome {
    /// `ln10 × log10 概率`。
    pub score: f64,
    /// 目标 token 解析出的 ID（词表外为 `unknown_id`）。
    pub id: u16,
    /// 旧的 `lm1`（调用方写入 `lm2` 槽位）。
    pub lm1: u16,
    /// 旧的 `lm2`（调用方写入 `lm3` 槽位）。
    pub lm2: u16,
    /// 旧的 `lm3`（调用方写入 `lm4` 槽位）。
    pub lm3: u16,
    /// `min(4, count + 1)`；与目标是否已知无关。
    pub count: u8,
}

/// 量化参数（header 里的 `quant[order]`，1..5 阶各一组）。
#[derive(Clone, Copy, Default)]
struct Quant {
    pmin_e7: i32,
    pstep_e12: u32,
    bmin_e7: i32,
    bstep_e12: u32,
}

/// 桶目录条目；只保留读取方真正消费的字段（`blocks_offset`/`blocks_bytes`/`record_count`
/// 参照实现解析后无人读取，故不取）。
#[derive(Clone, Copy, Default)]
struct BucketMeta {
    index_offset: u64,
    index_count: u32,
    block_count: u32,
}

/// 概率量化解码；表达式与参照实现逐字一致，不做 `mul_add`、不预先约分。
fn decode_probability(q: u16, quant: &Quant) -> f64 {
    quant.pmin_e7 as f64 / 1e7 + q as f64 * (quant.pstep_e12 as f64 / 1e12)
}

/// 回退量化解码；`q == 0` 表示「无该 context」（参照实现同一口径，回退为 0）。
fn decode_backoff(q: u16, quant: &Quant) -> f64 {
    if q == 0 {
        return 0.0;
    }
    quant.bmin_e7 as f64 / 1e7 + (q - 1) as f64 * (quant.bstep_e12 as f64 / 1e12)
}

/// 对照 Lua `read_at`：越界即 `truncated TCSKNM03`。
fn read_at(map: &[u8], offset: u64, count: usize) -> Result<&[u8]> {
    let start = usize::try_from(offset).map_err(|_| anyhow!("truncated TCSKNM03"))?;
    let end = start
        .checked_add(count)
        .ok_or_else(|| anyhow!("truncated TCSKNM03"))?;
    map.get(start..end)
        .ok_or_else(|| anyhow!("truncated TCSKNM03"))
}

/// 对照 Lua `u16`：越界即 `truncated TCSKNM03`（page 内只有读到才报错）。
fn read_u16(data: &[u8], offset: usize) -> Result<u16> {
    let bytes = data
        .get(offset..offset.saturating_add(2))
        .ok_or_else(|| anyhow!("truncated TCSKNM03"))?;
    Ok(u16::from_le_bytes(bytes.try_into().expect("2 字节")))
}

fn read_u32(data: &[u8], offset: usize) -> Result<u32> {
    let bytes = data
        .get(offset..offset.saturating_add(4))
        .ok_or_else(|| anyhow!("truncated TCSKNM03"))?;
    Ok(u32::from_le_bytes(bytes.try_into().expect("4 字节")))
}

fn read_u64(data: &[u8], offset: usize) -> Result<u64> {
    let bytes = data
        .get(offset..offset.saturating_add(8))
        .ok_or_else(|| anyhow!("truncated TCSKNM03"))?;
    Ok(u64::from_le_bytes(bytes.try_into().expect("8 字节")))
}

// 头部字段读取：调用方已确认映射长度 ≥ `HEADER_SIZE`，故偏移必然在界内。

fn header_u16(data: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes(data[offset..offset + 2].try_into().expect("头部 2 字节"))
}

fn header_u32(data: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(data[offset..offset + 4].try_into().expect("头部 4 字节"))
}

fn header_i32(data: &[u8], offset: usize) -> i32 {
    i32::from_le_bytes(data[offset..offset + 4].try_into().expect("头部 4 字节"))
}

fn header_u64(data: &[u8], offset: usize) -> u64 {
    u64::from_le_bytes(data[offset..offset + 8].try_into().expect("头部 8 字节"))
}

/// page cache 的键：参照实现用字符串 `"<order>:<bucket>:<entry>"`，此处等价打包为整数
/// （决定论、无分配；三段的取值范围分别是 order ∈ 2..=5、bucket < 256、entry < 2^32）。
fn page_key(order: usize, bucket: usize, entry: usize) -> u64 {
    ((order as u64) << 40) | ((bucket as u64) << 32) | entry as u64
}

/// page cache：**带字节上限的槽位 FIFO**（参照实现里的 `pages` 表）。
///
/// 与 `hux_core::cache::Fifo` 的差异（故不复用）：
/// - 容量是两个量：槽位数 `slots` 与总字节数 `limit`（字节超限时按槽位顺序淘汰）；
/// - `put` 分三段：**字节压力循环**（推进在循环体内，循环本身不推进游标）、
///   再取当前槽位作为覆盖目标、写入后推进游标；
/// - 计数口径是 `values` 的条目数（`pairs` 计数），不是槽位数。
struct PageCache {
    values: HashMap<u64, Rc<Vec<u8>>>,
    /// 槽位 `i` 保存第 `i + 1` 号槽的键；按需增长，未用到即 `None`（等价 Lua 表的 `nil`）。
    keys: Vec<Option<u64>>,
    /// 下一个写入槽位（1 基，对应参照实现 `pages.next`）。
    next: usize,
    bytes: usize,
    limit: usize,
    slots: usize,
}

impl PageCache {
    fn new(limit: usize, slots: usize) -> Self {
        Self {
            values: HashMap::new(),
            keys: Vec::new(),
            next: 1,
            bytes: 0,
            limit,
            slots,
        }
    }

    /// 对照 Lua `trim_caches`：清空缓存但保留上限与槽位数，游标与字节计数归零。
    fn trim(&mut self) {
        self.values.clear();
        self.keys.clear();
        self.next = 1;
        self.bytes = 0;
    }

    fn get(&self, key: u64) -> Option<Rc<Vec<u8>>> {
        self.values.get(&key).cloned()
    }

    fn entries(&self) -> usize {
        self.values.len()
    }

    /// 对照 Lua `page_put`（三段游标语义逐行一致）。
    fn put(&mut self, key: u64, value: Rc<Vec<u8>>) -> Rc<Vec<u8>> {
        let size = value.len();
        // ① 字节压力循环：牺牲 `keys[next]`；`next` 的推进只发生在循环体里。
        //    环空即 `break`（单页大于上限时该页超限驻留，不会死循环）。
        while !self.values.contains_key(&key) && self.bytes + size > self.limit {
            let Some(victim) = self.keys.get(self.next - 1).copied().flatten() else {
                break;
            };
            if let Some(old) = self.values.remove(&victim) {
                self.bytes -= old.len();
            }
            self.keys[self.next - 1] = None;
            self.next = self.next % self.slots + 1;
        }
        // ② 再取（可能已被 ① 腾空的）当前槽位作为覆盖目标。
        if let Some(old) = self.keys.get(self.next - 1).copied().flatten() {
            if let Some(data) = self.values.remove(&old) {
                self.bytes -= data.len();
            }
        }
        // ③ 写入并推进游标。
        if self.next > self.keys.len() {
            self.keys.push(Some(key));
        } else {
            self.keys[self.next - 1] = Some(key);
        }
        self.next = self.next % self.slots + 1;
        self.values.insert(key, value.clone());
        self.bytes += size;
        value
    }
}

/// TCSKNM03 五阶模型。
pub struct FivegramModel {
    file_size: u64,
    map: Mmap,
    unknown_id: u16,
    bos_id: u16,
    eos_id: u16,
    /// 词表：token 字节串 → ID（参照实现 `token_ids`）。
    token_ids: HashMap<Box<[u8]>, u16>,
    /// 单字符 token 的快路径（与字节串查表等价，省去热路径的字符串构造）。
    char_ids: HashMap<char, u16>,
    /// unigram 概率（已解码），下标即 token ID。
    unigram_p: Vec<f64>,
    /// 4 个 order（2..5）的 256 个桶目录。
    directories: [[BucketMeta; BUCKETS]; 4],
    /// order 1..5 的量化参数。
    quant: [Quant; 5],
    index_cache: Fifo<u32, Rc<Vec<u8>>>,
    /// index cache 容量（`cache_status().index_cache_limit`）。
    index_limit: usize,
    pages: PageCache,
}

impl FivegramModel {
    /// 按路径装载模型；`limits` 缺省为 8 MiB / 64 页。
    pub fn load(path: impl AsRef<Path>, limits: Option<Limits>) -> Result<Self> {
        let path = path.as_ref().to_path_buf();
        let display = path.display().to_string();
        let file = File::open(&path).with_context(|| format!("cannot open fivegram: {display}"))?;
        let file_size = file
            .metadata()
            .with_context(|| format!("cannot stat fivegram: {display}"))?
            .len();
        // SAFETY: 只读映射；模型生命周期内假定文件不被并发改写。
        let map = unsafe { Mmap::map(&file) }
            .with_context(|| format!("cannot map fivegram: {display}"))?;

        // 头部校验：顺序与参照实现 `parse_header` 一致（先读满 256 B，再比 magic）。
        if map.len() < HEADER_SIZE {
            bail!("truncated TCSKNM03: {display}");
        }
        if &map[..8] != b"TCSKNM03" {
            bail!("not a TCSKNM03 model: {display}");
        }
        if header_u32(&map, 8) != 1 || header_u32(&map, 12) as usize != HEADER_SIZE {
            bail!("unsupported TCSKNM03 version: {display}");
        }
        if header_u64(&map, 16) != file_size {
            bail!("TCSKNM03 size mismatch: {display}");
        }
        if header_u32(&map, 24) != 5 || header_u32(&map, 32) as usize != BUCKETS {
            bail!("invalid TCSKNM03 layout: {display}");
        }
        let vocab_count = header_u32(&map, 28);
        let vocab_offset = header_u64(&map, 40);
        let vocab_bytes = header_u64(&map, 48);
        let unknown_id = header_u16(&map, 56);
        let bos_id = header_u16(&map, 58);
        let eos_id = header_u16(&map, 60);

        let mut quant = [Quant::default(); 5];
        for (order, slot) in quant.iter_mut().enumerate() {
            let at = 160 + order * 16;
            *slot = Quant {
                pmin_e7: header_i32(&map, at),
                pstep_e12: header_u32(&map, at + 4),
                bmin_e7: header_i32(&map, at + 8),
                bstep_e12: header_u32(&map, at + 12),
            };
        }

        // 目录区不是常量偏移，按各 section 的 `directory_offset` 读。
        let mut directories = [[BucketMeta::default(); BUCKETS]; 4];
        for (index, directory) in directories.iter_mut().enumerate() {
            let offset = header_u64(&map, 64 + index * 24);
            let raw = read_at(&map, offset, BUCKETS * BUCKET_META_SIZE)?;
            for (bucket, meta) in directory.iter_mut().enumerate() {
                let at = bucket * BUCKET_META_SIZE;
                *meta = BucketMeta {
                    index_offset: read_u64(raw, at + 16)?,
                    index_count: read_u32(raw, at + 24)?,
                    block_count: read_u32(raw, at + 28)?,
                };
            }
        }

        let vocab_length =
            usize::try_from(vocab_bytes).map_err(|_| anyhow!("truncated TCSKNM03"))?;
        let vocab_raw = read_at(&map, vocab_offset, vocab_length)?;
        let mut token_ids: HashMap<Box<[u8]>, u16> = HashMap::with_capacity(vocab_count as usize);
        let mut char_ids: HashMap<char, u16> = HashMap::new();
        let mut unigram_p = Vec::with_capacity(vocab_count as usize);
        let mut position = 0usize;
        for id in 0..vocab_count {
            let length = read_u16(vocab_raw, position)? as usize;
            position += 2;
            let token = vocab_raw
                .get(position..position.saturating_add(length))
                .ok_or_else(|| anyhow!("invalid TCSKNM03 vocabulary: {display}"))?;
            position += length;
            let probability_q = read_u16(vocab_raw, position)?;
            // `b_q`（unigram backoff 量化）参照实现解析后无人读取：order-2 block 的 bow 来自
            // block 头部而非词表，故这里只跳过这 2 字节。
            let _backoff_q = read_u16(vocab_raw, position + 2)?;
            position += 4;
            let id = id as u16;
            token_ids.insert(token.into(), id);
            if let Ok(text) = std::str::from_utf8(token) {
                let mut chars = text.chars();
                if let (Some(single), None) = (chars.next(), chars.next()) {
                    char_ids.insert(single, id);
                }
            }
            unigram_p.push(decode_probability(probability_q, &quant[0]));
        }
        if position != vocab_raw.len() {
            bail!("invalid TCSKNM03 vocabulary: {display}");
        }

        let limits = limits.unwrap_or_default();
        let index_limit = limits.index_pages.max(1);
        Ok(Self {
            file_size,
            map,
            unknown_id,
            bos_id,
            eos_id,
            token_ids,
            char_ids,
            unigram_p,
            directories,
            quant,
            index_cache: Fifo::new(index_limit),
            index_limit,
            pages: PageCache::new(
                limits.page_bytes,
                limits.index_pages.saturating_mul(4).max(8),
            ),
        })
    }

    pub fn bytes(&self) -> u64 {
        self.file_size
    }

    pub fn bos_id(&self) -> u16 {
        self.bos_id
    }

    pub fn eos_id(&self) -> u16 {
        self.eos_id
    }

    pub fn unknown_id(&self) -> u16 {
        self.unknown_id
    }

    /// token → ID；词表外返回 `None`（**不是** `unknown_id`，与 `step` 的兜底口径不同）。
    pub fn token_id(&self, token: &str) -> Option<u16> {
        if token == BOS {
            return Some(self.bos_id);
        }
        if token == EOS {
            return Some(self.eos_id);
        }
        self.token_ids.get(token.as_bytes()).copied()
    }

    /// 单字符版本的 [`Self::token_id`]（解码热路径）。
    fn token_id_char(&self, token: char) -> Option<u16> {
        match token {
            '\u{2}' => Some(self.bos_id),
            '\u{3}' => Some(self.eos_id),
            _ => self.char_ids.get(&token).copied(),
        }
    }

    /// 对应参照实现 `configure_cache`：重建两个缓存并重置游标与字节计数。
    pub fn configure_cache(&mut self, limits: Limits) {
        self.index_limit = limits.index_pages.max(1);
        self.index_cache = Fifo::new(self.index_limit);
        self.pages = PageCache::new(
            limits.page_bytes,
            limits.index_pages.saturating_mul(4).max(8),
        );
    }

    /// 对应参照实现 `trim_caches`：清空两个缓存，保留上限与槽位数。
    pub fn trim_caches(&mut self) {
        self.index_cache = Fifo::new(self.index_limit);
        self.pages.trim();
    }

    /// 对应参照实现 `model.close()`：参照实现关闭文件句柄并清理缓存；
    /// mmap 随模型析构释放，故此处只有清理缓存这一步。
    pub fn close(&mut self) {
        self.trim_caches();
    }

    pub fn cache_status(&self) -> CacheStatus {
        CacheStatus {
            page_bytes: self.pages.bytes as u64,
            page_limit: self.pages.limit,
            page_entries: self.pages.entries(),
            index_cache_limit: self.index_limit,
        }
    }

    /// 整段稀疏索引（`index_count * 16` 字节）的 FIFO 缓存；键为 `order*256 + bucket`。
    fn get_index(&mut self, order: usize, bucket: usize, meta: &BucketMeta) -> Result<Rc<Vec<u8>>> {
        let key = (order as u32) * 256 + bucket as u32;
        if let Some(data) = self.index_cache.get(&key) {
            return Ok(data.clone());
        }
        let data = if meta.index_count > 0 {
            let count = usize::try_from(meta.index_count)
                .ok()
                .and_then(|count| count.checked_mul(INDEX_ENTRY_SIZE))
                .ok_or_else(|| anyhow!("truncated TCSKNM03"))?;
            Rc::new(read_at(&self.map, meta.index_offset, count)?.to_vec())
        } else {
            Rc::new(Vec::new())
        };
        Ok(self.index_cache.put(key, data).clone())
    }

    /// 由 context 定位 page 并线性扫描 block；返回 `(概率, 该 context 的 backoff, 是否观测到)`。
    ///
    /// - 概率为 `None` 且 `observed == false`：context 缺失（backoff 为 0）或
    ///   **context 命中但 target 缺失**（backoff 为该 context 自己的回退，会被上层累加）；
    /// - `observed == true` 时概率必然为 `Some`。
    fn lookup(
        &mut self,
        order: usize,
        history: &[u16],
        start: usize,
        target: u16,
    ) -> Result<(Option<f64>, f64, bool)> {
        let context_len = order - 1;
        let bucket = (history[start] % 256) as usize;
        let meta = self.directories[order - 2][bucket];
        if meta.block_count == 0 || meta.index_count == 0 {
            return Ok((None, 0.0, false));
        }
        let index = self.get_index(order, bucket, &meta)?;
        let index_count = meta.index_count as usize;
        // 二分：最后一个「索引点上下文 ≤ 查询上下文」的索引点。
        let (mut low, mut high) = (0usize, index_count);
        while low < high {
            let mid = low + (high - low) / 2;
            if compare_context(&index, mid * INDEX_ENTRY_SIZE, history, start, context_len)? <= 0 {
                low = mid + 1;
            } else {
                high = mid;
            }
        }
        if low == 0 {
            return Ok((None, 0.0, false));
        }
        let entry = low - 1;
        let at = entry * INDEX_ENTRY_SIZE;
        let offset = read_u64(&index, at + 8)?;
        // 最后一个索引点的右端点是索引区起点。
        let finish = if entry + 1 < index_count {
            read_u64(&index, at + INDEX_ENTRY_SIZE + 8)?
        } else {
            meta.index_offset
        };
        let key = page_key(order, bucket, entry);
        let data = match self.pages.get(key) {
            Some(data) => data,
            None => {
                // 区间为 0 是空页；为负（`finish < offset`）在参照实现里读文件即报错。
                let length = finish
                    .checked_sub(offset)
                    .ok_or_else(|| anyhow!("truncated TCSKNM03"))?;
                let length = usize::try_from(length).map_err(|_| anyhow!("truncated TCSKNM03"))?;
                let page = Rc::new(read_at(&self.map, offset, length)?.to_vec());
                self.pages.put(key, page)
            }
        };
        // page 内逐 block 前进：靠 `successor_count` 定位，不校验 `blocks_bytes`，
        // 也不校验是否跨过 page/index 边界（只有越界读才报错）。
        let mut position = 0usize;
        let block_header = context_len * 2 + 4;
        while position < data.len() {
            let compared = compare_context(&data, position, history, start, context_len)?;
            let bow_q = read_u16(&data, position + context_len * 2)?;
            let count = read_u16(&data, position + context_len * 2 + 2)? as usize;
            let successors = position + block_header;
            if compared == 0 {
                let bow = decode_backoff(bow_q, &self.quant[order - 2]);
                let (mut lo, mut hi) = (0usize, count);
                while lo < hi {
                    let mid = lo + (hi - lo) / 2;
                    if read_u16(&data, successors + mid * 4)? < target {
                        lo = mid + 1;
                    } else {
                        hi = mid;
                    }
                }
                if lo < count && read_u16(&data, successors + lo * 4)? == target {
                    let probability = decode_probability(
                        read_u16(&data, successors + lo * 4 + 2)?,
                        &self.quant[order - 1],
                    );
                    return Ok((Some(probability), bow, true));
                }
                return Ok((None, bow, false));
            } else if compared > 0 {
                // 已越过查询上下文（block 按 context 升序）⇒ 桶内不存在。
                return Ok((None, 0.0, false));
            }
            position = successors + count * 4;
        }
        Ok((None, 0.0, false))
    }

    /// 5→1 的 backoff 累加；命中即返回 `(total + 概率) * LN10`。
    fn score_history(&mut self, history: &[u16], id: u16) -> Result<f64> {
        let count = history.len();
        let mut total = 0.0f64;
        let maximum = count.min(4);
        for context_len in (1..=maximum).rev() {
            let start = count - context_len;
            let (probability, backoff, _) = self.lookup(context_len + 1, history, start, id)?;
            if let Some(probability) = probability {
                return Ok((total + probability) * LN10);
            }
            total += backoff;
        }
        let probability = match self.unigram_p.get(id as usize) {
            Some(probability) => *probability,
            // 头部 `unknown_id` 越界（损坏模型）时按参照实现的 nil 语义报错，而非 panic。
            None => *self
                .unigram_p
                .get(self.unknown_id as usize)
                .ok_or_else(|| anyhow!("invalid TCSKNM03 vocabulary"))?,
        };
        Ok((total + probability) * LN10)
    }

    /// 按 `(lm1, lm2, lm3, lm4, count, target)` 打一分，并回传槽位平移所需的值。
    ///
    /// `count == 0` 合法（history 仍取 `lm1`）；`count < 4` 时 `lm2..lm4` 被忽略。
    /// 返回值见 [`StepOutcome`]：调用方按 `(lm1, lm2, lm3, lm4, lm_count) =
    /// (id, lm1, lm2, lm3, count)` 平移槽位。
    pub fn step(
        &mut self,
        lm1: u16,
        lm2: u16,
        lm3: u16,
        lm4: u16,
        count: u8,
        target: &str,
    ) -> Result<StepOutcome> {
        let id = self.token_id(target).unwrap_or(self.unknown_id);
        self.step_id(lm1, lm2, lm3, lm4, count, id)
    }

    /// token 串入口：直接推进 `history` 槽位并返回分数（对应参照实现的 `step(a,b,c,d,n,target)`）。
    pub fn step_token(&mut self, history: &mut LmHistory, target: &str) -> Result<f64> {
        let id = self.token_id(target).unwrap_or(self.unknown_id);
        self.step_barrier(history, id)
    }

    /// 单字符入口：与 [`Self::step_token`] 同语义，供解码热路径避免构造字符串。
    pub fn step_char(&mut self, history: &mut LmHistory, target: char) -> Result<f64> {
        let id = self.token_id_char(target).unwrap_or(self.unknown_id);
        self.step_barrier(history, id)
    }

    fn step_id(
        &mut self,
        lm1: u16,
        lm2: u16,
        lm3: u16,
        lm4: u16,
        count: u8,
        id: u16,
    ) -> Result<StepOutcome> {
        let (history, length) = history_slots(lm1, lm2, lm3, lm4, count);
        let score = self.score_history(&history[..length], id)?;
        Ok(StepOutcome {
            score,
            id,
            lm1,
            lm2,
            lm3,
            count: count.saturating_add(1).min(4),
        })
    }

    fn step_barrier(&mut self, history: &mut LmHistory, id: u16) -> Result<f64> {
        let outcome = self.step_id(
            history.lm1,
            history.lm2,
            history.lm3,
            history.lm4,
            history.count,
            id,
        )?;
        // 槽位平移：新 `lm1` 是本次目标，`lm2..lm4` 依次接收旧的 `lm1..lm3`。
        history.lm1 = outcome.id;
        history.lm2 = outcome.lm1;
        history.lm3 = outcome.lm2;
        history.lm4 = outcome.lm3;
        history.count = outcome.count;
        Ok(outcome.score)
    }

    /// 旧口径兼容接口：`prev2` 为 BOS 时**不**把 BOS 放进 history。
    ///
    /// 与 TCSKNM02 的 `logp` 算法不同（无 `1e-300` 下限、无插值），故独立实现；
    /// 五阶主路径不要用它，它是「不支持 `step` 的旧模型」的回落口径。
    pub fn logp(&mut self, prev2: &str, prev1: &str, target: &str) -> Result<f64> {
        let mut history = LmHistory::begin(self.bos_id);
        if prev2 != BOS {
            self.step_token(&mut history, prev2)?;
        }
        if prev1 != BOS || prev2 != BOS {
            self.step_token(&mut history, prev1)?;
        }
        self.step_token(&mut history, target)
    }

    /// 该二元组是否在模型中有观测记录（只查 order 2，不做 backoff、不看概率）。
    ///
    /// 任一侧不在词表（且不是 BOS/EOS）即为 `false`——与 `step` 的 `unknown` 兜底不同。
    pub fn has_observed_bigram(&mut self, previous: &str, target: &str) -> Result<bool> {
        let (Some(left), Some(right)) = (self.token_id(previous), self.token_id(target)) else {
            return Ok(false);
        };
        let (_, _, observed) = self.lookup(2, &[left], 0, right)?;
        Ok(observed)
    }

    /// 单字符版本的 [`Self::has_observed_bigram`]（解码热路径）。
    pub fn has_observed_bigram_chars(&mut self, previous: char, target: char) -> Result<bool> {
        let (Some(left), Some(right)) = (self.token_id_char(previous), self.token_id_char(target))
        else {
            return Ok(false);
        };
        let (_, _, observed) = self.lookup(2, &[left], 0, right)?;
        Ok(observed)
    }
}

/// 按 `count` 取 history 槽位（最旧在前），返回 `(槽位, 有效长度)`。
///
/// 对照参照实现 `step` 的 history 拼接：`count <= 1` 取 `lm1`，`count == 2` 取
/// `[lm2, lm1]`，`count == 3` 取 `[lm3, lm2, lm1]`，其余取 4 个槽位（`count` 超过 4 亦然）。
fn history_slots(lm1: u16, lm2: u16, lm3: u16, lm4: u16, count: u8) -> ([u16; 4], usize) {
    match count {
        0 | 1 => ([lm1, 0, 0, 0], 1),
        2 => ([lm2, lm1, 0, 0], 2),
        3 => ([lm3, lm2, lm1, 0], 3),
        _ => ([lm4, lm3, lm2, lm1], 4),
    }
}

/// 无符号 u16 字典序比较（context 顺序「旧 → 新」）；`at` 是索引点或 block 的 context 起点。
fn compare_context(
    data: &[u8],
    at: usize,
    history: &[u16],
    start: usize,
    context_len: usize,
) -> Result<i32> {
    for index in 0..context_len {
        let left = read_u16(data, at + index * 2)?;
        let right = history[start + index];
        if left < right {
            return Ok(-1);
        }
        if left > right {
            return Ok(1);
        }
    }
    Ok(0)
}

#[cfg(test)]
mod tests;
