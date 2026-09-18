// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! 音查虎（⑧-1）：`tiger_sentence.pinyin.bin[.gz]`（TCSRV01）读取与音查虎翻译。
//!
//! 语义对齐 librime 1.17.0 的词典音查虎（`reverse_lookup_translator` + `ReverseLookupFilter`，
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
//! `code_comment`/`code_comment_filter` 定义于 `interaction`（K2 预留），此处沿用。

use crate::interaction::code_comment_filter;
use crate::lexicon::Lexicon;
use crate::punct::PunctTable;
use crate::session::Candidate;
use anyhow::{Context, Result, bail};
use std::path::{Path, PathBuf};

/// 索引文件名（发布为 `.gz`；fixture 常用未压缩）。
pub const PINYIN_FILE: &str = "tiger_sentence.pinyin.bin";
pub const PINYIN_FILE_GZ: &str = "tiger_sentence.pinyin.bin.gz";
/// 音查虎候选上限（与主候选一致；⑧ 裁决）。
pub const CANDIDATE_LIMIT: usize = 20;
/// 音查虎段标签（参照 schema 的 `reverse_lookup`）。
pub const PINYIN_LOOKUP_TAG: &str = "reverse_lookup";
/// 音查虎段提示（参照 schema `reverse_lookup/tips`）。
pub const PINYIN_LOOKUP_TIPS: &str = "〔拼音〕";

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

/// 拼音索引（TCSRV01；音查虎与字查音+虎共用）。
pub struct PinyinIndex {
    syllables: Vec<String>,
    /// 拼写键（字节序）：键 → [(音节 id, 类型)]。
    spellings: Vec<SpellingEntry>,
    /// 词条组（按码字典序；前缀连续）。
    groups: Vec<Group>,
    /// 单字读音倒排（字查音+虎用；源序，含多音字）。
    character_pinyin: hashbrown::HashMap<char, Vec<String>>,
    text: String,
    entries: Vec<Entry>,
}

impl PinyinIndex {
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
        let mut syllables = Vec::with_capacity(syllable_count);
        for _ in 0..syllable_count {
            syllables.push(String::from_utf8(reader.bytes()?.to_vec())?);
        }
        let mut spellings = Vec::with_capacity(spelling_count);
        for _ in 0..spelling_count {
            let key = reader.bytes()?.to_vec();
            let alt_count = reader.u8()? as usize;
            let mut alts = Vec::with_capacity(alt_count);
            for _ in 0..alt_count {
                let syllable = reader.u32()?;
                let kind = reader.u8()?;
                alts.push((syllable, kind));
            }
            spellings.push((key, alts));
        }
        let mut groups = Vec::with_capacity(group_count);
        for _ in 0..group_count {
            let count = reader.u8()? as usize;
            let mut code = Vec::with_capacity(count);
            for _ in 0..count {
                code.push(reader.u16()?);
            }
            let entries = reader.u32()?;
            groups.push(Group {
                code,
                first: 0,
                count: entries,
            });
        }
        let mut entries = Vec::with_capacity(entry_count);
        let mut text = String::new();
        for _ in 0..entry_count {
            let weight = f64::from(reader.u32()?);
            let bytes = reader.bytes()?;
            let offset = text.len() as u32;
            text.push_str(std::str::from_utf8(bytes)?);
            entries.push(Entry {
                weight,
                offset,
                len: bytes.len() as u16,
            });
        }
        let mut first = 0u32;
        for group in groups.iter_mut() {
            group.first = first;
            first += group.count;
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
                let reading: String = group
                    .code
                    .iter()
                    .map(|id| syllables[*id as usize].as_str())
                    .collect::<Vec<_>>()
                    .join("");
                let end = group.first + group.count;
                for entry in &entries[group.first as usize..end as usize] {
                    let mut chars = entry_text(entry).chars();
                    let (Some(ch), None) = (chars.next(), chars.next()) else {
                        continue;
                    };
                    let readings = character_pinyin.entry(ch).or_default();
                    if !readings.contains(&reading) {
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

    /// 音节数量（诊断）。
    pub fn syllable_count(&self) -> usize {
        self.syllables.len()
    }

    /// 组数（诊断）。
    pub fn group_count(&self) -> usize {
        self.groups.len()
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
pub fn load_first(dirs: &[PathBuf]) -> (Option<PinyinIndex>, Option<String>) {
    let mut errors = Vec::new();
    for dir in dirs {
        for name in [PINYIN_FILE, PINYIN_FILE_GZ] {
            let path = dir.join(name);
            if path.is_file() {
                match PinyinIndex::load(&path) {
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

/// 音查虎翻译（参照 `ReverseLookupTranslator::Query`）：`input` 为段输入（含前缀）。
#[allow(clippy::too_many_arguments)]
pub fn translate(
    index: &PinyinIndex,
    lexicon: &Lexicon,
    input: &[u8],
    prefix: char,
    start: usize,
    end: usize,
    punct: Option<&mut PunctTable>,
    full_shape: bool,
    limit: usize,
) -> Vec<Candidate> {
    let prefix_byte = prefix as u8;
    let code = if input.first() == Some(&prefix_byte) {
        &input[prefix.len_utf8()..]
    } else {
        input
    };
    // 前缀单独成段：`punct` 段与音查虎段同区间，参照里由标点翻译器给出候选。
    if code.is_empty() {
        return punct_candidate(punct, prefix, full_shape, start, end)
            .into_iter()
            .collect();
    }
    let len = code.len();
    let mut edges = build_edges(index, code);
    let types = path_types(&edges, len);
    let farthest = (0..=len).rev().find(|&position| types[position].is_some());
    let Some(mut farthest) = farthest else {
        return Vec::new();
    };
    // 参照 `BuildSyllableGraph` 的剪枝：最远顶点的最优拼写类型决定「缩写/补全」是否被弃
    // （全拼可达时缩写一律弃用，见 docs/rust-migration.md）。
    let last_type = types[farthest].unwrap_or(KIND_NORMAL).max(KIND_FUZZY);
    prune(&mut edges, &types, farthest, last_type);
    if farthest < len && !complete(index, &mut edges, code, farthest) {
        return Vec::new();
    }
    farthest = len;
    let _ = farthest;
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
fn build_edges(index: &PinyinIndex, code: &[u8]) -> Vec<Vec<Edge>> {
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
fn complete(index: &PinyinIndex, edges: &mut [Vec<Edge>], code: &[u8], farthest: usize) -> bool {
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
    cursor: u32,
    penalty: f64,
    /// 预编辑（按音节切分；不含音查虎前缀）。
    preedit: String,
}

/// 广度优先收集「码恰好等于路径音节序列」的词条块（参照 `Table::Query` 的推入序）。
/// `code` 用于生成「按音节分码」的预编辑：上一段为全拼（正常拼写）时在下一个音节前插空格，
/// 缩写/补全段与后续合并（如 `` `zhongguo `` → `` `zhong guo ``、`` `zho `` → `` `zho ``）。
fn collect_chunks(index: &PinyinIndex, edges: &[Vec<Edge>], code: &[u8], len: usize) -> Vec<Chunk> {
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
                        cursor: 0,
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
    index: &PinyinIndex,
    chunks: &[Chunk],
    code_prefix: &str,
    start: usize,
    end: usize,
    limit: usize,
) -> Vec<Candidate> {
    let mut result = Vec::new();
    let mut cursors: Vec<u32> = chunks.iter().map(|chunk| chunk.cursor).collect();
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
/// 字查音+虎的「默认可上屏候选」复用同一实现。
pub(crate) fn punct_candidate(
    punct: Option<&mut PunctTable>,
    prefix: char,
    full_shape: bool,
    start: usize,
    end: usize,
) -> Option<Candidate> {
    let text = punct?.resolve(prefix, full_shape)?;
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

/// 音查虎输入模式：`<前缀>[a-z]*'?`（参照 schema `recognizer/patterns/reverse_lookup`）。
pub fn matches_pattern(input: &[u8], prefix: char) -> bool {
    let prefix = prefix as u8;
    let Some(rest) = input.strip_prefix(&[prefix][..]) else {
        return false;
    };
    let mut quote = false;
    for &byte in rest {
        if byte.is_ascii_lowercase() && !quote {
            continue;
        }
        if byte == b'\'' && !quote {
            quote = true;
            continue;
        }
        return false;
    }
    true
}

struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn take(&mut self, count: usize) -> Result<&'a [u8]> {
        if self.pos + count > self.data.len() {
            bail!("truncated index");
        }
        let slice = &self.data[self.pos..self.pos + count];
        self.pos += count;
        Ok(slice)
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

    fn fixture_index() -> PinyinIndex {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../goldens/pinyin_lookup/tiger_sentence.pinyin.bin");
        PinyinIndex::load(&path).expect("fixture index")
    }

    #[test]
    fn pattern_matches_reference_recognizer() {
        assert!(matches_pattern(b"`", '`'));
        assert!(matches_pattern(b"`zhong", '`'));
        assert!(matches_pattern(b"`xi'", '`'));
        assert!(!matches_pattern(b"`xi'a", '`'));
        assert!(!matches_pattern(b"`Z", '`'));
        assert!(!matches_pattern(b"a`", '`'));
        assert!(!matches_pattern(b"``", '`'));
        assert!(!matches_pattern(b"`1", '`'));
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
}
