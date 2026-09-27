// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! 解码入口：`decode`、`decode_with`、`decode_with_lock`。

use super::*;

impl Decoder {
    /// 参照 `decode(raw_code, false, nil, nil)` 的冷路径。
    pub fn decode(&mut self, raw_code: &str) -> Result<DecodeOutput> {
        self.decode_with(raw_code, false, "")
    }

    /// 参照 `decode(raw_code, include_early_commit, required_text_prefix, nil)` 的冷路径。
    pub fn decode_with(
        &mut self,
        raw_code: &str,
        include_early_commit: bool,
        required_text_prefix: &str,
    ) -> Result<DecodeOutput> {
        self.decode_with_lock(raw_code, include_early_commit, required_text_prefix, None)
    }

    /// 参照 `decode(raw_code, include_early_commit, required_text_prefix, locked)` 的冷路径。
    pub fn decode_with_lock(
        &mut self,
        raw_code: &str,
        include_early_commit: bool,
        required_text_prefix: &str,
        lock: Option<DecodeLock<'_>>,
    ) -> Result<DecodeOutput> {
        let raw = normalize(raw_code);
        if let Some(lock) = lock {
            let prefix = normalize(lock.raw);
            if prefix.is_empty() || !raw.starts_with(&prefix) {
                return Ok(DecodeOutput::empty());
            }
            self.arena.clear();
            self.learning_affected = false;
            let length = raw.len();
            let mut states = self.new_states(length);
            if !self.seed_locked(&raw, &mut states, &prefix, &lock)? {
                return Ok(DecodeOutput::empty());
            }
            self.expand_range(&raw, &mut states, prefix.len(), length, -1)?;
            return self.emit(
                &raw,
                &mut states,
                length,
                include_early_commit,
                required_text_prefix,
            );
        }
        if raw.is_empty() || !has_letter(&raw) {
            return Ok(DecodeOutput::empty());
        }
        self.arena.clear();
        self.learning_affected = false;
        let length = raw.len();
        let mut states = self.new_states(length);
        self.expand_range(&raw, &mut states, 0, length, -1)?;
        self.emit(
            &raw,
            &mut states,
            length,
            include_early_commit,
            required_text_prefix,
        )
    }
}
