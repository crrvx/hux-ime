// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! 词库索引的构建：精确码表、主码/最优码、高频过滤与合法前缀集。

use hashbrown::{HashMap, HashSet};
use hux_core::collections::{Map, Set};

use super::CodeEntry;
use super::files::is_single_character;

pub(super) struct LexiconIndex {
    pub(super) codes: Map<String, Vec<CodeEntry>>,
    pub(super) character_codes: Map<String, Vec<String>>,
    pub(super) lengths: Vec<usize>,
    pub(super) max_code_len: usize,
    pub(super) proper_code_prefixes: Set<String>,
}

/// 参照 `build_lexicon_index`：精确码表 + 主码/最优码 + 高频过滤 + 前缀集。
///
/// 中间容器（`exact` / `primary` / `optimal_input`）只在本函数内用，故键值一律借 `entries`
/// 的切片——原实现为每条目克隆一次字与码（~117k + ~76k 次分配），其中绝大多数随即被丢弃。
pub(super) fn build_lexicon_index(
    entries: &[(String, String)],
    character_ranks: Option<&Map<String, usize>>,
    high_freq_limit: usize,
    whitelist: &Set<String>,
) -> LexiconIndex {
    let (exact, codes_by_character) = collect_exact(entries);
    let common = common_characters(character_ranks, high_freq_limit);
    let primary = primary_codes(&codes_by_character, &exact);
    let optimal_input = optimal_inputs(&codes_by_character);
    let (codes, lengths, max_len, prefixes) =
        build_codes(&exact, common.as_ref(), whitelist, &primary, &optimal_input);

    // `primary` / `optimal_input` 借 `codes_by_character`（下面要移入返回值），先释放。
    drop(primary);
    drop(optimal_input);

    LexiconIndex {
        codes,
        character_codes: codes_by_character,
        lengths,
        max_code_len: max_len,
        proper_code_prefixes: prefixes,
    }
}

/// 码 → 文本（保行序）；键值借 `entries` 的切片。
type ExactCodes<'a> = HashMap<&'a str, Vec<&'a str>>;

/// 字 → 码组（单字读音倒排）。
type CodesByCharacter = Map<String, Vec<String>>;

/// 精确码表（码 → 文本，保行序）与单字读音倒排（字 → 码组）；键值借 `entries` 的切片。
fn collect_exact(entries: &[(String, String)]) -> (ExactCodes<'_>, CodesByCharacter) {
    let mut exact: ExactCodes<'_> = HashMap::with_capacity(entries.len());
    let mut codes_by_character: CodesByCharacter = Map::with_capacity(entries.len());
    for (word, code) in entries {
        exact.entry(code.as_str()).or_default().push(word.as_str());
        if is_single_character(word) {
            codes_by_character
                .entry_or_default(word.clone())
                .push(code.clone());
        }
    }
    (exact, codes_by_character)
}

/// 高频字集：字频表里 rank ≤ `high_freq_limit` 的字；无字频或上限为 0 即不限。
fn common_characters(
    character_ranks: Option<&Map<String, usize>>,
    high_freq_limit: usize,
) -> Option<HashSet<&str>> {
    match character_ranks {
        Some(ranks) if high_freq_limit > 0 => Some(
            ranks
                .iter()
                .filter(|(_, rank)| **rank <= high_freq_limit)
                .map(|(character, _)| character.as_str())
                .collect(),
        ),
        _ => None,
    }
}

/// 主码：优先“短且以该字为首候选”的码，否则最短码；并列取先见。
fn primary_codes<'a>(
    codes_by_character: &'a Map<String, Vec<String>>,
    exact: &ExactCodes<'_>,
) -> HashMap<&'a str, &'a str> {
    let mut primary: HashMap<&str, &str> = HashMap::with_capacity(codes_by_character.len());
    for (character, codes) in codes_by_character.iter() {
        let (mut best_first, mut best_any): (Option<&str>, Option<&str>) = (None, None);
        for code in codes {
            if code.len() < 2 {
                continue;
            }
            if best_any.is_none_or(|current| code.len() < current.len()) {
                best_any = Some(code);
            }
            if let Some(texts) = exact.get(code.as_str())
                && texts.first().copied() == Some(character.as_str())
                && best_first.is_none_or(|current| code.len() < current.len())
            {
                best_first = Some(code);
            }
        }
        if let Some(chosen) = best_first.or(best_any) {
            primary.insert(character.as_str(), chosen);
        }
    }
    primary
}

/// 最优码：该字的最短码；并列取先见。
fn optimal_inputs(codes_by_character: &Map<String, Vec<String>>) -> HashMap<&str, &str> {
    let mut optimal_input: HashMap<&str, &str> = HashMap::with_capacity(codes_by_character.len());
    for (character, codes) in codes_by_character.iter() {
        let mut chosen: Option<&str> = None;
        for code in codes {
            if chosen.is_none_or(|current| code.len() < current.len()) {
                chosen = Some(code);
            }
        }
        if let Some(chosen) = chosen {
            optimal_input.insert(character.as_str(), chosen);
        }
    }
    optimal_input
}

/// 逐码分配候选条目（含高频限制与白名单口径），并收集长度集合与合法前缀集。
fn build_codes(
    exact: &ExactCodes<'_>,
    common: Option<&HashSet<&str>>,
    whitelist: &Set<String>,
    primary: &HashMap<&str, &str>,
    optimal_input: &HashMap<&str, &str>,
) -> (Map<String, Vec<CodeEntry>>, Vec<usize>, usize, Set<String>) {
    let mut codes: Map<String, Vec<CodeEntry>> = Map::with_capacity(exact.len());
    let mut length_values: HashSet<usize> = HashSet::new();
    let mut max_len = 1usize;
    let mut prefixes: Set<String> = Set::new();
    for (&code, texts) in exact {
        let mut allowed = Vec::new();
        for (position, &text) in texts.iter().enumerate() {
            let allow_non_primary = code.len() == 1
                || !is_single_character(text)
                || common.is_none_or(|set| !set.contains(text))
                || whitelist.contains(text);
            if allow_non_primary || primary.get(text).copied() == Some(code) {
                allowed.push(CodeEntry {
                    text: text.to_string(),
                    rank: position + 1,
                    optimal_single: optimal_input.get(text).copied() == Some(code),
                    primary_single: primary.get(text).copied() == Some(code),
                });
            }
        }
        if !allowed.is_empty() {
            length_values.insert(code.len());
            if code.len() > max_len {
                max_len = code.len();
            }
            for length in 1..code.len() {
                // 先查重再分配：前缀集只有 ~16k 个不同值，而 (码, 长度) 组合有 ~110k 个。
                let prefix = &code[..length];
                if !prefixes.contains(prefix) {
                    prefixes.insert(prefix.to_string());
                }
            }
            codes.insert(code.to_string(), allowed);
        }
    }

    let mut lengths: Vec<usize> = length_values.into_iter().collect();
    lengths.sort_unstable();
    (codes, lengths, max_len, prefixes)
}
