// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! 差分测试：重放键金样（由 librime 探针生成），逐条比对。
//!
//! 金样 `goldens/key.tsv.gz` 由 `tools/generators/gen_key_golden.sh` 生成
//! （系统 librime 1.17.0 + `tools/cases/key_cases.txt`）。

// 集成测试目标里 `mod x;` 相对 `tests/` 解析，故用显式路径指向本文件同名目录。
#[path = "key_differential/transcript.rs"]
mod transcript;

#[test]
fn key_transcript_is_bit_exact() {
    let records = transcript::run();
    assert!(records > 5_000, "transcript too short: {records} records");
    println!("key: {records} golden records verified");
}
