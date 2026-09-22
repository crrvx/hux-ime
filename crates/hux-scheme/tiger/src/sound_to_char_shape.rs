// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! 音反查：`tiger_sentence.pinyin.bin[.gz]`（TCSRV01）读取与音反查翻译。
//!
//! 语义对齐 librime 1.17.0 的词典音反查（`reverse_lookup_translator` + `ReverseLookupFilter`，
//! 见 `docs/rust-migration.md`）：
//! - 输入（去掉前缀后）按**拼写表**分段：音节本体 + 缩写（PY_c.schema.yaml 的两条
//!   `abbrev` 规则），缩写可信度罚 `log(0.5)`；
//! - 输入尾部无法由拼写键消耗时，对剩余部分做**补全**（拼写表子树展开；本体拼写再罚
//!   `log(0.05)`，缩写保持自身罚）；
//! - 分段路径的音节序列必须与词条的码**完全一致**；
//! - 候选次序 = 「可信度 + ln(权重)」降序（权重序取自组内稳定排序），上限
//!   [`CANDIDATE_LIMIT`]（与主候选一致）；
//! - 注释（虎码）由 [`code_comment_filter`] 追加以复用现有码注释格式。
//!
//! `code_comment`/`code_comment_filter` 定义于同 crate 的 `interaction`（与主候选共用码注释格式）。

use crate::interaction::code_comment_filter;
use crate::lexicon::Lexicon;
use anyhow::{Context, Result, bail};
use hux_core::punct::{PairState, PunctTable};
use hux_core::session::Candidate;
use std::path::{Path, PathBuf};

/// 索引文件名（发布为 `.gz`；fixture 常用未压缩）。
pub const SOUND_TO_CHAR_SHAPE_FILE: &str = "tiger_sentence.pinyin.bin";
pub const SOUND_TO_CHAR_SHAPE_FILE_GZ: &str = "tiger_sentence.pinyin.bin.gz";
/// 音反查候选上限（与主候选一致；单一来源 [`crate::decode::CANDIDATE_LIMIT`]）。
pub use crate::decode::CANDIDATE_LIMIT;
/// 音反查段标签（参照 schema 的 `reverse_lookup`）。
pub const SOUND_TO_CHAR_SHAPE_TAG: &str = "reverse_lookup";
/// 音反查段提示（参照 schema `reverse_lookup/tips`）。
pub const SOUND_TO_CHAR_SHAPE_TIPS: &str = "〔拼音〕";

const MAGIC: &[u8; 8] = b"TCSRV01\n";
/// 拼写类型（同 librime `SpellingType` 序：normal < fuzzy < abbreviation < completion）。
const KIND_NORMAL: u8 = 0;
const KIND_FUZZY: u8 = 1;
const KIND_ABBREV: u8 = 2;
const KIND_COMPLETION: u8 = 3;
/// 索引中的类型标记（0 = 本体，1 = 缩写）。
const TYPE_NORMAL: u8 = 0;
const TYPE_ABBREV: u8 = 1;
/// 参照 `kAbbreviationPenalty = log(0.5)`。
const ABBREV_PENALTY: f64 = -std::f64::consts::LN_2;
/// 参照 `kCompletionPenalty = log(0.05)`。
const COMPLETION_PENALTY: f64 = -2.995732273553991;
/// 参照 `log(DBL_EPSILON)`（权重为 0 时）。
const ZERO_WEIGHT_LOG: f64 = -36.04365338911715;
/// 各类记录的**最小**字节数（容量钳制用）：长度前缀 / 计数 / 权重等固定字段。
/// 文件头声明的计数不可信（可要求数十 GB 预分配），实际记录数受剩余字节数限制。
const MIN_SYLLABLE_BYTES: usize = 2;
const MIN_SPELLING_BYTES: usize = 3;
const MIN_GROUP_BYTES: usize = 5;
const MIN_ENTRY_BYTES: usize = 6;

/// 拼写键：字节串 → [(音节 id, 类型)]。
type SpellingEntry = (Vec<u8>, Vec<(u32, u8)>);

/// 词条组（同码）。
struct Group {
    code: Vec<u16>,
    first: u32,
    count: u32,
}

/// 词条（文本在 `text` 池中切片）。
struct Entry {
    weight: f64,
    offset: u32,
    len: u16,
}

/// 拼音索引（TCSRV01；音反查与字反查共用）。
pub struct SoundToCharShapeIndex {
    syllables: Vec<String>,
    /// 拼写键（字节序）：键 → [(音节 id, 类型)]。
    spellings: Vec<SpellingEntry>,
    /// 词条组（按码字典序；前缀连续）。
    groups: Vec<Group>,
    /// 单字读音倒排（字反查用；源序，含多音字）。
    character_pinyin: hashbrown::HashMap<char, Vec<String>>,
    text: String,
    entries: Vec<Entry>,
}

impl SoundToCharShapeIndex {
    /// 载入索引（`gzip` 由魔数识别）。
    pub fn load(path: &Path) -> Result<Self> {
        let raw = std::fs::read(path).with_context(|| format!("读取 {}", path.display()))?;
        let data = if raw.starts_with(&[0x1f, 0x8b]) {
            let mut decoder = flate2::read::GzDecoder::new(raw.as_slice());
            let mut out = Vec::new();
            std::io::Read::read_to_end(&mut decoder, &mut out)
                .with_context(|| format!("解压 {}", path.display()))?;
            out
        } else {
            raw
        };
        Self::parse(&data).with_context(|| format!("解析 {}", path.display()))
    }

    fn parse(data: &[u8]) -> Result<Self> {
        let mut reader = Reader { data, pos: 0 };
        if reader.take(8)? != MAGIC {
            bail!("bad magic");
        }
        let syllable_count = reader.u32()? as usize;
        let spelling_count = reader.u32()? as usize;
        let group_count = reader.u32()? as usize;
        let entry_count = reader.u32()? as usize;
        // 组码是 `u16`：音节 id 超出该宽度时无法与任何组码相等，且 `collect_chunks`
        // 的 `edge.syllable as u16` 会把它静默截断成**别的**音节而查到错误的组。
        if syllable_count > u16::MAX as usize {
            bail!("syllable count exceeds the group code width: {syllable_count}");
        }
        let mut syllables = Vec::with_capacity(reader.capacity(syllable_count, MIN_SYLLABLE_BYTES));
        for _ in 0..syllable_count {
            syllables.push(String::from_utf8(reader.bytes()?.to_vec())?);
        }
        let mut spellings = Vec::with_capacity(reader.capacity(spelling_count, MIN_SPELLING_BYTES));
        for _ in 0..spelling_count {
            let key = reader.bytes()?.to_vec();
            let alt_count = reader.u8()? as usize;
            let mut alts = Vec::with_capacity(alt_count);
            for _ in 0..alt_count {
                let syllable = reader.u32()?;
                let kind = reader.u8()?;
                // 解析期校验：音节 id 必须落在音节表内（否则建边/查组越界）。
                if syllable as usize >= syllable_count {
                    bail!("spelling syllable id out of range: {syllable} >= {syllable_count}");
                }
                if kind > TYPE_ABBREV {
                    bail!("unknown spelling kind: {kind}");
                }
                alts.push((syllable, kind));
            }
            spellings.push((key, alts));
        }
        let mut groups = Vec::with_capacity(reader.capacity(group_count, MIN_GROUP_BYTES));
        for _ in 0..group_count {
            let count = reader.u8()? as usize;
            let mut code = Vec::with_capacity(count);
            for _ in 0..count {
                let id = reader.u16()?;
                if id as usize >= syllable_count {
                    bail!("group syllable id out of range: {id} >= {syllable_count}");
                }
                code.push(id);
            }
            let entries = reader.u32()?;
            groups.push(Group {
                code,
                first: 0,
                count: entries,
            });
        }
        let mut entries = Vec::with_capacity(reader.capacity(entry_count, MIN_ENTRY_BYTES));
        let mut text = String::new();
        for _ in 0..entry_count {
            let weight = f64::from(reader.u32()?);
            let bytes = reader.bytes()?;
            let offset = u32::try_from(text.len()).context("text pool exceeds u32 offsets")?;
            text.push_str(std::str::from_utf8(bytes)?);
            entries.push(Entry {
                weight,
                offset,
                len: bytes.len() as u16,
            });
        }
        // 组 → 词条区间：`checked_add` 保证偏移不 `u32` 回绕（回绕能骗过末尾的
        // 「总数一致」校验，随后在按组切片处越界 panic），并逐组保证
        // `first + count <= entry_count`（切片的实际边界依据）。
        let mut first = 0u32;
        for group in groups.iter_mut() {
            group.first = first;
            first = first
                .checked_add(group.count)
                .context("group entry offsets overflow")?;
            if first as usize > entry_count {
                bail!("group entry count exceeds the entry count: {first} > {entry_count}");
            }
        }
        if first as usize != entry_count {
            bail!("entry count mismatch");
        }
        // 单字读音倒排：组内音节串按源序收集，去重。
        let mut character_pinyin: hashbrown::HashMap<char, Vec<String>> = hashbrown::HashMap::new();
        {
            let entry_text = |entry: &Entry| -> &str {
                let start = entry.offset as usize;
                &text[start..start + entry.len as usize]
            };
            for group in &groups {
                // 音节 id 与组区间均已在上方校验 ⇒ 索引与切片在界内。
                let end = group.first + group.count;
                // 读音串**按需**构造（复核整改 3b / C7）：真实索引 600,869 组里只有
                // 412 组含单字词条，先前的「每组 `Vec<&str>` + `join`」在首次反查时
                // 白付约 60 万次分配。
                let mut reading: Option<String> = None;
                for entry in &entries[group.first as usize..end as usize] {
                    let mut chars = entry_text(entry).chars();
                    let (Some(ch), None) = (chars.next(), chars.next()) else {
                        continue;
                    };
                    let reading = reading.get_or_insert_with(|| {
                        group
                            .code
                            .iter()
                            .map(|id| syllables[*id as usize].as_str())
                            .collect::<Vec<_>>()
                            .join("")
                    });
                    let readings = character_pinyin.entry(ch).or_default();
                    if !readings.contains(reading) {
                        readings.push(reading.clone());
                    }
                }
            }
        }
        Ok(Self {
            character_pinyin,
            syllables,
            spellings,
            groups,
            text,
            entries,
        })
    }

    /// 音节数量（**当前唯一读取方是测试**：解析自检；保留为诊断面）。
    pub fn syllable_count(&self) -> usize {
        self.syllables.len()
    }

    /// 词条数（诊断）。
    pub fn entry_count(&self) -> usize {
        self.entries.len()
    }

    /// 单字读音（源序；无记录返回空切片）。
    pub fn character_pinyin(&self, ch: char) -> &[String] {
        self.character_pinyin
            .get(&ch)
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }

    fn entry_text(&self, entry: &Entry) -> &str {
        let start = entry.offset as usize;
        &self.text[start..start + entry.len as usize]
    }

    /// 精确查找码所在组。
    fn group(&self, code: &[u16]) -> Option<&Group> {
        self.groups
            .binary_search_by(|group| group.code.as_slice().cmp(code))
            .ok()
            .map(|index| &self.groups[index])
    }

    /// 是否存在以 `code` 为前缀的码（路径剪枝用）。
    fn prefix_exists(&self, code: &[u16]) -> bool {
        let index = self
            .groups
            .partition_point(|group| group.code.as_slice() < code);
        self.groups
            .get(index)
            .is_some_and(|group| group.code.starts_with(code))
    }
}

/// 载入目录序列中首个存在的索引（用户目录 → 共享目录；`.gz` 与未压缩皆可）。
pub fn load_first(dirs: &[PathBuf]) -> (Option<SoundToCharShapeIndex>, Option<String>) {
    let mut errors = Vec::new();
    for dir in dirs {
        for name in [SOUND_TO_CHAR_SHAPE_FILE, SOUND_TO_CHAR_SHAPE_FILE_GZ] {
            let path = dir.join(name);
            if path.is_file() {
                match SoundToCharShapeIndex::load(&path) {
                    Ok(index) => return (Some(index), None),
                    Err(error) => errors.push(format!("{error:#}")),
                }
            }
        }
    }
    if errors.is_empty() {
        (None, None)
    } else {
        (None, Some(errors.join("; ")))
    }
}

/// 音反查翻译（参照 `ReverseLookupTranslator::Query`）：`input` 为段输入（含前缀）。
#[allow(clippy::too_many_arguments)]
pub fn translate(
    index: &SoundToCharShapeIndex,
    lexicon: &Lexicon,
    input: &[u8],
    prefix: char,
    start: usize,
    end: usize,
    punct: Option<&PunctTable>,
    pairs: &mut PairState,
    full_shape: bool,
    limit: usize,
) -> Vec<Candidate> {
    let prefix_byte = prefix as u8;
    let code = if input.first() == Some(&prefix_byte) {
        &input[prefix.len_utf8()..]
    } else {
        input
    };
    // 前缀单独成段：`punct` 段与音反查段同区间，参照里由标点翻译器给出候选。
    if code.is_empty() {
        return punct_candidate(punct, pairs, prefix, full_shape, start, end)
            .into_iter()
            .collect();
    }
    let len = code.len();
    let mut edges = build_edges(index, code);
    let types = path_types(&edges, len);
    // `path_types` 恒置 `types[0]`（见其定义）⇒ 该兜底分支不可达，保留为防御。
    let Some(farthest) = (0..=len).rev().find(|&position| types[position].is_some()) else {
        return Vec::new();
    };
    // 参照 `BuildSyllableGraph` 的剪枝：最远顶点的最优拼写类型决定「缩写/补全」是否被弃
    // （全拼可达时缩写一律弃用，见 docs/rust-migration.md）。
    let last_type = types[farthest].unwrap_or(KIND_NORMAL).max(KIND_FUZZY);
    prune(&mut edges, &types, farthest, last_type);
    if farthest < len && !complete(index, &mut edges, code, farthest) {
        return Vec::new();
    }
    let chunks = collect_chunks(index, &edges, code, len);
    let code_prefix = String::from_utf8_lossy(&input[..prefix.len_utf8()]).into_owned();
    let mut candidates = emit(index, &chunks, &code_prefix, start, end, limit);
    code_comment_filter(&mut candidates, true, lexicon);
    candidates
}

/// 图的一条边（拼写键的一次匹配）。
#[derive(Clone, Copy)]
struct Edge {
    end: usize,
    syllable: u32,
    kind: u8,
    penalty: f64,
}

/// 建立拼写边（按音节 id、终点排序；与参照 `Transpose` 的索引序一致）。
fn build_edges(index: &SoundToCharShapeIndex, code: &[u8]) -> Vec<Vec<Edge>> {
    let len = code.len();
    let mut edges: Vec<Vec<Edge>> = (0..=len).map(|_| Vec::new()).collect();
    for position in 0..len {
        let rest = &code[position..];
        for (key, alts) in &index.spellings {
            if key.is_empty() || !rest.starts_with(key) {
                continue;
            }
            let end = position + key.len();
            for &(syllable, kind) in alts {
                edges[position].push(Edge {
                    end,
                    syllable,
                    kind: if kind == TYPE_ABBREV {
                        KIND_ABBREV
                    } else {
                        KIND_NORMAL
                    },
                    penalty: if kind == TYPE_NORMAL {
                        0.0
                    } else {
                        ABBREV_PENALTY
                    },
                });
            }
        }
        edges[position].sort_by_key(|edge| (edge.syllable, edge.end));
    }
    edges
}

/// 各顶点的最优拼写类型（路径上最差类型的最小值；参照 BFS 的优先队列语义）。
fn path_types(edges: &[Vec<Edge>], len: usize) -> Vec<Option<u8>> {
    let mut types: Vec<Option<u8>> = vec![None; len + 1];
    types[0] = Some(KIND_NORMAL);
    let mut queue = std::collections::VecDeque::new();
    queue.push_back(0usize);
    while let Some(position) = queue.pop_front() {
        let Some(current) = types[position] else {
            continue;
        };
        for edge in &edges[position] {
            let next = current.max(edge.kind);
            if types[edge.end].is_none_or(|known| next < known) {
                types[edge.end] = Some(next);
                queue.push_back(edge.end);
            }
        }
    }
    types
}

/// 参照 `BuildSyllableGraph` 的「remove stale vertices and edges」：
/// 从 `farthest` 向前逐顶点保留「类型可接受且有出边通往保留顶点」的顶点与边。
fn prune(edges: &mut [Vec<Edge>], types: &[Option<u8>], farthest: usize, last_type: u8) {
    let mut good = vec![false; edges.len()];
    good[farthest] = true;
    for position in (0..farthest).rev() {
        if types[position].is_none() || types[position].is_some_and(|kind| kind > last_type) {
            continue;
        }
        edges[position].retain(|edge| good[edge.end] && edge.kind <= last_type);
        if !edges[position].is_empty() {
            good[position] = true;
        }
    }
}

/// 尾部补全（参照 `BuildSyllableGraph` 的 completion 段）：`tail` 对应拼写键子树；
/// 本体拼写按补全罚、缩写保持自身罚。补全后不重跑剪枝。
fn complete(
    index: &SoundToCharShapeIndex,
    edges: &mut [Vec<Edge>],
    code: &[u8],
    farthest: usize,
) -> bool {
    let len = code.len();
    let tail = &code[farthest..];
    let mut added = false;
    for (key, alts) in &index.spellings {
        if !key.starts_with(tail) {
            continue;
        }
        for &(syllable, kind) in alts {
            edges[farthest].push(Edge {
                end: len,
                syllable,
                kind: if kind == TYPE_ABBREV {
                    KIND_ABBREV
                } else {
                    KIND_COMPLETION
                },
                penalty: if kind == TYPE_NORMAL {
                    COMPLETION_PENALTY
                } else {
                    ABBREV_PENALTY
                },
            });
            added = true;
        }
    }
    if !added {
        return false;
    }
    edges[farthest].sort_by_key(|edge| (edge.syllable, edge.end));
    true
}

/// 路径块（同码词条区间 + 可信度 + 按音节切分的输入切片）。
struct Chunk {
    first: u32,
    count: u32,
    penalty: f64,
    /// 预编辑（按音节切分；不含音反查前缀）。
    preedit: String,
}

/// 广度优先收集「码恰好等于路径音节序列」的词条块（参照 `Table::Query` 的推入序）。
/// `code` 用于生成「按音节分码」的预编辑：上一段为全拼（正常拼写）时在下一个音节前插空格，
/// 缩写/补全段与后续合并（如 `` `zhongguo `` → `` `zhong guo ``、`` `zho `` → `` `zho ``）。
fn collect_chunks(
    index: &SoundToCharShapeIndex,
    edges: &[Vec<Edge>],
    code: &[u8],
    len: usize,
) -> Vec<Chunk> {
    let mut chunks = Vec::new();
    let mut queue = std::collections::VecDeque::new();
    queue.push_back((
        0usize,
        Vec::<u16>::new(),
        0.0f64,
        String::new(),
        KIND_NORMAL,
    ));
    while let Some((position, path, penalty, preedit, last_kind)) = queue.pop_front() {
        for edge in &edges[position] {
            let mut next_path = path.clone();
            next_path.push(edge.syllable as u16);
            let next_penalty = penalty + edge.penalty;
            let mut next_preedit = preedit.clone();
            if !next_preedit.is_empty() && last_kind == KIND_NORMAL {
                next_preedit.push(' ');
            }
            next_preedit.push_str(&String::from_utf8_lossy(&code[position..edge.end]));
            if let Some(group) = index.group(&next_path) {
                if edge.end == len {
                    chunks.push(Chunk {
                        first: group.first,
                        count: group.count,
                        penalty: next_penalty,
                        preedit: next_preedit.clone(),
                    });
                }
            }
            if edge.end < len && index.prefix_exists(&next_path) {
                queue.push_back((edge.end, next_path, next_penalty, next_preedit, edge.kind));
            }
        }
    }
    chunks
}

/// 按「可信度 + ln(权重)」降序逐条产出（并列取块序在前者；参照 `DictEntryIterator::Sort`）。
fn emit(
    index: &SoundToCharShapeIndex,
    chunks: &[Chunk],
    code_prefix: &str,
    start: usize,
    end: usize,
    limit: usize,
) -> Vec<Candidate> {
    let mut result = Vec::new();
    // 块内游标（参照 `DictEntryIterator` 的增量位置；本仓每次查询从 0 起 ⇒ 恒 0）。
    let mut cursors: Vec<u32> = vec![0; chunks.len()];
    while result.len() < limit {
        let mut best: Option<(usize, f64)> = None;
        for (position, chunk) in chunks.iter().enumerate() {
            if cursors[position] >= chunk.count {
                continue;
            }
            let entry = &index.entries[(chunk.first + cursors[position]) as usize];
            let key = chunk.penalty
                + if entry.weight > 0.0 {
                    entry.weight.ln()
                } else {
                    ZERO_WEIGHT_LOG
                };
            if best.is_none_or(|(_, best_key)| key > best_key) {
                best = Some((position, key));
            }
        }
        let Some((position, _)) = best else {
            break;
        };
        let entry = &index.entries[(chunks[position].first + cursors[position]) as usize];
        let mut candidate =
            Candidate::new("reverse_lookup", start, end, index.entry_text(entry), "");
        candidate.preedit = format!("{code_prefix}{}", chunks[position].preedit);
        result.push(candidate);
        cursors[position] += 1;
    }
    result
}

/// 裸前缀的标点候选（参照 `PunctTranslator` 与 `CreatePunctCandidate`）；
/// 字反查的「默认可上屏候选」复用同一实现。
pub(crate) fn punct_candidate(
    punct: Option<&PunctTable>,
    pairs: &mut PairState,
    prefix: char,
    full_shape: bool,
    start: usize,
    end: usize,
) -> Option<Candidate> {
    let text = punct?.resolve(prefix, full_shape, pairs)?;
    let comment = punct_shape_comment(&text);
    let mut candidate = Candidate::new("punct", start, end, &text, &comment);
    if end.saturating_sub(start) == 1 {
        candidate.preedit = text;
    }
    Some(candidate)
}

/// 参照 `CreatePunctCandidate` 的形状注释（单个 Unicode 字符时给出〔半角〕/〔全角〕）。
fn punct_shape_comment(punct: &str) -> String {
    let mut chars = punct.chars();
    let Some(ch) = chars.next() else {
        return String::new();
    };
    if chars.next().is_some() {
        return String::new();
    }
    let code = ch as u32;
    let is_ascii = (0x20..0x7f).contains(&code);
    let is_ideographic_space = code == 0x3000;
    let is_full_shape_ascii = (0xff01..=0xff5e).contains(&code);
    let is_kana = (0x30a1..=0x30fc).contains(&code)
        || [0x3001, 0x3002, 0x300c, 0x300d, 0x309b, 0x309c].contains(&code);
    let is_half_shape_kana = (0xff61..=0xff9f).contains(&code);
    let is_hangul = (0x3131..=0x3164).contains(&code);
    let is_half_shape_hangul = (0xffa0..=0xffdc).contains(&code);
    let is_full_shape_narrow_symbol =
        code == 0xff5f || code == 0xff60 || (0xffe0..=0xffe6).contains(&code);
    let is_narrow_symbol = [
        0x00a2, 0x00a3, 0x00a5, 0x00a6, 0x00ac, 0x00af, 0x2985, 0x2986,
    ]
    .contains(&code);
    let is_half_shape_wide_symbol = (0xffe8..=0xffee).contains(&code);
    let is_wide_symbol =
        (0x2190..=0x2193).contains(&code) || code == 0x2502 || code == 0x25a0 || code == 0x25cb;
    let is_half_shape = is_ascii
        || is_half_shape_kana
        || is_half_shape_hangul
        || is_narrow_symbol
        || is_half_shape_wide_symbol;
    let is_full_shape = is_ideographic_space
        || is_full_shape_ascii
        || is_kana
        || is_hangul
        || is_full_shape_narrow_symbol
        || is_wide_symbol;
    if is_half_shape {
        "〔半角〕".to_string()
    } else if is_full_shape {
        "〔全角〕".to_string()
    } else {
        String::new()
    }
}

/// 音反查输入模式：`<前缀>[a-z']*`（参照 schema `recognizer/patterns/reverse_lookup`
/// = `^` + 前缀 + `[a-z']*$`）：撇号可出现在任意位置。
///
/// 口径事实（与本仓实现一致）：上游 `92a0b54` 把撇号同时放进 `speller/delimiter`，
/// 使反查段内按撇号**切分音节**（依赖上游 librime 的 delimiter 修复
/// [rime/librime#1233](https://github.com/rime/librime/pull/1233)；本机 librime 1.17.0
/// 未含该修复，故已入库金样里含撇号的反查段**无候选**）。
/// 本仓只落地「模式放行 + 撇号保留在输入中」，**不实现音节切分**：反查段由本段独占，
/// 音节按拼写键前缀匹配建边，而拼写表不含 `'` ⇒ 含撇号的反查段同样无候选（与金样一致）。
/// 撇号在 abc 段一侧的效果见 `interaction::translate::SEGMENTATION_DELIMITER`
/// 与 `docs/upstream-deviations.md` ③。
pub fn matches_pattern(input: &[u8], prefix: char) -> bool {
    let prefix = prefix as u8;
    let Some(rest) = input.strip_prefix(&[prefix][..]) else {
        return false;
    };
    rest.iter()
        .all(|&byte| byte.is_ascii_lowercase() || byte == b'\'')
}

struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn take(&mut self, count: usize) -> Result<&'a [u8]> {
        let end = self
            .pos
            .checked_add(count)
            .context("index offset overflow")?;
        if end > self.data.len() {
            bail!("truncated index");
        }
        let slice = &self.data[self.pos..end];
        self.pos = end;
        Ok(slice)
    }

    /// 剩余未读字节数。
    fn remaining(&self) -> usize {
        self.data.len() - self.pos
    }

    /// 容量钳制：文件头声明的记录数最多只能是「剩余字节 / 每条最小字节数」
    /// （畸形头部可声明 `u32::MAX` ⇒ 数十 GB 的预分配）。
    fn capacity(&self, declared: usize, min_record_bytes: usize) -> usize {
        declared.min(self.remaining() / min_record_bytes.max(1))
    }

    fn u8(&mut self) -> Result<u8> {
        Ok(self.take(1)?[0])
    }

    fn u16(&mut self) -> Result<u16> {
        Ok(u16::from_le_bytes(self.take(2)?.try_into()?))
    }

    fn u32(&mut self) -> Result<u32> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into()?))
    }

    fn bytes(&mut self) -> Result<&'a [u8]> {
        let len = self.u16()? as usize;
        self.take(len)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn fixture_index() -> SoundToCharShapeIndex {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../../goldens/sound_to_char_shape/tiger_sentence.pinyin.bin");
        SoundToCharShapeIndex::load(&path).expect("fixture index")
    }

    #[test]
    fn pattern_matches_reference_recognizer() {
        // 参照 `tiger_sentence.schema.yaml` @92a0b54：`^`[a-z']*$`；
        // 上游 `tools/test_reverse_lookup.lua` 同一批断言。
        assert!(matches_pattern(b"`", '`'));
        assert!(matches_pattern(b"`zhong", '`'));
        assert!(matches_pattern(b"`xi'", '`'));
        assert!(matches_pattern(b"`xi'a", '`'));
        assert!(matches_pattern(b"`xi'an", '`'));
        assert!(matches_pattern(b"`xi'an'", '`'));
        assert!(!matches_pattern(b"`Z", '`'));
        assert!(!matches_pattern(b"a`", '`'));
        assert!(!matches_pattern(b"``", '`'));
        assert!(!matches_pattern(b"`1", '`'));
        assert!(!matches_pattern(b"`ni2", '`'));
        assert!(!matches_pattern(b"`xi'an2", '`'));
        assert!(!matches_pattern(b"`xi a", '`'));
        assert!(!matches_pattern(b"xi'an", '`'));
    }

    #[test]
    fn punct_shape_comments_match_reference() {
        assert_eq!(punct_shape_comment("`"), "〔半角〕");
        assert_eq!(punct_shape_comment("｀"), "〔全角〕");
        assert_eq!(punct_shape_comment(""), "");
        assert_eq!(punct_shape_comment("ab"), "");
    }

    #[test]
    fn fixture_index_reports_counts() {
        let index = fixture_index();
        assert_eq!(index.syllable_count(), 14);
        assert_eq!(index.entry_count(), 22);
    }

    #[test]
    fn translate_matches_abbrev_and_pruning_candidates() {
        let index = fixture_index();
        let lexicon = Lexicon::load(&[], 0);
        let texts = |input: &[u8]| -> Vec<String> {
            translate(
                &index,
                &lexicon,
                input,
                '`',
                0,
                input.len(),
                None,
                &mut PairState::default(),
                false,
                CANDIDATE_LIMIT,
            )
            .into_iter()
            .map(|candidate| candidate.text)
            .collect()
        };
        // 缩写路径（「zh」+「o」）与剪枝（zhou 全拼可达 → 缩写全弃）。
        assert_eq!(
            texts(b"`zho"),
            ["中哦", "中龘", "中欧", "找哦", "兆欧", "找欧"]
        );
        assert_eq!(texts(b"`zhou"), ["周", "轴"]);
        assert_eq!(texts(b"`zhong"), ["中", "重", "种", "钟", "垚"]);
        assert!(texts(b"`zhon").is_empty());
        assert!(texts(b"`zuo").is_empty());
    }

    #[test]
    fn translate_segments_preedit_by_syllable() {
        let index = fixture_index();
        let lexicon = Lexicon::load(&[], 0);
        let preedits = |input: &[u8]| -> Vec<String> {
            translate(
                &index,
                &lexicon,
                input,
                '`',
                0,
                input.len(),
                None,
                &mut PairState::default(),
                false,
                CANDIDATE_LIMIT,
            )
            .into_iter()
            .map(|candidate| candidate.preedit)
            .collect()
        };
        // 预编辑「按音节分码」：全拼段之后插空格；缩写段与后续合并。
        assert_eq!(preedits(b"`zhong")[0], "`zhong");
        assert_eq!(preedits(b"`zhongguo")[0], "`zhong guo");
        assert_eq!(preedits(b"`zhongg")[0], "`zhong g");
        assert_eq!(preedits(b"`zho")[0], "`zho");
    }

    // ------------------------------------------------------------ 畸形索引加固
    //
    // 索引从**用户目录优先**的数据目录懒加载（`decode.rs`），畸形/被替换的
    // `tiger_sentence.pinyin.bin[.gz]` 必须返回诊断而非 panic（addon 内 panic
    // 跨 FFI 即进程终止）。以下用例逐一构造畸形字节。

    /// 一个拼写条目：`(拼写字节, [(音节下标, 该音节内的拼写键下标)])`。
    type SpellingEntry = (Vec<u8>, Vec<(u32, u8)>);

    /// TCSRV01 构造器（字段序与 [`SoundToCharShapeIndex::parse`] 一致）。
    #[derive(Default)]
    struct IndexBuilder {
        syllables: Vec<String>,
        spellings: Vec<SpellingEntry>,
        groups: Vec<(Vec<u16>, u32)>,
        entries: Vec<(u32, String)>,
    }

    impl IndexBuilder {
        fn push16(out: &mut Vec<u8>, value: u16) {
            out.extend_from_slice(&value.to_le_bytes());
        }

        fn push32(out: &mut Vec<u8>, value: u32) {
            out.extend_from_slice(&value.to_le_bytes());
        }

        /// 序列化；`header` 覆盖四个计数（构造畸形头部用）。
        fn bytes_with(&self, header: [u32; 4]) -> Vec<u8> {
            let mut out = MAGIC.to_vec();
            for value in header {
                Self::push32(&mut out, value);
            }
            for syllable in &self.syllables {
                Self::push16(&mut out, syllable.len() as u16);
                out.extend_from_slice(syllable.as_bytes());
            }
            for (key, alts) in &self.spellings {
                Self::push16(&mut out, key.len() as u16);
                out.extend_from_slice(key);
                out.push(alts.len() as u8);
                for &(syllable, kind) in alts {
                    Self::push32(&mut out, syllable);
                    out.push(kind);
                }
            }
            for (code, entries) in &self.groups {
                out.push(code.len() as u8);
                for &id in code {
                    Self::push16(&mut out, id);
                }
                Self::push32(&mut out, *entries);
            }
            for (weight, text) in &self.entries {
                Self::push32(&mut out, *weight);
                Self::push16(&mut out, text.len() as u16);
                out.extend_from_slice(text.as_bytes());
            }
            out
        }

        fn bytes(&self) -> Vec<u8> {
            self.bytes_with([
                self.syllables.len() as u32,
                self.spellings.len() as u32,
                self.groups.len() as u32,
                self.entries.len() as u32,
            ])
        }
    }

    /// `parse` 的失败路径（`SoundToCharShapeIndex` 未派生 `Debug`，不用 `expect_err`）。
    fn parse_error(bytes: &[u8]) -> anyhow::Error {
        match SoundToCharShapeIndex::parse(bytes) {
            Ok(_) => panic!("畸形索引不应解析成功"),
            Err(error) => error,
        }
    }

    /// 正对照：合法字节流仍照常解析（校验不得误伤正常索引）。
    #[test]
    fn parse_accepts_well_formed_bytes() {
        let builder = IndexBuilder {
            syllables: vec!["zhong".to_string(), "guo".to_string()],
            spellings: vec![
                (b"zhong".to_vec(), vec![(0, TYPE_NORMAL)]),
                (b"guo".to_vec(), vec![(1, TYPE_ABBREV)]),
            ],
            groups: vec![(vec![0, 1], 2)],
            entries: vec![(3, "中国".to_string()), (1, "中".to_string())],
        };
        let index = SoundToCharShapeIndex::parse(&builder.bytes()).expect("合法索引");
        assert_eq!(index.syllable_count(), 2);
        assert_eq!(index.entry_count(), 2);
        assert_eq!(index.character_pinyin('中'), ["zhongguo".to_string()]);
        // 头部计数与实际记录数的关系仍被校验（少一条即报错）。
        let mut short = builder.bytes_with([2, 2, 1, 1]);
        short.truncate(short.len() - (4 + 2 + "中".len()));
        assert!(SoundToCharShapeIndex::parse(&short).is_err());
    }

    /// 越界音节 id（拼写变体侧）：解析失败，不得在建边/查组时越界。
    #[test]
    fn parse_rejects_spelling_syllable_id_out_of_range() {
        let builder = IndexBuilder {
            syllables: vec!["a".to_string()],
            spellings: vec![(b"a".to_vec(), vec![(1, TYPE_NORMAL)])],
            groups: vec![],
            entries: vec![],
        };
        let error = parse_error(&builder.bytes());
        assert!(
            error.to_string().contains("syllable id out of range"),
            "{error}"
        );
        // kind 只有 0/1 两种合法取值。
        let builder = IndexBuilder {
            syllables: vec!["a".to_string()],
            spellings: vec![(b"a".to_vec(), vec![(0, 2)])],
            groups: vec![],
            entries: vec![],
        };
        assert!(SoundToCharShapeIndex::parse(&builder.bytes()).is_err());
    }

    /// 越界音节 id（组码侧）与超出 `u16` 宽度的音节表：解析失败。
    #[test]
    fn parse_rejects_group_syllable_id_out_of_range() {
        let builder = IndexBuilder {
            syllables: vec!["a".to_string()],
            spellings: vec![],
            groups: vec![(vec![1], 0)],
            entries: vec![],
        };
        let error = parse_error(&builder.bytes());
        assert!(
            error.to_string().contains("syllable id out of range"),
            "{error}"
        );
        // 音节数超过组码宽度（`u16`）：组码表示不了 ⇒ 拒绝（否则 `as u16` 静默截断）。
        let builder = IndexBuilder {
            syllables: vec!["a".to_string()],
            ..Default::default()
        };
        let header = [65_536u32, 0, 0, 0];
        let error = parse_error(&builder.bytes_with(header));
        assert!(error.to_string().contains("group code width"), "{error}");
    }

    /// 组条目数回绕：两组各 `0x8000_0000`、总条目数 0 —— `first += count` 回绕后
    /// **恰好**满足原来的「总数一致」校验，随后按组切片越界（`&entries[0..0x8000_0000]`）。
    /// 修法后由「逐组 `first + count <= entry_count`」先拦下（`checked_add` 是第二道）。
    #[test]
    fn parse_rejects_group_entry_offsets_that_wrap() {
        let builder = IndexBuilder {
            syllables: vec!["a".to_string()],
            spellings: vec![],
            groups: vec![(vec![0], 0x8000_0000), (vec![0], 0x8000_0000)],
            entries: vec![],
        };
        let bytes = builder.bytes_with([1, 0, 2, 0]);
        let error = parse_error(&bytes);
        assert!(
            error.to_string().contains("entry count"),
            "回绕必须在切片前被拒绝：{error}"
        );
        // `checked_add` 自身：单组 `u32::MAX` 已够（`entry_count` 也取 `u32::MAX`
        // 才能越过逐组上界检查，但条目循环会先因数据不足失败 —— 这里只要求
        // 「任何畸形头部都不得走到切片」）。
        let builder = IndexBuilder {
            syllables: vec!["a".to_string()],
            spellings: vec![],
            groups: vec![(vec![0], u32::MAX), (vec![0], 1)],
            entries: vec![],
        };
        assert!(SoundToCharShapeIndex::parse(&builder.bytes()).is_err());
    }

    /// 单组条目数超过总条目数：即便总和不回绕也要拒绝（切片边界依据）。
    #[test]
    fn parse_rejects_group_entry_count_exceeding_entries() {
        let builder = IndexBuilder {
            syllables: vec!["a".to_string()],
            spellings: vec![],
            groups: vec![(vec![0], 5)],
            entries: vec![(1, "甲".to_string())],
        };
        let error = parse_error(&builder.bytes_with([1, 0, 1, 1]));
        assert!(
            error.to_string().contains("exceeds the entry count"),
            "{error}"
        );
    }

    /// 巨型头部计数（`u32::MAX`）：容量按剩余字节钳制 ⇒ 快速失败而非巨额分配
    /// （未钳制时 `Vec::with_capacity(u32::MAX)` 直接 OOM/abort）。
    #[test]
    fn parse_rejects_oversized_header_counts_without_huge_allocation() {
        let builder = IndexBuilder::default();
        for header in [
            [u32::MAX, 0, 0, 0],
            [0, u32::MAX, 0, 0],
            [0, 0, u32::MAX, 0],
            [0, 0, 0, u32::MAX],
        ] {
            let error = parse_error(&builder.bytes_with(header));
            let text = error.to_string();
            assert!(
                text.contains("truncated index") || text.contains("group code width"),
                "巨型头部必须以诊断失败：{text}"
            );
        }
    }

    /// 截断文件：头部声明多于实际字节 ⇒ 诊断（不 panic）；`load` 侧带路径上下文。
    #[test]
    fn parse_rejects_truncated_index() {
        let builder = IndexBuilder {
            syllables: vec!["zhong".to_string()],
            spellings: vec![(b"zhong".to_vec(), vec![(0, TYPE_NORMAL)])],
            groups: vec![(vec![0], 1)],
            entries: vec![(1, "中".to_string())],
        };
        let full = builder.bytes();
        for cut in [8, 9, 12, 20, full.len() - 1] {
            let error = parse_error(&full[..cut]);
            assert!(!error.to_string().is_empty());
        }
        // 尾部文本长度前缀超出剩余字节。
        let mut bad = full.clone();
        let len = bad.len();
        bad[len - 3..len - 1].copy_from_slice(&0xffffu16.to_le_bytes());
        assert!(SoundToCharShapeIndex::parse(&bad).is_err());
    }

    /// `load` 对畸形**文件**同样返回诊断（不 panic）；gzip 分支同路。
    #[test]
    fn load_reports_malformed_files_without_panicking() {
        let dir = std::env::temp_dir().join(format!("hux-tcsrv-bad-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("临时目录");
        let path = dir.join(SOUND_TO_CHAR_SHAPE_FILE);
        std::fs::write(
            &path,
            b"TCSRV01\n\xff\xff\xff\xff\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00",
        )
        .expect("写畸形索引");
        let error = match SoundToCharShapeIndex::load(&path) {
            Ok(_) => panic!("畸形文件不应加载成功"),
            Err(error) => error,
        };
        assert!(error.to_string().contains("解析"), "{error}");
        let gz = dir.join(SOUND_TO_CHAR_SHAPE_FILE_GZ);
        let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        std::io::Write::write_all(&mut encoder, &std::fs::read(&path).expect("读回"))
            .expect("压缩");
        std::fs::write(&gz, encoder.finish().expect("gzip")).expect("写畸形 gz");
        assert!(SoundToCharShapeIndex::load(&gz).is_err());
        std::fs::remove_dir_all(&dir).expect("清理临时目录");
    }
}
