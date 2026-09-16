//! 码表 / 字频 / 白名单 / 补充短语的数据层，对应参照实现
//! `lua/tiger_sentence.lua` 的 parse_* / build_lexicon_index / rebuild_lexicon /
//! supplement 部分。
//!
//! 语义要点（与参照一致）：
//! - `codes.txt` 行序即 rank；同一 `(word, code)` 去重保首见；
//! - `char_ranks.txt` 每行首字符按行序获得稠密 rank；
//! - 高频限制只过滤“常用字的非最优码”，白名单与多字词不受限；
//! - `-` 字节序：纯 UTF-8，按字符切分；Lua `%s` 语义 = ASCII 空白。

use hashbrown::{HashMap, HashSet};
use std::path::{Path, PathBuf};

pub const DEFAULT_HIGH_FREQ_LIMIT: usize = 1500;
pub const UNKNOWN_CHARACTER_RANK_FALLBACK: usize = 20001;

const CODES_FILE: &str = "tiger_sentence.codes.txt";
const RANKS_FILE: &str = "tiger_sentence.char_ranks.txt";
const WHITELIST_FILE: &str = "tiger_sentence.full_code_whitelist.txt";
pub const SUPPLEMENT_FILE: &str = "tiger_sentence.supplement.txt";

// ---------------------------------------------------------------- 文本工具

/// Lua 模式类 `%s` 的 ASCII 空白集合（含垂直制表符，Rust `trim()` 不含）。
fn is_lua_space(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\n' | '\u{0b}' | '\u{0c}' | '\r')
}

fn is_lua_space_byte(byte: u8) -> bool {
    matches!(byte, b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r')
}

fn trim_lua_whitespace(text: &str) -> &str {
    text.trim_matches(is_lua_space)
}

/// 参照 `normalize_text_content`：去 BOM，CRLF/CR → LF。
fn normalize_text_content(content: &str) -> String {
    let body = content.strip_prefix('\u{feff}').unwrap_or(content);
    if body.contains('\r') {
        body.replace("\r\n", "\n").replace('\r', "\n")
    } else {
        body.to_string()
    }
}

/// 参照 `each_content_line`：跳过空行与 `#` 注释，两侧去 ASCII 空白。
fn each_content_line(content: &str, mut callback: impl FnMut(&str)) {
    for raw_line in content.split('\n') {
        if raw_line.is_empty() {
            continue; // gmatch("[^\n]+") 不产出空串
        }
        let line = trim_lua_whitespace(raw_line);
        if !line.is_empty() && !line.starts_with('#') {
            callback(line);
        }
    }
}

/// 参照 `^(%S+)%s+(%S+)`：前两个 ASCII 空白分隔的 token。
fn first_two_tokens(line: &str) -> Option<(&str, &str)> {
    let bytes = line.as_bytes();
    let mut index = 0;
    while index < bytes.len() && !is_lua_space_byte(bytes[index]) {
        index += 1;
    }
    if index == 0 || index >= bytes.len() {
        return None;
    }
    let word = &line[..index];
    while index < bytes.len() && is_lua_space_byte(bytes[index]) {
        index += 1;
    }
    if index >= bytes.len() {
        return None;
    }
    let start = index;
    while index < bytes.len() && !is_lua_space_byte(bytes[index]) {
        index += 1;
    }
    Some((word, &line[start..index]))
}

fn is_single_character(text: &str) -> bool {
    text.chars().count() == 1
}

// ---------------------------------------------------------------- 数据结构

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CodeEntry {
    pub text: String,
    pub rank: usize,
    pub optimal_single: bool,
}

/// `data_status()` 的稳定字段（路径不入样）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DataStatus {
    pub built: bool,
    pub high_freq_limit: usize,
    pub codes_entries: usize,
    pub codes_count: usize,
    pub ranks_count: usize,
    pub whitelist_count: usize,
    pub isolation_enabled: bool,
    pub error_count: usize,
}

impl DataStatus {
    /// 差分金样使用的规范文本。
    pub fn canonical(&self) -> String {
        format!(
            "built={} high_freq_limit={} codes_entries={} codes_count={} ranks_count={} \
             whitelist_count={} isolation_enabled={} errors={}",
            self.built as u8,
            self.high_freq_limit,
            self.codes_entries,
            self.codes_count,
            self.ranks_count,
            self.whitelist_count,
            self.isolation_enabled as u8,
            self.error_count,
        )
    }
}

pub struct Lexicon {
    dirs: Vec<PathBuf>,
    pub built: bool,
    pub high_freq_limit: usize,
    pub codes: HashMap<String, Vec<CodeEntry>>,
    /// 每个单字出现的全部编码（源序）。
    pub character_codes: HashMap<String, Vec<String>>,
    pub lengths: Vec<usize>,
    pub max_code_len: usize,
    pub proper_code_prefixes: HashSet<String>,
    pub character_ranks: Option<HashMap<String, usize>>,
    pub ranks_count: usize,
    pub unknown_character_rank: usize,
    pub isolation_enabled: bool,
    pub codes_path: Option<String>,
    pub codes_entries: usize,
    pub codes_count: usize,
    pub ranks_path: Option<String>,
    pub whitelist_path: Option<String>,
    pub whitelist_count: usize,
    pub errors: Vec<String>,
}

impl Lexicon {
    /// 按目录顺序（用户目录 → 共享目录）装载并构建索引。
    pub fn load(dirs: &[PathBuf], limit: usize) -> Self {
        let mut lexicon = Self {
            dirs: dirs.to_vec(),
            built: false,
            high_freq_limit: limit,
            codes: HashMap::new(),
            character_codes: HashMap::new(),
            lengths: Vec::new(),
            max_code_len: 1,
            proper_code_prefixes: HashSet::new(),
            character_ranks: None,
            ranks_count: 0,
            unknown_character_rank: UNKNOWN_CHARACTER_RANK_FALLBACK,
            isolation_enabled: false,
            codes_path: None,
            codes_entries: 0,
            codes_count: 0,
            ranks_path: None,
            whitelist_path: None,
            whitelist_count: 0,
            errors: Vec::new(),
        };
        lexicon.rebuild(limit);
        lexicon
    }

    /// 参照 `M.apply_high_freq_limit` 的重建路径；负数由调用方归一为 0
    /// （Rust 侧 API 为 `usize`，与参照的 `value < 0 → 0` 等价）。
    pub fn apply_high_freq_limit(&mut self, limit: usize) {
        self.rebuild(limit);
    }

    pub fn rebuild(&mut self, limit: usize) {
        let mut errors = Vec::new();

        let codes_file = self.read_data_file(CODES_FILE);
        let (entries, codes_path) = match &codes_file {
            Some((content, path)) => (parse_codes_content(content), Some(path.clone())),
            None => {
                errors.push(format!("missing {CODES_FILE}"));
                (Vec::new(), None)
            }
        };

        let ranks_file = self.read_data_file(RANKS_FILE);
        let (character_ranks, ranks_count, ranks_path) = match &ranks_file {
            Some((content, path)) => {
                let (ranks, count) = parse_ranks_content(content);
                if count == 0 {
                    (None, 0, Some(path.clone()))
                } else {
                    (Some(ranks), count, Some(path.clone()))
                }
            }
            None => (None, 0, None),
        };

        let whitelist_file = self.read_data_file(WHITELIST_FILE);
        let (whitelist, whitelist_path) = match &whitelist_file {
            Some((content, path)) => (parse_whitelist_content(content), Some(path.clone())),
            None => (HashSet::new(), None),
        };
        let whitelist_count = whitelist.len();

        let index = build_lexicon_index(&entries, character_ranks.as_ref(), limit, &whitelist);

        self.built = true;
        self.high_freq_limit = limit;
        self.codes = index.codes;
        self.character_codes = index.character_codes;
        self.lengths = index.lengths;
        self.max_code_len = index.max_code_len;
        self.proper_code_prefixes = index.proper_code_prefixes;
        self.ranks_count = ranks_count;
        self.unknown_character_rank = if ranks_count > 0 {
            ranks_count + 1
        } else {
            UNKNOWN_CHARACTER_RANK_FALLBACK
        };
        self.isolation_enabled = character_ranks.is_some();
        self.character_ranks = character_ranks;
        self.codes_path = codes_path;
        self.codes_entries = entries.len();
        self.codes_count = self.codes.len();
        self.ranks_path = ranks_path;
        self.whitelist_path = whitelist_path;
        self.whitelist_count = whitelist_count;
        self.errors = errors;
    }

    /// 数据文件按 UTF-8 文本读取；不可读（含非法 UTF-8）视为缺失。
    /// 参照实现对字节流宽松，本项目数据文件均为 UTF-8。
    fn read_data_file(&self, name: &str) -> Option<(String, String)> {
        for directory in &self.dirs {
            let path = directory.join(name);
            if let Ok(content) = std::fs::read_to_string(&path) {
                return Some((content, path.to_string_lossy().into_owned()));
            }
        }
        None
    }

    pub fn data_status(&self) -> DataStatus {
        DataStatus {
            built: self.built,
            high_freq_limit: self.high_freq_limit,
            codes_entries: self.codes_entries,
            codes_count: self.codes_count,
            ranks_count: self.ranks_count,
            whitelist_count: self.whitelist_count,
            isolation_enabled: self.isolation_enabled,
            error_count: self.errors.len(),
        }
    }

    pub fn probe(&self, code: &str) -> Option<&[CodeEntry]> {
        self.codes.get(code).map(|entries| entries.as_slice())
    }

    pub fn lengths(&self) -> &[usize] {
        &self.lengths
    }
}

// ---------------------------------------------------------------- 解析

/// 参照 `parse_codes_content`：`word code`，去重 `(word, code)`，保源序。
pub fn parse_codes_content(content: &str) -> Vec<(String, String)> {
    let mut entries = Vec::new();
    let mut seen = HashSet::new();
    each_content_line(&normalize_text_content(content), |line| {
        let Some((word, code)) = first_two_tokens(line) else {
            return;
        };
        let code = code.to_ascii_lowercase();
        if code.is_empty() || !code.bytes().all(|byte| byte.is_ascii_lowercase()) {
            return;
        }
        if seen.insert((word.to_string(), code.clone())) {
            entries.push((word.to_string(), code));
        }
    });
    entries
}

/// 参照 `parse_ranks_content`：每行首字符按行序获得稠密 rank。
pub fn parse_ranks_content(content: &str) -> (HashMap<String, usize>, usize) {
    let mut ranks = HashMap::new();
    let mut count = 0usize;
    each_content_line(&normalize_text_content(content), |line| {
        let Some(character) = line.chars().next() else {
            return;
        };
        let key = character.to_string();
        if !ranks.contains_key(&key) {
            count += 1;
            ranks.insert(key, count);
        }
    });
    (ranks, count)
}

/// 参照 `parse_whitelist_content`：行内每个字符入白名单。
pub fn parse_whitelist_content(content: &str) -> HashSet<String> {
    let mut characters = HashSet::new();
    each_content_line(&normalize_text_content(content), |line| {
        for character in line.chars() {
            characters.insert(character.to_string());
        }
    });
    characters
}

struct LexiconIndex {
    codes: HashMap<String, Vec<CodeEntry>>,
    character_codes: HashMap<String, Vec<String>>,
    lengths: Vec<usize>,
    max_code_len: usize,
    proper_code_prefixes: HashSet<String>,
}

/// 参照 `build_lexicon_index`：精确码表 + 主码/最优码 + 高频过滤 + 前缀集。
fn build_lexicon_index(
    entries: &[(String, String)],
    character_ranks: Option<&HashMap<String, usize>>,
    high_freq_limit: usize,
    whitelist: &HashSet<String>,
) -> LexiconIndex {
    let mut exact: HashMap<String, Vec<String>> = HashMap::new();
    let mut codes_by_character: HashMap<String, Vec<String>> = HashMap::new();
    for (word, code) in entries {
        exact.entry(code.clone()).or_default().push(word.clone());
        if is_single_character(word) {
            codes_by_character
                .entry(word.clone())
                .or_default()
                .push(code.clone());
        }
    }

    let common: Option<HashSet<&str>> = match character_ranks {
        Some(ranks) if high_freq_limit > 0 => Some(
            ranks
                .iter()
                .filter(|(_, rank)| **rank <= high_freq_limit)
                .map(|(character, _)| character.as_str())
                .collect(),
        ),
        _ => None,
    };

    // 主码：优先“短且以该字为首候选”的码，否则最短码；并列取先见。
    let mut primary: HashMap<String, String> = HashMap::new();
    for (character, codes) in &codes_by_character {
        let (mut best_first, mut best_any): (Option<&str>, Option<&str>) = (None, None);
        for code in codes {
            if code.len() < 2 {
                continue;
            }
            if best_any.is_none_or(|current| code.len() < current.len()) {
                best_any = Some(code);
            }
            if let Some(texts) = exact.get(code)
                && texts.first().map(String::as_str) == Some(character.as_str())
                && best_first.is_none_or(|current| code.len() < current.len())
            {
                best_first = Some(code);
            }
        }
        if let Some(chosen) = best_first.or(best_any) {
            primary.insert(character.clone(), chosen.to_string());
        }
    }

    let mut optimal_input: HashMap<String, String> = HashMap::new();
    for (character, codes) in &codes_by_character {
        let mut chosen: Option<&str> = None;
        for code in codes {
            if chosen.is_none_or(|current| code.len() < current.len()) {
                chosen = Some(code);
            }
        }
        if let Some(chosen) = chosen {
            optimal_input.insert(character.clone(), chosen.to_string());
        }
    }

    let mut codes: HashMap<String, Vec<CodeEntry>> = HashMap::new();
    let mut length_values: HashSet<usize> = HashSet::new();
    let mut max_len = 1usize;
    let mut prefixes: HashSet<String> = HashSet::new();
    for (code, texts) in &exact {
        let mut allowed = Vec::new();
        for (position, text) in texts.iter().enumerate() {
            let allow_non_primary = code.len() == 1
                || !is_single_character(text)
                || common
                    .as_ref()
                    .is_none_or(|set| !set.contains(text.as_str()))
                || whitelist.contains(text);
            if allow_non_primary
                || primary.get(text.as_str()).map(String::as_str) == Some(code.as_str())
            {
                allowed.push(CodeEntry {
                    text: text.clone(),
                    rank: position + 1,
                    optimal_single: optimal_input.get(text.as_str()).map(String::as_str)
                        == Some(code.as_str()),
                });
            }
        }
        if !allowed.is_empty() {
            length_values.insert(code.len());
            if code.len() > max_len {
                max_len = code.len();
            }
            for length in 1..code.len() {
                prefixes.insert(code[..length].to_string());
            }
            codes.insert(code.clone(), allowed);
        }
    }

    let mut lengths: Vec<usize> = length_values.into_iter().collect();
    lengths.sort_unstable();

    LexiconIndex {
        codes,
        character_codes: codes_by_character,
        lengths,
        max_code_len: max_len,
        proper_code_prefixes: prefixes,
    }
}

// ---------------------------------------------------------------- 补充短语

pub const SUPPLEMENT_BASELINE_REWARD: f64 = 9.0;
pub const SUPPLEMENT_WEIGHT_SCALE: f64 = 2.0;
pub const SUPPLEMENT_BASELINE_WEIGHT: f64 = 1000.0;
pub const SUPPLEMENT_MAXIMUM_REWARD: f64 = 16.0;

/// 参照 `reward_for_weight`：重量 → 补充奖励。
pub fn reward_for_weight(weight: f64) -> f64 {
    let bounded = weight.clamp(1.0, 1_000_000_000.0);
    let reward = SUPPLEMENT_BASELINE_REWARD
        + SUPPLEMENT_WEIGHT_SCALE * (bounded / SUPPLEMENT_BASELINE_WEIGHT).ln();
    reward.clamp(0.0, SUPPLEMENT_MAXIMUM_REWARD)
}

struct SupplementNode {
    transitions: HashMap<char, usize>,
    failure: usize,
    reward: f64,
}

/// 补充短语的 Aho–Corasick 匹配器（`supplement` 表）。
///
/// 节点编号受构建顺序影响，但 `advance` 的奖励结果与顺序无关；
/// 差分验证以奖励序列为准（随 K1 decode 金样覆盖）。
pub struct Supplement {
    nodes: Vec<SupplementNode>,
    pub path: Option<String>,
    pub count: usize,
    pub error: Option<String>,
}

impl Supplement {
    fn empty(path: Option<String>, error: Option<String>) -> Self {
        Self {
            nodes: vec![SupplementNode {
                transitions: HashMap::new(),
                failure: 0,
                reward: 0.0,
            }],
            path,
            count: 0,
            error,
        }
    }

    /// 参照 `supplement.load_default`：文件缺失是正常状态（count=0）。
    pub fn load_default(user_dir: Option<&Path>) -> Self {
        let Some(directory) = user_dir else {
            return Self::empty(None, None);
        };
        let path = directory.join(SUPPLEMENT_FILE);
        Self::load_file(&path)
    }

    pub fn load_file(path: &Path) -> Self {
        let display = path.to_string_lossy().into_owned();
        let content = match std::fs::read_to_string(path) {
            Ok(content) => content,
            Err(error) => return Self::empty(Some(display), Some(error.to_string())),
        };
        Self::build(&parse_supplement_content(&content), Some(display))
    }

    pub fn build(entries: &HashMap<String, f64>, path: Option<String>) -> Self {
        let mut nodes = vec![SupplementNode {
            transitions: HashMap::new(),
            failure: 0,
            reward: 0.0,
        }];
        let mut count = 0usize;
        for (text, weight) in entries {
            let reward = reward_for_weight(*weight);
            if text.is_empty() || reward <= 0.0 {
                continue;
            }
            let mut state = 0usize;
            for character in text.chars() {
                let next = nodes[state].transitions.get(&character).copied();
                let next = match next {
                    Some(next) => next,
                    None => {
                        let index = nodes.len();
                        nodes.push(SupplementNode {
                            transitions: HashMap::new(),
                            failure: 0,
                            reward: 0.0,
                        });
                        nodes[state].transitions.insert(character, index);
                        index
                    }
                };
                state = next;
            }
            nodes[state].reward = nodes[state].reward.max(reward);
            count += 1;
        }

        if count == 0 {
            return Self::empty(path, None);
        }

        // AC 失配链与奖励上推（BFS）。
        let mut queue: Vec<usize> = Vec::new();
        let root_children: Vec<usize> = nodes[0].transitions.values().copied().collect();
        for &child in &root_children {
            nodes[child].failure = 0;
            queue.push(child);
        }
        let mut head = 0usize;
        while head < queue.len() {
            let current = queue[head];
            head += 1;
            let children: Vec<(char, usize)> = nodes[current]
                .transitions
                .iter()
                .map(|(character, index)| (*character, *index))
                .collect();
            for (character, child) in children {
                let mut fallback = nodes[current].failure;
                while fallback != 0 && !nodes[fallback].transitions.contains_key(&character) {
                    fallback = nodes[fallback].failure;
                }
                let failure_target = nodes[fallback].transitions.get(&character).copied();
                if let Some(target) = failure_target
                    && target != child
                {
                    nodes[child].failure = target;
                } else {
                    nodes[child].failure = 0;
                }
                let failure = nodes[child].failure;
                nodes[child].reward = nodes[child].reward.max(nodes[failure].reward);
                queue.push(child);
            }
        }

        Self {
            nodes,
            path,
            count,
            error: None,
        }
    }

    /// 参照 `supplement.advance`：返回 (状态, 奖励)；状态为 1 基（根 = 1）。
    pub fn advance(&self, state: usize, character: char) -> (usize, f64) {
        if self.count == 0 {
            return (1, 0.0);
        }
        let mut current = if state >= 1 && state <= self.nodes.len() {
            state
        } else {
            1
        };
        while current != 1 && !self.nodes[current - 1].transitions.contains_key(&character) {
            current = self.nodes[current - 1].failure + 1;
        }
        current = self.nodes[current - 1]
            .transitions
            .get(&character)
            .map(|index| index + 1)
            .unwrap_or(1);
        (current, self.nodes[current - 1].reward)
    }

    pub fn status(&self) -> SupplementStatus {
        SupplementStatus {
            count: self.count,
            has_error: self.error.is_some(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SupplementStatus {
    pub count: usize,
    pub has_error: bool,
}

impl SupplementStatus {
    pub fn canonical(&self) -> String {
        format!("count={} error={}", self.count, self.has_error as u8)
    }
}

/// 参照 `supplement.load_file` 的行解析：`text [weight]`，非法权重丢弃该行。
pub fn parse_supplement_content(content: &str) -> HashMap<String, f64> {
    let mut entries = HashMap::new();
    let body = content.strip_prefix('\u{feff}').unwrap_or(content);
    // `(content .. "\n"):gmatch("(.-)\r?\n")`：按行切分，含空行。
    for raw_line in body.split('\n') {
        let raw_line = raw_line.strip_suffix('\r').unwrap_or(raw_line);
        let line = trim_lua_whitespace(raw_line);
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (text, rest) = match first_two_tokens_relaxed(line) {
            Some((text, rest)) => (text, rest),
            None => (line, ""),
        };
        let weight = if rest.is_empty() {
            1000.0
        } else if !rest.bytes().all(|byte| byte.is_ascii_digit()) {
            continue;
        } else {
            match rest.parse::<f64>() {
                Ok(weight) if weight > 0.0 => weight,
                _ => continue,
            }
        };
        entries.insert(text.to_string(), weight);
    }
    entries
}

/// 参照 `^(%S+)%s*(.-)$`：首 token 与其后剩余（去前导空白）。
fn first_two_tokens_relaxed(line: &str) -> Option<(&str, &str)> {
    let bytes = line.as_bytes();
    let mut index = 0;
    while index < bytes.len() && !is_lua_space_byte(bytes[index]) {
        index += 1;
    }
    if index == 0 {
        return None;
    }
    let text = &line[..index];
    while index < bytes.len() && is_lua_space_byte(bytes[index]) {
        index += 1;
    }
    Some((text, &line[index..]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_codes_with_dedup_and_lowercasing() {
        let entries = parse_codes_content("来\tA\n那个\tab\n来\ta\n坏\tA1\n缺码\t\n");
        assert_eq!(
            entries,
            vec![
                ("来".to_string(), "a".to_string()),
                ("那个".to_string(), "ab".to_string()),
            ]
        );
    }

    #[test]
    fn reward_matches_reference_curve() {
        assert_eq!(reward_for_weight(1000.0), 9.0);
        assert!((reward_for_weight(4000.0) - (9.0 + 2.0 * 4.0f64.ln())).abs() < 1e-12);
        assert_eq!(reward_for_weight(1e12), 16.0);
    }

    #[test]
    fn supplement_advance_walks_trie() {
        let mut entries = HashMap::new();
        entries.insert("甲乙".to_string(), 4000.0);
        let matcher = Supplement::build(&entries, None);
        assert_eq!(matcher.count, 1);
        let (state, reward) = matcher.advance(1, '甲');
        assert_eq!(reward, 0.0);
        let (state, reward) = matcher.advance(state, '乙');
        assert!((reward - reward_for_weight(4000.0)).abs() < 1e-12);
        let _ = state;
    }
}
