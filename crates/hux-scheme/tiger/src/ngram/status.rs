// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! `cache_status` 快照：字段与 Lua 表一一对应。

/// `cache_status` 快照；字段与 Lua 表一一对应。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CacheStatus {
    pub page_bytes: u64,
    pub page_limit: usize,
    pub resident_index_bytes: u64,
    pub index_cache_bytes: u64,
    pub index_cache_limit: u64,
    pub index_misses: u64,
    pub index_bytes_read: u64,
    pub bigram_entries: usize,
    pub bigram_limit: usize,
    pub context_entries: usize,
    pub context_limit: usize,
    pub bigram_hits: u64,
    pub bigram_misses: u64,
}

impl CacheStatus {
    /// 差分金样使用的规范文本（固定字段序）。
    pub fn canonical(&self) -> String {
        format!(
            "page_bytes={}\tpage_limit={}\tresident_index_bytes={}\tindex_cache_bytes={}\t\
             index_cache_limit={}\tindex_misses={}\tindex_bytes_read={}\tbigram_entries={}\t\
             bigram_limit={}\tcontext_entries={}\tcontext_limit={}\tbigram_hits={}\tbigram_misses={}",
            self.page_bytes,
            self.page_limit,
            self.resident_index_bytes,
            self.index_cache_bytes,
            self.index_cache_limit,
            self.index_misses,
            self.index_bytes_read,
            self.bigram_entries,
            self.bigram_limit,
            self.context_entries,
            self.context_limit,
            self.bigram_hits,
            self.bigram_misses,
        )
    }
}
