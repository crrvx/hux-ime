// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! 词库文件名、字集开关、装载状态与数据目录的读取。
//!
//! 词库与语言模型文件名的单一来源；[`LexiconOptions`] 是字集开关，[`DataStatus`] 是
//! `data_status()` 的稳定字段；逐目录读盘的 `Lexicon` 方法也落在本模块。

use std::borrow::Cow;
use std::path::Path;

use super::Lexicon;
use super::parse::{CodeTables, ExtraCodeTable};

/// 未知字符的字频回退（参照 `unknown_character_rank`）。
pub(super) const UNKNOWN_CHARACTER_RANK_FALLBACK: usize = 20001;

// ---------------------------------------------------------------- 词库文件名
//
// 词库与语言模型文件名的单一来源：本模块的加载路径与产物清单（`scheme::ASSETS`）都取自这里。

/// 主码表文件名（**必需**）。
pub(crate) const CODES_FILE: &str = "tiger_sentence.codes.txt";
/// 追加码表的前后缀：`tiger_sentence.codes.<name>.txt`（`<name>` 至少一个字符）。
/// 主表与全部追加表按确定顺序拼接后一起解析（见 [`Lexicon::read_code_tables`]）；
/// 追加表是可选项（内核按文件名字典序拼在主表之后，见 `data/README.md`），不进 `scheme::ASSETS`。
const CODES_EXTRA_PREFIX: &str = "tiger_sentence.codes.";
const CODES_EXTRA_SUFFIX: &str = ".txt";
/// 字频表文件名。
pub(crate) const RANKS_FILE: &str = "tiger_sentence.char_ranks.txt";
/// 全码白名单文件名。
pub(crate) const WHITELIST_FILE: &str = "tiger_sentence.full_code_whitelist.txt";
/// 补充词库文件名。
pub(crate) const SUPPLEMENT_FILE: &str = "tiger_sentence.supplement.txt";
/// 词先验位图文件名（参考 `tiger_sentence.lexical.bin`）。
pub(crate) const LEXICAL_FILE: &str = "tiger_sentence.lexical.bin";
/// 语言模型相对路径（参考 `models/sentence-ngram-mobile.bin`）。
pub(crate) const MODEL_PATH: &str = "models/sentence-ngram-mobile.bin";

/// 字集开关（[`Lexicon::load_with`] / `Lexicon::apply_lexicon_options` 的入参）。
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

// ---------------------------------------------------------------- 文本工具

/// Lua 模式类 `%s` 的 ASCII 空白集合（含垂直制表符，Rust `trim()` 不含）。
fn is_lua_space(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\n' | '\u{0b}' | '\u{0c}' | '\r')
}

pub(super) fn is_lua_space_byte(byte: u8) -> bool {
    matches!(byte, b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r')
}

pub(super) fn trim_lua_whitespace(text: &str) -> &str {
    text.trim_matches(is_lua_space)
}

/// 参照 `normalize_text_content`：去 BOM，CRLF/CR → LF。
/// 无需改动时借用原串（逐表装载时不再为每张表复制一份内容）。
pub(super) fn normalize_text_content(content: &str) -> Cow<'_, str> {
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
pub(super) fn each_content_line<'a>(content: &'a str, mut callback: impl FnMut(&'a str)) {
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
pub(super) fn first_two_tokens(line: &str) -> Option<(&str, &str)> {
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

pub(super) fn is_single_character(text: &str) -> bool {
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
pub(super) fn is_filtered_non_han(word: &str) -> bool {
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

impl Lexicon {
    /// 数据文件按 UTF-8 文本读取；不可读（含非法 UTF-8）视为缺失。
    /// 参照实现对字节流宽松，本项目数据文件均为 UTF-8。
    pub(super) fn read_data_file(&self, name: &str) -> Option<(String, String)> {
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
    pub(super) fn read_code_tables(&self) -> Option<CodeTables> {
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
}
