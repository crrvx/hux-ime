// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! 移动端 n-gram 的字节级读取：哨兵常量、打包规则与越界检查。

use anyhow::{Result, anyhow, bail};

use super::{BOS_CODE, EOS_CODE, Limits};

pub(super) const BOS: &str = "\u{2}";
pub(super) const EOS: &str = "\u{3}";

/// 42-bit 三元组/二元组打包位移（2^21）。
pub(super) const SHIFT: u64 = 2_097_152;

/// 参照 `scalar`：空串→0，BOS/EOS→2/3，其余取首个码位。
pub(super) fn scalar(token: &str) -> u32 {
    if token.is_empty() {
        return 0;
    }
    if token == BOS {
        return BOS_CODE;
    }
    if token == EOS {
        return EOS_CODE;
    }
    token.chars().next().map(|c| c as u32).unwrap_or(0)
}

/// 参照 `pack2`：`first * SHIFT + second % SHIFT`。
pub(super) fn pack2(first: u32, second: u32) -> u64 {
    first as u64 * SHIFT + second as u64 % SHIFT
}

pub(super) fn le_u32(data: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(data[offset..offset + 4].try_into().expect("bounds checked"))
}

pub(super) fn le_u64(data: &[u8], offset: usize) -> u64 {
    u64::from_le_bytes(data[offset..offset + 8].try_into().expect("bounds checked"))
}

pub(super) fn le_f32(data: &[u8], offset: usize) -> f32 {
    f32::from_le_bytes(data[offset..offset + 4].try_into().expect("bounds checked"))
}

/// 缓存上限校验：`Fifo::new` / `Columns::new` 要求上限 ≥ 1（否则 `assert!` panic），
/// `load` 与 `configure_cache` 两条入口共用同一口径。
pub(super) fn validate_limits(limits: Limits) -> Result<()> {
    if limits.page_bytes < 1
        || limits.context_entries < 1
        || limits.bigram_entries < 1
        || limits.index_pages < 1
    {
        bail!("invalid model cache limits");
    }
    Ok(())
}

/// 对照 Lua `read_at`：越界即“truncated mobile n-gram”。
pub(super) fn read_at(map: &[u8], offset: u64, count: usize) -> Result<&[u8]> {
    let start = usize::try_from(offset).map_err(|_| anyhow!("truncated mobile n-gram"))?;
    let end = start
        .checked_add(count)
        .ok_or_else(|| anyhow!("truncated mobile n-gram"))?;
    map.get(start..end)
        .ok_or_else(|| anyhow!("truncated mobile n-gram"))
}
