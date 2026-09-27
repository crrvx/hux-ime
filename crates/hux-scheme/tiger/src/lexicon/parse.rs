// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! 码表 / 字频 / 白名单的文本解析与学习指纹。

use hashbrown::HashSet;
use hux_core::collections::{Map, Set};
use std::borrow::Cow;

use super::files::{
    CODES_FILE, LexiconOptions, each_content_line, first_two_tokens, is_filtered_non_han,
    normalize_text_content,
};

// ---------------------------------------------------------------- 解析

/// 参照 `parse_codes_content`：`word code`，去重 `(word, code)`，保源序。
///
/// 读取方只有本文件单测（生产路径直接走 `append_codes_content`），故收为私有并随测试编译。
#[cfg(test)]
pub(super) fn parse_codes_content(content: &str) -> Vec<(String, String)> {
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
pub(super) fn append_codes_content<'a>(
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
pub(super) fn parse_ranks_content(content: &str) -> (Map<String, usize>, usize) {
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
pub(super) fn parse_whitelist_content(content: &str) -> Set<String> {
    let mut characters = Set::new();
    each_content_line(&normalize_text_content(content), |line| {
        for character in line.chars() {
            characters.insert(character.to_string());
        }
    });
    characters
}

/// `rebuild` 的字频侧：解析文件内容，空表视为未装载。
pub(super) fn parse_ranks_file(
    file: Option<&(String, String)>,
) -> (Option<Map<String, usize>>, usize) {
    match file {
        Some((content, _)) => {
            let (ranks, count) = parse_ranks_content(content);
            if count == 0 {
                (None, 0)
            } else {
                (Some(ranks), count)
            }
        }
        None => (None, 0),
    }
}

/// `rebuild` 的白名单侧：文件缺失即空集。
pub(super) fn parse_whitelist_file(file: Option<&(String, String)>) -> Set<String> {
    match file {
        Some((content, _)) => parse_whitelist_content(content),
        None => Set::new(),
    }
}

/// 一张追加码表：文件名 + 原始内容（BOM 未剥，剥法见 [`ExtraCodeTable::body`]）。
pub(super) struct ExtraCodeTable {
    pub(super) name: String,
    pub(super) content: String,
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
pub(super) struct CodeTables {
    pub(super) primary: String,
    pub(super) extras: Vec<ExtraCodeTable>,
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

/// 主表与追加表的解析合并：`codes.txt` 行序即 rank，同一 `(word, code)` 去重保首见。
///
/// 追加表按文件名字典序拼在主表之后，故主表内所有 rank 逐位不变。主表缺失时记一条错误，
/// 返回空表——`built` 仍为真，只是词库为空（与原实现一致）。
pub(super) fn parse_code_tables(
    tables: Option<&CodeTables>,
    options: LexiconOptions,
    errors: &mut Vec<String>,
) -> (Vec<(String, String)>, Vec<String>) {
    let mut entries: Vec<(String, String)> = Vec::new();
    let mut code_tables: Vec<String> = Vec::new();
    if let Some(tables) = tables {
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
            let filter = order > 0 && options.filter_non_han;
            append_codes_content(content, filter, &mut seen, &mut entries);
        }
    } else {
        errors.push(format!("missing {CODES_FILE}"));
    }
    (entries, code_tables)
}

/// 规则指纹：三份文件内容（缺失视为空串）逐段喂给 `hash_parts`。
pub(super) fn learning_rules_fingerprint(
    tables: Option<&CodeTables>,
    ranks_file: Option<&(String, String)>,
    whitelist_file: Option<&(String, String)>,
) -> String {
    // 参照 `build_lexicon_index` 末尾：以三份文件内容（缺失视为空串）计算规则指纹。
    // 码表侧取的是**合并后**的内容（主表 + 追加表）：追加表变化即触发重新学习。
    // 逐表分段喂给 `hash_parts`（拼接结果与合并成一个大串逐字节相同），故出厂口径下
    // 指纹与优化前**逐位一致**（升级不重置既有学习库分区）；过滤只丢解析后的条目，
    // 不进指纹。关掉全字集时装载内容即主表，指纹随之只含主表。
    let mut hash_parts: Vec<&str> = tables
        .map(CodeTables::fingerprint_parts)
        .unwrap_or_default();
    hash_parts.push("\0");
    hash_parts.push(
        ranks_file
            .map(|(content, _)| content.as_str())
            .unwrap_or(""),
    );
    hash_parts.push("\0");
    hash_parts.push(
        whitelist_file
            .map(|(content, _)| content.as_str())
            .unwrap_or(""),
    );
    hux_core::learning::hash_parts(&hash_parts)
}
