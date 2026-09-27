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

use hux_core::collections::{Map, Set};
use hux_core::session::Candidate;
use std::path::PathBuf;

use files::UNKNOWN_CHARACTER_RANK_FALLBACK;
use index::build_lexicon_index;
use parse::{
    learning_rules_fingerprint, parse_code_tables, parse_ranks_file, parse_whitelist_file,
};

mod files;
mod index;
mod parse;
mod query;
mod supplement;
#[cfg(test)]
mod tests;

pub(crate) use files::{
    CODES_FILE, LEXICAL_FILE, MODEL_PATH, RANKS_FILE, SUPPLEMENT_FILE, WHITELIST_FILE,
};
pub use files::{DataStatus, LexiconOptions};
pub use supplement::{Supplement, SupplementStatus};

/// 码注释（上游音反查件；当前 pin 的 main 未含，音反查接线用）：单字显示全部编码（源序），词组逐字 `字:码组`。
pub(crate) fn code_comment(lexicon: &Lexicon, text: &str) -> Option<String> {
    if !lexicon.built {
        return None;
    }
    let chars: Vec<char> = text.chars().collect();
    if chars.is_empty() {
        return None;
    }
    if chars.len() == 1 {
        let codes = lexicon.character_codes.get(&chars[0].to_string())?;
        if codes.is_empty() {
            return None;
        }
        return Some(format!(" {}", codes.join(" / ")));
    }
    let mut parts = Vec::with_capacity(chars.len());
    for ch in &chars {
        match lexicon.character_codes.get(&ch.to_string()) {
            Some(codes) if !codes.is_empty() => {
                parts.push(format!("{}:{}", ch, codes.join("/")));
            }
            _ => parts.push(format!("{}:?", ch)),
        }
    }
    Some(format!(" {}", parts.join(" ")))
}

/// 码注释过滤器（同上；音反查接线用）：音反查段候选写入虎码注释。
pub(crate) fn code_comment_filter(candidates: &mut [Candidate], active: bool, lexicon: &Lexicon) {
    if !active {
        return;
    }
    for candidate in candidates {
        if let Some(comment) = code_comment(lexicon, &candidate.text) {
            candidate.comment = comment;
        }
    }
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
    /// 参照 `M.apply_high_freq_limit` 的重建路径；负数由调用方归一为 0
    /// （Rust 侧 API 为 `usize`，与参照的 `value < 0 → 0` 等价）。
    pub fn apply_high_freq_limit(&mut self, limit: usize) {
        self.rebuild(limit);
    }

    /// 应用字集开关与高频上限：两者都改词库内容，故一并落位后**只重建一次**
    /// （只改开关的调用方传当前上限即可；差分重放走单参数的
    /// [`Lexicon::apply_high_freq_limit`]）。
    pub(crate) fn apply_lexicon_options(&mut self, limit: usize, options: LexiconOptions) {
        self.options = options;
        self.rebuild(limit);
    }

    /// 重建索引（装载码表 / 字频 / 白名单 / 补充词库后调用）。
    fn rebuild(&mut self, limit: usize) {
        let mut errors = Vec::new();

        let tables = self.read_code_tables();
        let (entries, code_tables) = parse_code_tables(tables.as_ref(), self.options, &mut errors);

        let ranks_file = self.read_data_file(RANKS_FILE);
        let (character_ranks, ranks_count) = parse_ranks_file(ranks_file.as_ref());

        let whitelist_file = self.read_data_file(WHITELIST_FILE);
        let whitelist = parse_whitelist_file(whitelist_file.as_ref());
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
        self.learning_rules = learning_rules_fingerprint(
            tables.as_ref(),
            ranks_file.as_ref(),
            whitelist_file.as_ref(),
        );
        self.errors = errors;
    }
}
