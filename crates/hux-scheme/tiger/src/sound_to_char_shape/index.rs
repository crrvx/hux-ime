// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! 音反查索引的字节格式与解析（`TCSRV01`）。
//!
//! 归属：文件头/计数校验、拼写表、词条组与词条区间、单字读音倒排，以及最小记录字节数
//! 钳制（声明计数不可信）。翻译侧见 [`super::graph`]，对外入口见 [`super`]。

use super::{SOUND_TO_CHAR_SHAPE_FILE, SOUND_TO_CHAR_SHAPE_FILE_GZ};
use anyhow::{Context, Result, bail};
use std::path::{Path, PathBuf};

pub(super) const MAGIC: &[u8; 8] = b"TCSRV01\n";
/// 拼写类型（同 librime `SpellingType` 序：normal < fuzzy < abbreviation < completion）。
pub(super) const KIND_NORMAL: u8 = 0;
pub(super) const KIND_FUZZY: u8 = 1;
pub(super) const KIND_ABBREV: u8 = 2;
pub(super) const KIND_COMPLETION: u8 = 3;
/// 索引中的类型标记（0 = 本体，1 = 缩写）。
pub(super) const TYPE_NORMAL: u8 = 0;
pub(super) const TYPE_ABBREV: u8 = 1;

/// 参照 `kAbbreviationPenalty = log(0.5)`。
pub(super) const ABBREV_PENALTY: f64 = -std::f64::consts::LN_2;
/// 参照 `kCompletionPenalty = log(0.05)`。
pub(super) const COMPLETION_PENALTY: f64 = -2.995732273553991;
/// 参照 `log(DBL_EPSILON)`（权重为 0 时）。
pub(super) const ZERO_WEIGHT_LOG: f64 = -36.04365338911715;
/// 各类记录的**最小**字节数（容量钳制用）：长度前缀 / 计数 / 权重等固定字段。
/// 文件头声明的计数不可信（可要求数十 GB 预分配），实际记录数受剩余字节数限制。
const MIN_SYLLABLE_BYTES: usize = 2;
const MIN_SPELLING_BYTES: usize = 3;
const MIN_GROUP_BYTES: usize = 5;
const MIN_ENTRY_BYTES: usize = 6;

/// 拼写键：字节串 → [(音节 id, 类型)]。
pub(super) type SpellingEntry = (Vec<u8>, Vec<(u32, u8)>);

/// 词条组（同码）。
pub(super) struct Group {
    code: Vec<u16>,
    pub(super) first: u32,
    pub(super) count: u32,
}

/// 词条（文本在 `text` 池中切片）。
pub(super) struct Entry {
    pub(super) weight: f64,
    offset: u32,
    len: u16,
}

/// 拼音索引（TCSRV01；音反查与字反查共用）。
pub struct SoundToCharShapeIndex {
    /// 拼写键（字节序）：键 → [(音节 id, 类型)]。
    pub(super) spellings: Vec<SpellingEntry>,
    /// 词条组（按码字典序；前缀连续）。
    groups: Vec<Group>,
    /// 单字读音倒排（字反查用；源序，含多音字）。
    character_pinyin: hashbrown::HashMap<char, Vec<String>>,
    text: String,
    pub(super) entries: Vec<Entry>,
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

    pub(super) fn parse(data: &[u8]) -> Result<Self> {
        let mut reader = Reader { data, pos: 0 };
        let (syllable_count, spelling_count, group_count, entry_count) = parse_header(&mut reader)?;
        let syllables = read_syllables(&mut reader, syllable_count)?;
        let spellings = read_spellings(&mut reader, spelling_count, syllable_count)?;
        let mut groups = read_groups(&mut reader, group_count, syllable_count)?;
        let (entries, text) = read_entries(&mut reader, entry_count)?;
        assign_group_ranges(&mut groups, entry_count)?;
        let character_pinyin = build_character_pinyin(&groups, &entries, &text, &syllables);
        Ok(Self {
            character_pinyin,
            spellings,
            groups,
            text,
            entries,
        })
    }

    /// 词条数（诊断面：读取方只有本文件单测）。
    #[cfg(test)]
    pub(super) fn entry_count(&self) -> usize {
        self.entries.len()
    }

    /// 单字读音（源序；无记录返回空切片）。
    pub fn character_pinyin(&self, ch: char) -> &[String] {
        self.character_pinyin
            .get(&ch)
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }

    pub(super) fn entry_text(&self, entry: &Entry) -> &str {
        let start = entry.offset as usize;
        &self.text[start..start + entry.len as usize]
    }

    /// 精确查找码所在组。
    pub(super) fn group(&self, code: &[u16]) -> Option<&Group> {
        self.groups
            .binary_search_by(|group| group.code.as_slice().cmp(code))
            .ok()
            .map(|index| &self.groups[index])
    }

    /// 是否存在以 `code` 为前缀的码（路径剪枝用）。
    pub(super) fn prefix_exists(&self, code: &[u16]) -> bool {
        let index = self
            .groups
            .partition_point(|group| group.code.as_slice() < code);
        self.groups
            .get(index)
            .is_some_and(|group| group.code.starts_with(code))
    }
}

/// 读取魔数与四个计数（音节 / 拼写 / 组 / 词条）。
fn parse_header(reader: &mut Reader<'_>) -> Result<(usize, usize, usize, usize)> {
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
    Ok((syllable_count, spelling_count, group_count, entry_count))
}

/// 读取音节表（每项为 UTF-8 字节串）。
fn read_syllables(reader: &mut Reader<'_>, syllable_count: usize) -> Result<Vec<String>> {
    let mut syllables = Vec::with_capacity(reader.capacity(syllable_count, MIN_SYLLABLE_BYTES));
    for _ in 0..syllable_count {
        syllables.push(String::from_utf8(reader.bytes()?.to_vec())?);
    }
    Ok(syllables)
}

/// 读取拼写表；逐条校验音节 id 与类型字节。
fn read_spellings(
    reader: &mut Reader<'_>,
    spelling_count: usize,
    syllable_count: usize,
) -> Result<Vec<SpellingEntry>> {
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
    Ok(spellings)
}

/// 读取词条组；组内音节 id 逐条校验，词条区间留待回填。
fn read_groups(
    reader: &mut Reader<'_>,
    group_count: usize,
    syllable_count: usize,
) -> Result<Vec<Group>> {
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
    Ok(groups)
}

/// 读取词条与文本池（文本池偏移按 `u32` 记录）。
fn read_entries(reader: &mut Reader<'_>, entry_count: usize) -> Result<(Vec<Entry>, String)> {
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
    Ok((entries, text))
}

/// 回填每组的词条区间（`first` 累加，必须在界内且与词条总数一致）。
fn assign_group_ranges(groups: &mut [Group], entry_count: usize) -> Result<()> {
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
    Ok(())
}

/// 构建单字读音倒排。
fn build_character_pinyin(
    groups: &[Group],
    entries: &[Entry],
    text: &str,
    syllables: &[String],
) -> hashbrown::HashMap<char, Vec<String>> {
    // 单字读音倒排：组内音节串按源序收集，去重。
    let mut character_pinyin: hashbrown::HashMap<char, Vec<String>> = hashbrown::HashMap::new();
    {
        let entry_text = |entry: &Entry| -> &str {
            let start = entry.offset as usize;
            &text[start..start + entry.len as usize]
        };
        for group in groups {
            // 音节 id 与组区间均已在上方校验 ⇒ 索引与切片在界内。
            let end = group.first + group.count;
            // 读音串**按需**构造：真实索引 600,869 组里只有
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
    character_pinyin
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
