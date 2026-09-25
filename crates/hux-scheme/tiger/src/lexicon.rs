// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! 码表 / 字频 / 白名单 / 补充短语的数据层，对应参照实现
//! `lua/tiger_sentence.lua` 的 parse_* / build_lexicon_index / rebuild_lexicon /
//! supplement 部分。
//!
//! 语义要点（与参照一致）：
//! - `codes.txt` 行序即 rank；同一 `(word, code)` 去重保首见；
//! - 追加码表 `tiger_sentence.codes.<name>.txt`（与主表**同目录**）按文件名字典序拼在主表之后：
//!   主表内所有 rank 逐位不变，追加表只能在既有码上垫后或引入新码（编排见 `data/README.md`）；
//! - `char_ranks.txt` 每行首字符按行序获得稠密 rank；
//! - 高频限制只过滤“常用字的非最优码”，白名单与多字词不受限；
//! - `-` 字节序：纯 UTF-8，按字符切分；Lua `%s` 语义 = ASCII 空白。
//!
//! 字集开关（[`LexiconOptions`]）同样只改**词库内容**：关掉全字集即不装载追加码表，
//! 关掉过滤即保留追加表里的非汉字行（主表行不受过滤影响）。
//!
//! 装载口径（[`Lexicon::load_with`]；本机 release 实测随包数据 ~117k 行：逐表解析 ~15 ms、
//! 建索引 ~135 ms、满载 ~150 ms、常驻 ~170 MB）：各表**逐表解析**后合并（不拼成一个
//! ~1.1 MB 的大串再解析），索引的中间容器借条目切片作键——只有返回结构真正持有的字符串
//! 才分配。

use hashbrown::{HashMap, HashSet};
use hux_core::collections::{Map, Set};
use std::borrow::Cow;
use std::path::{Path, PathBuf};

/// 未知字符的字频回退（参照 `unknown_character_rank`）。
pub const UNKNOWN_CHARACTER_RANK_FALLBACK: usize = 20001;

const CODES_FILE: &str = "tiger_sentence.codes.txt";
/// 追加码表的前后缀：`tiger_sentence.codes.<name>.txt`（`<name>` 至少一个字符）。
/// 主表与全部追加表按确定顺序拼接后一起解析（见 [`Lexicon::read_code_tables`]）。
const CODES_EXTRA_PREFIX: &str = "tiger_sentence.codes.";
const CODES_EXTRA_SUFFIX: &str = ".txt";
const RANKS_FILE: &str = "tiger_sentence.char_ranks.txt";
const WHITELIST_FILE: &str = "tiger_sentence.full_code_whitelist.txt";
pub const SUPPLEMENT_FILE: &str = "tiger_sentence.supplement.txt";

/// 词先验位图文件名（参考 `tiger_sentence.lexical.bin`）。
pub const LEXICAL_FILE: &str = "tiger_sentence.lexical.bin";
/// 语言模型相对路径（参考 `models/sentence-ngram-mobile.bin`）。
pub const MODEL_PATH: &str = "models/sentence-ngram-mobile.bin";

/// 字集开关（[`Lexicon::load_with`] / [`Lexicon::apply_lexicon_options`] 的入参）。
///
/// 两者都只改**词库内容**（[`DataStatus`] 的字段不随之增减），故变更即重建索引。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LexiconOptions {
    /// 启用全字集：装载同目录的追加码表（`tiger_sentence.codes.<name>.txt`）；
    /// 关掉时**连读盘都不做**，只装主表。
    pub extra_code_tables: bool,
    /// 过滤非汉字：追加码表里「单字符且不属于 CJK 统一表意文字」的行不入词库。
    ///
    /// **只作用于追加码表**：随包主表里有 3 个非汉字（兼容汉字等），差分金样夹具
    /// （`goldens/lexicon` = 上游主表）按主表逐位比对，主表行必须原样保留。
    pub filter_non_han: bool,
}

impl Default for LexiconOptions {
    /// 出厂口径：两个开关都开（全字集 + 过滤追加表里的非汉字）。
    fn default() -> Self {
        Self {
            extra_code_tables: true,
            filter_non_han: true,
        }
    }
}

/// 依次在各数据目录下探测相对路径（是否存在交由调用方的加载器处理）。
///
/// 目录本身由平台层解析（桌面/Android 各自构造），内核不读取环境变量。
#[cfg(test)]
pub fn candidate_paths(dirs: &[PathBuf], relative: &str) -> Vec<PathBuf> {
    dirs.iter().map(|dir| dir.join(relative)).collect()
}

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
/// 无需改动时借用原串（逐表装载时不再为每张表复制一份内容）。
fn normalize_text_content(content: &str) -> Cow<'_, str> {
    let body = content.strip_prefix('\u{feff}').unwrap_or(content);
    if body.contains('\r') {
        Cow::Owned(body.replace("\r\n", "\n").replace('\r', "\n"))
    } else {
        Cow::Borrowed(body)
    }
}

/// 参照 `each_content_line`：跳过空行与 `#` 注释，两侧去 ASCII 空白。
///
/// 回调收到的行借自 `content`（同一生命周期）：去重键直接借原切片时不必再复制一份。
fn each_content_line<'a>(content: &'a str, mut callback: impl FnMut(&'a str)) {
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

/// CJK 统一表意文字（基本区 + 扩展 A + 扩展 B..F 所在区间）：
/// [`LexiconOptions::filter_non_han`] 的判定口径。
fn is_cjk_unified_ideograph(character: char) -> bool {
    matches!(
        character as u32,
        0x4E00..=0x9FFF | 0x3400..=0x4DBF | 0x20000..=0x3FFFF
    )
}

/// 该行是否属于「追加码表里的非汉字」（过滤判据）：**单字符**且不在 CJK 统一表意文字区。
/// 多字符的词不受过滤（词里含非汉字也照旧保留）。
fn is_filtered_non_han(word: &str) -> bool {
    let mut characters = word.chars();
    match (characters.next(), characters.next()) {
        (Some(character), None) => !is_cjk_unified_ideograph(character),
        _ => false,
    }
}

/// 追加码表文件名：`tiger_sentence.codes.<name>.txt`，`<name>` 至少一个字符
/// （`tiger_sentence.codes.txt` 本身是主表，不算追加表）。
fn is_extra_codes_file(name: &str) -> bool {
    name.starts_with(CODES_EXTRA_PREFIX)
        && name.ends_with(CODES_EXTRA_SUFFIX)
        && name.len() > CODES_EXTRA_PREFIX.len() + CODES_EXTRA_SUFFIX.len()
}

/// 某数据目录里的追加码表文件名（字典序；目录不存在即空）。
fn extra_codes_names(directory: &Path) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(directory) else {
        return Vec::new();
    };
    let mut names: Vec<String> = entries
        .flatten()
        .filter_map(|entry| entry.file_name().into_string().ok())
        .filter(|name| is_extra_codes_file(name))
        .collect();
    names.sort();
    names
}

// ---------------------------------------------------------------- 数据结构

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CodeEntry {
    pub text: String,
    pub rank: usize,
    pub optimal_single: bool,
    /// 该字的最强合法拼写（rank-1 优先，其次最短）；排序先验的 P(code|character) 证据。
    pub primary_single: bool,
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
    /// 本次装载生效的字集开关（见 [`Lexicon::options`]）。
    options: LexiconOptions,
    pub built: bool,
    pub high_freq_limit: usize,
    pub codes: Map<String, Vec<CodeEntry>>,
    /// 每个单字出现的全部编码（源序）。
    pub character_codes: Map<String, Vec<String>>,
    pub lengths: Vec<usize>,
    pub max_code_len: usize,
    pub proper_code_prefixes: Set<String>,
    pub character_ranks: Option<Map<String, usize>>,
    pub ranks_count: usize,
    pub unknown_character_rank: usize,
    pub isolation_enabled: bool,

    pub codes_entries: usize,
    pub codes_count: usize,
    /// 实际装载的码表文件名（**主表在前**，追加表按装载顺序；诊断用，见
    /// [`Lexicon::extra_code_tables`] 与 `hux_engine_data_info` 的摘要）。
    code_tables: Vec<String>,

    pub whitelist_count: usize,
    /// 参照 `lexicon_state.learning_rules`：数据文件内容的 `learning.hash`（NUL 分隔）。
    pub learning_rules: String,
    pub errors: Vec<String>,
}

impl Lexicon {
    /// 按目录顺序（用户目录 → 共享目录）装载并构建索引；字集开关取 [`LexiconOptions::default`]。
    pub fn load(dirs: &[PathBuf], limit: usize) -> Self {
        Self::load_with(dirs, limit, LexiconOptions::default())
    }

    /// 同 [`Lexicon::load`]，但显式给出字集开关。
    pub fn load_with(dirs: &[PathBuf], limit: usize, options: LexiconOptions) -> Self {
        let mut lexicon = Self {
            dirs: dirs.to_vec(),
            options,
            built: false,
            high_freq_limit: limit,
            codes: Map::new(),
            character_codes: Map::new(),
            lengths: Vec::new(),
            max_code_len: 1,
            proper_code_prefixes: Set::new(),
            character_ranks: None,
            ranks_count: 0,
            unknown_character_rank: UNKNOWN_CHARACTER_RANK_FALLBACK,
            isolation_enabled: false,
            codes_entries: 0,
            codes_count: 0,
            code_tables: Vec::new(),
            whitelist_count: 0,
            learning_rules: String::new(),
            errors: Vec::new(),
        };
        lexicon.rebuild(limit);
        lexicon
    }

    /// 本次装载生效的字集开关。
    pub fn options(&self) -> LexiconOptions {
        self.options
    }

    /// 参照 `M.apply_high_freq_limit` 的重建路径；负数由调用方归一为 0
    /// （Rust 侧 API 为 `usize`，与参照的 `value < 0 → 0` 等价）。
    pub fn apply_high_freq_limit(&mut self, limit: usize) {
        self.rebuild(limit);
    }

    /// 应用字集开关与高频上限：两者都改词库内容，故一并落位后**只重建一次**
    /// （只改开关的调用方传当前上限即可；差分重放走单参数的
    /// [`Lexicon::apply_high_freq_limit`]）。
    pub fn apply_lexicon_options(&mut self, limit: usize, options: LexiconOptions) {
        self.options = options;
        self.rebuild(limit);
    }

    pub fn rebuild(&mut self, limit: usize) {
        let mut errors = Vec::new();

        let tables = self.read_code_tables();
        let mut entries: Vec<(String, String)> = Vec::new();
        let mut code_tables: Vec<String> = Vec::new();
        if let Some(tables) = &tables {
            code_tables.push(CODES_FILE.to_string());
            code_tables.extend(tables.extras.iter().map(|table| table.name.clone()));
            // 归一化（去 BOM / CRLF）结果与跨表去重键同为借用，故先全部落在本帧再逐表解析。
            let normalized: Vec<Cow<'_, str>> = std::iter::once(tables.primary.as_str())
                .chain(tables.extras.iter().map(ExtraCodeTable::body))
                .map(normalize_text_content)
                .collect();
            let mut seen: HashSet<(&str, Cow<'_, str>)> = HashSet::new();
            for (order, content) in normalized.iter().enumerate() {
                // 过滤只作用于追加表（`order > 0`）。
                let filter = order > 0 && self.options.filter_non_han;
                append_codes_content(content, filter, &mut seen, &mut entries);
            }
        } else {
            errors.push(format!("missing {CODES_FILE}"));
        }

        let ranks_file = self.read_data_file(RANKS_FILE);
        let (character_ranks, ranks_count) = match &ranks_file {
            Some((content, _)) => {
                let (ranks, count) = parse_ranks_content(content);
                if count == 0 {
                    (None, 0)
                } else {
                    (Some(ranks), count)
                }
            }
            None => (None, 0),
        };

        let whitelist_file = self.read_data_file(WHITELIST_FILE);
        let whitelist = match &whitelist_file {
            Some((content, _)) => parse_whitelist_content(content),
            None => Set::new(),
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
        self.codes_entries = entries.len();
        self.codes_count = self.codes.len();
        self.whitelist_count = whitelist_count;
        self.code_tables = code_tables;
        // 参照 `build_lexicon_index` 末尾：以三份文件内容（缺失视为空串）计算规则指纹。
        // 码表侧取的是**合并后**的内容（主表 + 追加表）：追加表变化即触发重新学习。
        // 逐表分段喂给 `hash_parts`（拼接结果与合并成一个大串逐字节相同），故出厂口径下
        // 指纹与优化前**逐位一致**（升级不重置既有学习库分区）；过滤只丢解析后的条目，
        // 不进指纹。关掉全字集时装载内容即主表，指纹随之只含主表。
        let mut hash_parts: Vec<&str> = tables
            .as_ref()
            .map(CodeTables::fingerprint_parts)
            .unwrap_or_default();
        hash_parts.push("\0");
        hash_parts.push(
            ranks_file
                .as_ref()
                .map(|(content, _)| content.as_str())
                .unwrap_or(""),
        );
        hash_parts.push("\0");
        hash_parts.push(
            whitelist_file
                .as_ref()
                .map(|(content, _)| content.as_str())
                .unwrap_or(""),
        );
        self.learning_rules = hux_core::learning::hash_parts(&hash_parts);
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

    /// 码表：主表 + **同一数据目录**里的全部追加表（`tiger_sentence.codes.<name>.txt`）。
    ///
    /// 主表照旧只取第一个命中的数据目录（用户覆盖共享）；追加表只从该目录取、按文件名字典序
    /// 装载——顺序确定，故 `parse_codes_content` 的行序语义给出：主表内所有 rank 逐位不变，
    /// 追加表只能在既有码上垫后或引入新码。不跨目录收集：别的数据目录（例如只提供词先验的
    /// `data/`）里的码表不混进这份方案数据。主表缺失即返回 `None`。
    ///
    /// 关掉全字集（[`LexiconOptions::extra_code_tables`]）时连读盘都不做，返回的
    /// [`CodeTables::extras`] 为空。
    fn read_code_tables(&self) -> Option<CodeTables> {
        for directory in &self.dirs {
            let Ok(primary) = std::fs::read_to_string(directory.join(CODES_FILE)) else {
                continue;
            };
            let mut extras = Vec::new();
            if self.options.extra_code_tables {
                for name in extra_codes_names(directory) {
                    if let Ok(content) = std::fs::read_to_string(directory.join(&name)) {
                        extras.push(ExtraCodeTable { name, content });
                    }
                }
            }
            return Some(CodeTables { primary, extras });
        }
        None
    }

    /// 实际装载的追加码表文件名（按装载顺序）；关掉全字集时为空。诊断用；不进
    /// [`DataStatus::canonical`]（那是差分金样的比对文本，加字段会动到金样）。
    pub fn extra_code_tables(&self) -> &[String] {
        // 首项是主表（见 [`Lexicon::read_code_tables`]）；主表缺失时整表为空。
        self.code_tables.get(1..).unwrap_or_default()
    }

    /// 数据装载摘要（`hux_engine_data_info`）：装了哪几张码表 + 条目数 / 单字数 +
    /// 两个字集开关的生效值。
    pub fn data_info(&self) -> String {
        format!(
            "code_tables=[{}] entries={} chars={} full_charset={} filter_non_han={}",
            self.code_tables.join(","),
            self.codes_entries,
            self.character_codes.len(),
            u8::from(self.options.extra_code_tables),
            u8::from(self.options.filter_non_han),
        )
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

    /// 数据目录（参照 `lexicon_state.directories` 的用途：定位词先验位图等随包数据）。
    pub fn dirs(&self) -> &[PathBuf] {
        &self.dirs
    }
}

// ---------------------------------------------------------------- 解析

/// 参照 `parse_codes_content`：`word code`，去重 `(word, code)`，保源序。
pub fn parse_codes_content(content: &str) -> Vec<(String, String)> {
    let normalized = normalize_text_content(content);
    let mut entries = Vec::new();
    let mut seen = HashSet::new();
    append_codes_content(&normalized, false, &mut seen, &mut entries);
    entries
}

/// 把一份码表内容解析进条目表：`seen` 跨表共享 ⇒ 逐表解析与「把各表拼成一个大串再解析」
/// 得到**逐位相同**的条目序列（行序不变、去重保首见，追加表里与主表重复的 `(字, 码)`
/// 同样被丢掉）；`filter` 为真时丢掉单字符的非汉字行（只作用于追加码表）。
///
/// 去重键借原切片（`Cow::Borrowed`）：码只在含大写字母时才归一为小写并分配
/// ——逐行分配后再丢弃是原实现里最贵的一环，而随包数据的码全是小写。
fn append_codes_content<'a>(
    normalized: &'a str,
    filter: bool,
    seen: &mut HashSet<(&'a str, Cow<'a, str>)>,
    entries: &mut Vec<(String, String)>,
) {
    // 条目数与行数同量级（每行 ~10 字节）；一次预留，省掉十几次增长复制。
    entries.reserve(normalized.len() / 8);
    each_content_line(normalized, |line| {
        let Some((word, code)) = first_two_tokens(line) else {
            return;
        };
        // 归一为小写**之后**要求全是 ASCII 小写字母（参照 `to_ascii_lowercase` + 逐字节判定）：
        // 等价于原字节全为 ASCII 字母，故先按借用切片筛掉，不为被丢弃的行分配。
        if code.is_empty() || !code.bytes().all(|byte| byte.is_ascii_alphabetic()) {
            return;
        }
        if filter && is_filtered_non_han(word) {
            return;
        }
        let code = if code.bytes().any(|byte| byte.is_ascii_uppercase()) {
            Cow::Owned(code.to_ascii_lowercase())
        } else {
            Cow::Borrowed(code)
        };
        if seen.insert((word, code.clone())) {
            entries.push((word.to_string(), code.into_owned()));
        }
    });
}

/// 参照 `parse_ranks_content`：每行首字符按行序获得稠密 rank。
pub fn parse_ranks_content(content: &str) -> (Map<String, usize>, usize) {
    let mut ranks = Map::new();
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
pub fn parse_whitelist_content(content: &str) -> Set<String> {
    let mut characters = Set::new();
    each_content_line(&normalize_text_content(content), |line| {
        for character in line.chars() {
            characters.insert(character.to_string());
        }
    });
    characters
}

struct LexiconIndex {
    codes: Map<String, Vec<CodeEntry>>,
    character_codes: Map<String, Vec<String>>,
    lengths: Vec<usize>,
    max_code_len: usize,
    proper_code_prefixes: Set<String>,
}

/// 一张追加码表：文件名 + 原始内容（BOM 未剥，剥法见 [`ExtraCodeTable::body`]）。
struct ExtraCodeTable {
    name: String,
    content: String,
}

impl ExtraCodeTable {
    /// 表体：剥掉首 BOM。
    ///
    /// 每张追加表各自剥一次：`normalize_text_content` 只剥得掉**合并内容最前面**那个 BOM，
    /// 否则第二张表起首行的 BOM 会粘进候选文本。指纹口径同此（合并时也是先剥再拼接）。
    fn body(&self) -> &str {
        self.content
            .strip_prefix('\u{feff}')
            .unwrap_or(&self.content)
    }
}

/// 一个数据目录里的码表：主表内容（原样，指纹含其 BOM）+ 追加表（按文件名字典序）。
struct CodeTables {
    primary: String,
    extras: Vec<ExtraCodeTable>,
}

impl CodeTables {
    /// 指纹的码表侧分段：合并串（主表 + 各追加表，`\n` 相隔）的逐段视图。
    fn fingerprint_parts(&self) -> Vec<&str> {
        let mut parts = Vec::with_capacity(2 * self.extras.len() + 1);
        parts.push(self.primary.as_str());
        for table in &self.extras {
            parts.push("\n");
            parts.push(table.body());
        }
        parts
    }
}

/// 参照 `build_lexicon_index`：精确码表 + 主码/最优码 + 高频过滤 + 前缀集。
///
/// 中间容器（`exact` / `primary` / `optimal_input`）只在本函数内用，故键值一律借 `entries`
/// 的切片——原实现为每条目克隆一次字与码（~117k + ~76k 次分配），其中绝大多数随即被丢弃。
fn build_lexicon_index(
    entries: &[(String, String)],
    character_ranks: Option<&Map<String, usize>>,
    high_freq_limit: usize,
    whitelist: &Set<String>,
) -> LexiconIndex {
    let mut exact: HashMap<&str, Vec<&str>> = HashMap::with_capacity(entries.len());
    let mut codes_by_character: Map<String, Vec<String>> = Map::with_capacity(entries.len());
    for (word, code) in entries {
        exact.entry(code.as_str()).or_default().push(word.as_str());
        if is_single_character(word) {
            codes_by_character
                .entry_or_default(word.clone())
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

    let mut codes: Map<String, Vec<CodeEntry>> = Map::with_capacity(exact.len());
    let mut length_values: HashSet<usize> = HashSet::new();
    let mut max_len = 1usize;
    let mut prefixes: Set<String> = Set::new();
    for (&code, texts) in &exact {
        let mut allowed = Vec::new();
        for (position, &text) in texts.iter().enumerate() {
            let allow_non_primary = code.len() == 1
                || !is_single_character(text)
                || common.as_ref().is_none_or(|set| !set.contains(text))
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

// ---------------------------------------------------------------- 补充短语

pub const SUPPLEMENT_BASELINE_REWARD: f64 = 9.0;
pub const SUPPLEMENT_WEIGHT_SCALE: f64 = 2.0;
pub const SUPPLEMENT_BASELINE_WEIGHT: f64 = 1000.0;
pub const SUPPLEMENT_MAXIMUM_REWARD: f64 = 16.0;

/// 参照 `reward_for_weight`：重量 → 补充奖励。
///
/// NaN 口径：Lua 的 `math.max(1, math.min(1e9, nan))` 返回 `1e9`，
/// Rust 的 `clamp` 对 NaN 返回 NaN ⇒ 两端相反。此处**保持 Rust 语义**：正常数据不可达
/// （`parse_supplement_content` 的 `weight > 0.0` 已排除 NaN），唯一可达面是
/// [`Supplement::build`] 的公开入参（畸形输入、无金样支撑），故只注明差异、不改行为。
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
/// 差分验证以奖励序列为准（随 decode 金样覆盖）。
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
        // 与 `Lexicon::read_data_file` 一致：非法 UTF-8 视作空数据并记错误（有意偏离）。
        let content = match std::fs::read_to_string(path) {
            Ok(content) => content,
            Err(error) => return Self::empty(Some(display), Some(error.to_string())),
        };
        Self::build(&parse_supplement_content(&content), Some(display))
    }

    pub fn build(entries: &Map<String, f64>, path: Option<String>) -> Self {
        let mut nodes = vec![SupplementNode {
            transitions: HashMap::new(),
            failure: 0,
            reward: 0.0,
        }];
        let mut count = 0usize;
        for (text, weight) in entries.iter() {
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
pub fn parse_supplement_content(content: &str) -> Map<String, f64> {
    let mut entries = Map::new();
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
    fn candidate_paths_join_each_directory() {
        let dirs = vec![PathBuf::from("/a/hux"), PathBuf::from("/b/hux")];
        let paths = candidate_paths(&dirs, LEXICAL_FILE);
        assert_eq!(paths[0], PathBuf::from("/a/hux").join(LEXICAL_FILE));
        assert_eq!(paths[1], PathBuf::from("/b/hux").join(LEXICAL_FILE));
        assert_eq!(paths.len(), dirs.len());
    }

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
        let mut entries = Map::new();
        entries.insert("甲乙".to_string(), 4000.0);
        let matcher = Supplement::build(&entries, None);
        assert_eq!(matcher.count, 1);
        let (state, reward) = matcher.advance(1, '甲');
        assert_eq!(reward, 0.0);
        let (state, reward) = matcher.advance(state, '乙');
        assert!((reward - reward_for_weight(4000.0)).abs() < 1e-12);
        let _ = state;
    }

    /// 追加码表拼在主表之后：既有码上主表条目仍是 rank 1（简码归主表），重复对去重；
    /// 追加表引入的新码可查。
    #[test]
    fn extra_code_table_appends_after_the_primary_table() {
        let dir = hux_test_support::temp_dir("lexicon-extra-codes");
        std::fs::write(dir.join(CODES_FILE), "来\ta\n").unwrap();
        std::fs::write(
            dir.join("tiger_sentence.codes.huma.txt"),
            "# 追加表：注释与空行照旧忽略\n来\ta\n\n𠀀\tfgf\n",
        )
        .unwrap();
        let lexicon = Lexicon::load(std::slice::from_ref(&dir), 0);
        let entries = lexicon.probe("a").expect("码 a");
        assert_eq!(entries.len(), 1, "与主表重复的 (字, 码) 应被去重保首见");
        assert_eq!(entries[0].text, "来");
        assert_eq!(entries[0].rank, 1);
        assert_eq!(lexicon.probe("fgf").expect("追加表的新码")[0].text, "𠀀");
        assert_eq!(lexicon.codes_entries, 2);
        std::fs::remove_dir_all(&dir).ok();
    }

    /// 文件名不合前后缀的（备份、空名、别的方案）都不算追加表；缺失追加表时主表照常。
    #[test]
    fn extra_code_table_requires_the_name_pattern() {
        let dir = hux_test_support::temp_dir("lexicon-extra-codes-pattern");
        std::fs::write(dir.join(CODES_FILE), "来\ta\n").unwrap();
        std::fs::write(dir.join("tiger_sentence.codes.txt.bak"), "不该被读\tzz\n").unwrap();
        std::fs::write(dir.join("tiger_sentence.codes..txt"), "不该被读\tzy\n").unwrap();
        std::fs::write(dir.join("别的.codes.extra.txt"), "不该被读\tzx\n").unwrap();
        let lexicon = Lexicon::load(std::slice::from_ref(&dir), 0);
        assert!(lexicon.probe("zz").is_none());
        assert!(lexicon.probe("zy").is_none());
        assert!(lexicon.probe("zx").is_none());
        assert_eq!(lexicon.codes_entries, 1);
        assert_eq!(lexicon.probe("a").unwrap()[0].text, "来");
        std::fs::remove_dir_all(&dir).ok();
    }

    /// 追加表只从**主表所在的数据目录**取：别的数据目录里的追加表不混进来
    /// （差分夹具目录提供主表、`data/` 只提供词先验，正是这种用法）；同目录的追加表
    /// 按文件名字典序拼在主表之后。
    #[test]
    fn extra_code_tables_only_come_from_the_primary_directory() {
        let fixture = hux_test_support::temp_dir("lexicon-extra-fixture");
        let other = hux_test_support::temp_dir("lexicon-extra-other");
        std::fs::write(fixture.join(CODES_FILE), "甲\tab\n").unwrap();
        std::fs::write(other.join("tiger_sentence.codes.zzz.txt"), "乙\tab\n").unwrap();
        let texts_of = |lexicon: &Lexicon| -> Vec<String> {
            lexicon
                .probe("ab")
                .expect("码 ab")
                .iter()
                .map(|entry| entry.text.clone())
                .collect()
        };
        let lexicon = Lexicon::load(&[fixture.clone(), other.clone()], 0);
        assert_eq!(
            texts_of(&lexicon),
            vec!["甲"],
            "别的目录里的追加表不该被读入"
        );

        std::fs::write(fixture.join("tiger_sentence.codes.bbb.txt"), "丙\tab\n").unwrap();
        std::fs::write(fixture.join("tiger_sentence.codes.aaa.txt"), "丁\tab\n").unwrap();
        let lexicon = Lexicon::load(&[fixture.clone(), other.clone()], 0);
        assert_eq!(texts_of(&lexicon), vec!["甲", "丁", "丙"]);
        assert_eq!(
            lexicon
                .extra_code_tables()
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>(),
            vec![
                "tiger_sentence.codes.aaa.txt",
                "tiger_sentence.codes.bbb.txt"
            ],
            "诊断口径应给出实际装载的追加表（按字典序）"
        );
        std::fs::remove_dir_all(&fixture).ok();
        std::fs::remove_dir_all(&other).ok();
    }

    /// 每张追加表各自剥 BOM：`normalize_text_content` 只剥得掉合并内容最前面那个，
    /// 否则第二张表起首行的 BOM 会粘进候选文本。
    #[test]
    fn extra_code_table_bom_is_stripped_per_table() {
        let dir = hux_test_support::temp_dir("lexicon-extra-bom");
        std::fs::write(dir.join(CODES_FILE), "\u{feff}甲\ta\n").unwrap();
        std::fs::write(dir.join("tiger_sentence.codes.aaa.txt"), "\u{feff}乙\tab\n").unwrap();
        std::fs::write(dir.join("tiger_sentence.codes.bbb.txt"), "\u{feff}丙\tab\n").unwrap();
        let lexicon = Lexicon::load(std::slice::from_ref(&dir), 0);
        assert_eq!(lexicon.probe("a").expect("码 a")[0].text, "甲");
        let ab: Vec<&str> = lexicon
            .probe("ab")
            .expect("码 ab")
            .iter()
            .map(|entry| entry.text.as_str())
            .collect();
        assert_eq!(ab, vec!["乙", "丙"], "追加表的 BOM 未逐表剥掉");
        std::fs::remove_dir_all(&dir).ok();
    }

    /// 逐表解析与「拼成一个大串再解析」同语义：跨表去重按**小写化后**的码
    /// （同一 `(字, 码)` 的大小写变体算重复），主表条目照旧保首见。
    #[test]
    fn extra_code_table_dedup_is_case_insensitive_across_tables() {
        let dir = hux_test_support::temp_dir("lexicon-extra-dedup");
        std::fs::write(dir.join(CODES_FILE), "来\tA\n").unwrap();
        std::fs::write(dir.join("tiger_sentence.codes.aaa.txt"), "来\ta\n甲\ta\n").unwrap();
        let lexicon = Lexicon::load(std::slice::from_ref(&dir), 0);
        let entries = lexicon.probe("a").expect("码 a");
        assert_eq!(
            entries
                .iter()
                .map(|entry| entry.text.as_str())
                .collect::<Vec<_>>(),
            vec!["来", "甲"],
            "追加表里与主表重复的 (字, 码) 大小写变体应被丢掉"
        );
        assert_eq!(lexicon.codes_entries, 2);
        std::fs::remove_dir_all(&dir).ok();
    }

    /// 关掉全字集：追加表不入词库（连读盘都不做），诊断口径为空；重新打开即恢复。
    #[test]
    fn extra_code_tables_can_be_disabled() {
        let dir = hux_test_support::temp_dir("lexicon-full-charset");
        std::fs::write(dir.join(CODES_FILE), "来\ta\n").unwrap();
        std::fs::write(dir.join("tiger_sentence.codes.huma.txt"), "𠀀\tfgf\n").unwrap();
        let options = LexiconOptions {
            extra_code_tables: false,
            ..LexiconOptions::default()
        };
        let mut lexicon = Lexicon::load_with(std::slice::from_ref(&dir), 0, options);
        assert!(lexicon.probe("fgf").is_none(), "关掉全字集时追加表不入词库");
        assert!(lexicon.extra_code_tables().is_empty());
        assert_eq!(lexicon.codes_entries, 1);
        assert_eq!(lexicon.options(), options);
        assert_eq!(
            lexicon.data_info(),
            "code_tables=[tiger_sentence.codes.txt] entries=1 chars=1 full_charset=0 \
             filter_non_han=1"
        );

        // 重新打开：与缺省口径（[`Lexicon::load`]）逐项一致。
        lexicon.apply_lexicon_options(0, LexiconOptions::default());
        assert_eq!(lexicon.probe("fgf").expect("追加表的码")[0].text, "𠀀");
        assert_eq!(
            lexicon.extra_code_tables(),
            ["tiger_sentence.codes.huma.txt".to_string()]
        );
        assert_eq!(lexicon.codes_entries, 2);
        assert_eq!(
            lexicon.data_info(),
            "code_tables=[tiger_sentence.codes.txt,tiger_sentence.codes.huma.txt] entries=2 \
             chars=2 full_charset=1 filter_non_han=1"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    /// 过滤只作用于**追加表**：主表行（差分金样夹具 = 上游主表，含 3 个非汉字）照旧，
    /// 追加表里的单字符非汉字被丢掉；词与扩展区汉字不受影响。
    #[test]
    fn filter_non_han_only_applies_to_extra_code_tables() {
        let dir = hux_test_support::temp_dir("lexicon-filter-non-han");
        // 主表：一个非汉字（`々`）+ 一个汉字。
        std::fs::write(dir.join(CODES_FILE), "々\taa\n来\tab\n").unwrap();
        // 追加表：单字符非汉字（部首 `⽧`）/ 含非汉字的词 / 扩展 B 汉字。
        std::fs::write(
            dir.join("tiger_sentence.codes.extra.txt"),
            "⽧\tba\n々々\tbb\n𤕫\tbc\n",
        )
        .unwrap();

        let lexicon = Lexicon::load(std::slice::from_ref(&dir), 0);
        assert_eq!(
            lexicon.probe("aa").expect("主表的非汉字")[0].text,
            "々",
            "主表行不受过滤影响"
        );
        assert!(
            lexicon.probe("ba").is_none(),
            "追加表的单字符非汉字应被丢掉"
        );
        assert_eq!(lexicon.probe("bb").expect("含非汉字的词")[0].text, "々々");
        assert_eq!(lexicon.probe("bc").expect("扩展 B 汉字")[0].text, "𤕫");
        let filtered_entries = lexicon.codes_entries;

        // 关掉过滤：追加表的非汉字行入词库（正好多一条），主表条目照旧。
        let unfiltered = Lexicon::load_with(
            std::slice::from_ref(&dir),
            0,
            LexiconOptions {
                filter_non_han: false,
                ..LexiconOptions::default()
            },
        );
        assert_eq!(unfiltered.probe("ba").expect("关掉过滤")[0].text, "⽧");
        assert_eq!(unfiltered.codes_entries, filtered_entries + 1);
        assert_eq!(unfiltered.probe("aa").expect("主表的非汉字")[0].text, "々");
        assert_eq!(unfiltered.probe("bb").expect("词")[0].text, "々々");
        std::fs::remove_dir_all(&dir).ok();
    }
}
