// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! 词库的查询、诊断与码注释。

use std::path::PathBuf;

use super::CodeEntry;
use super::Lexicon;
use super::files::{DataStatus, LexiconOptions};

impl Lexicon {
    /// 本次装载生效的字集开关。
    pub(crate) fn options(&self) -> LexiconOptions {
        self.options
    }
    /// 实际装载的追加码表文件名（按装载顺序）；关掉全字集时为空。诊断用；不进
    /// [`DataStatus::canonical`]（那是差分金样的比对文本，加字段会动到金样）。
    pub fn extra_code_tables(&self) -> &[String] {
        // 首项是主表（见 [`Lexicon::read_code_tables`]）；主表缺失时整表为空。
        self.code_tables.get(1..).unwrap_or_default()
    }

    /// 数据装载摘要（`hux_engine_data_info`）：装了哪几张码表 + 条目数 / 单字数 +
    /// 两个字集开关的生效值。
    pub(crate) fn data_info(&self) -> String {
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
    pub(crate) fn dirs(&self) -> &[PathBuf] {
        &self.dirs
    }
}
