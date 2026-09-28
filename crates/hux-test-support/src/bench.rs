// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! 基准（`examples/*_bench.rs`）共用的命令行取值与耗时分位数。

/// `--name value` 形式的命令行取值：取 `args` 中 `--name` 的后一个参数。
pub fn flag(args: &[String], name: &str) -> Option<String> {
    args.iter()
        .position(|arg| arg == name)
        .and_then(|index| args.get(index + 1))
        .cloned()
}

/// 从升序样本取分位耗时（微秒；样本单位为纳秒）。`sorted` 不得为空。
pub fn quantile_us(sorted: &[u64], quantile: f64) -> f64 {
    let index = ((sorted.len() as f64 - 1.0) * quantile).round() as usize;
    sorted[index] as f64 / 1000.0
}
