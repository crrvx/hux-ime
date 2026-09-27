// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! 音反查的图翻译：折码、建图、补全、分段与候选生成。
//!
//! 分隔符语义见 [`super::SYLLABLE_DELIMITER`]：先把段输入折成紧凑码 + 边界掩码，
//! 建图、补全、预编辑都按它定位与断句；索引侧见 [`super::index`]。

use super::SYLLABLE_DELIMITER;
use super::index::{
    ABBREV_PENALTY, COMPLETION_PENALTY, KIND_ABBREV, KIND_COMPLETION, KIND_FUZZY, KIND_NORMAL,
    SoundToCharShapeIndex, TYPE_ABBREV, TYPE_NORMAL, ZERO_WEIGHT_LOG,
};
use super::punct_candidate;
use crate::lexicon::{Lexicon, code_comment_filter};
use hux_core::punct::{PairState, PunctTable};
use hux_core::session::Candidate;

/// 音反查翻译（参照 `ReverseLookupTranslator::Query`）：`input` 为段输入（含前缀）。
///
/// 分隔符语义见 `SYLLABLE_DELIMITER`：先折成 [`DelimitedCode`]（紧凑码 + 边界掩码），
/// 建图、补全、预编辑都按它定位与断句。
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
    let raw = if input.first() == Some(&prefix_byte) {
        &input[prefix.len_utf8()..]
    } else {
        input
    };
    // 前缀单独成段：`punct` 段与音反查段同区间，参照里由标点翻译器给出候选。
    if raw.is_empty() {
        return punct_candidate(punct, pairs, prefix, full_shape, start, end)
            .into_iter()
            .collect();
    }
    let code = DelimitedCode::new(raw);
    let mut edges = build_edges(index, &code);
    let types = path_types(&edges, code.len());
    // `path_types` 恒置 `types[0]`（见其定义）⇒ 反向查找必然命中，该兜底分支不可达；
    // `debug_assert!` 把不变式写明（release 下不生效，返回值行为不变）。
    let Some(farthest) = (0..=code.len())
        .rev()
        .find(|&position| types[position].is_some())
    else {
        debug_assert!(false, "path_types 恒置 types[0]，反向查找必然命中");
        return Vec::new();
    };
    // 参照 `BuildSyllableGraph` 的剪枝：最远顶点的最优拼写类型决定「缩写/补全」是否被弃
    // （全拼可达时缩写一律弃用）。
    let last_type = types[farthest].unwrap_or(KIND_NORMAL).max(KIND_FUZZY);
    prune(&mut edges, &types, farthest, last_type);
    if farthest < code.len() && !complete(index, &mut edges, &code, farthest) {
        return Vec::new();
    }
    let chunks = collect_chunks(index, &edges, &code);
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

/// 段输入去掉音节分隔符后的形态：紧凑码 + 边界掩码。
///
/// `boundary.len() == compact.len() + 1`，`boundary[i] == true` 表示 `compact[i - 1]` 与
/// `compact[i]` 之间原有分隔符 ⇒ `boundary[0]` = 段首、`boundary[compact.len()]` = 段尾；
/// 同一界上的连续分隔符并成一个（故连写与单写等价）。
struct DelimitedCode {
    compact: Vec<u8>,
    boundary: Vec<bool>,
}

impl DelimitedCode {
    fn new(code: &[u8]) -> Self {
        let mut compact = Vec::with_capacity(code.len());
        let mut boundary = vec![false];
        for &byte in code {
            if byte == SYLLABLE_DELIMITER {
                boundary[compact.len()] = true;
            } else {
                compact.push(byte);
                boundary.push(false);
            }
        }
        Self { compact, boundary }
    }

    fn len(&self) -> usize {
        self.compact.len()
    }

    /// `position..end`（要求 `position < end`）内没有分隔符 ⇒ 这段紧凑码是一个完整音节的拼写。
    /// 边界落在 `position` 或 `end` 上都算「没有」：分隔符只在音节之间断音，不吞掉两侧音节。
    fn unbroken(&self, position: usize, end: usize) -> bool {
        !self.boundary[position + 1..end].iter().any(|&flag| flag)
    }

    /// 紧凑码下标 `position`（含末尾哨兵 `len`）之前原有分隔符 ⇒ 预编辑在这里拼回撇号。
    fn delimiter_before(&self, position: usize) -> bool {
        self.boundary[position]
    }
}

/// 建立拼写边（按音节 id、终点排序；与参照 `Transpose` 的索引序一致）；音节不得跨分隔符
/// （见 [`DelimitedCode::unbroken`]）。
fn build_edges(index: &SoundToCharShapeIndex, code: &DelimitedCode) -> Vec<Vec<Edge>> {
    let len = code.len();
    let mut edges: Vec<Vec<Edge>> = (0..=len).map(|_| Vec::new()).collect();
    for (position, vertex) in edges[..len].iter_mut().enumerate() {
        let rest = &code.compact[position..];
        for (key, alts) in &index.spellings {
            if key.is_empty() || !rest.starts_with(key) {
                continue;
            }
            let end = position + key.len();
            if !code.unbroken(position, end) {
                continue;
            }
            for &(syllable, kind) in alts {
                vertex.push(Edge {
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
        vertex.sort_by_key(|edge| (edge.syllable, edge.end));
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
///
/// 补全边自 `farthest` 直连 `len`：`farthest..len` 内有分隔符时直接放弃补全（返回 `false`
/// ⇒ 整段无候选），否则补出的音节会跨过分隔符。
fn complete(
    index: &SoundToCharShapeIndex,
    edges: &mut [Vec<Edge>],
    code: &DelimitedCode,
    farthest: usize,
) -> bool {
    let len = code.len();
    if farthest < len && !code.unbroken(farthest, len) {
        return false;
    }
    let tail = &code.compact[farthest..];
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

/// 广度优先收集「码恰好等于路径音节序列」的词条块（参照 `Table::Query` 的推入序），并生成
/// 「按音节分码」的预编辑：上一段为全拼时在下一音节前插空格（`` `zhongguo `` → `` `zhong guo ``），
/// 上一段是缩写/补全则与后续音节合并（`` `zhguo `` → `` `zhguo ``），分隔符处原样保留撇号
/// （含段首与段尾，`` `zh'guo `` → `` `zh'guo ``、`` `zh' `` → `` `zh' ``）——与输入同形。
fn collect_chunks(
    index: &SoundToCharShapeIndex,
    edges: &[Vec<Edge>],
    code: &DelimitedCode,
) -> Vec<Chunk> {
    let len = code.len();
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
            // 分隔符原样保留（含段首），只有音节边界插空格。
            if code.delimiter_before(position) {
                next_preedit.push(SYLLABLE_DELIMITER as char);
            } else if !next_preedit.is_empty() && last_kind == KIND_NORMAL {
                next_preedit.push(' ');
            }
            next_preedit.push_str(&String::from_utf8_lossy(&code.compact[position..edge.end]));
            if edge.end == len {
                // 到达段尾：段尾分隔符同样保留（此路径不再扩展，可直接改预编辑）。
                if code.delimiter_before(len) {
                    next_preedit.push(SYLLABLE_DELIMITER as char);
                }
                if let Some(group) = index.group(&next_path) {
                    chunks.push(Chunk {
                        first: group.first,
                        count: group.count,
                        penalty: next_penalty,
                        preedit: next_preedit,
                    });
                }
            } else if index.prefix_exists(&next_path) {
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
