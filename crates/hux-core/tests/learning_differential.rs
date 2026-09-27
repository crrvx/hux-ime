// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! 差分测试：重放 learning 金样（纯计算部分），逐位比对。
//!
//! 金样由 `tools/generators/gen_learning_golden.lua` 生成（`goldens/learning.tsv.gz`）。

// 集成测试目标里 `mod x;` 相对 `tests/` 解析，故用显式路径指向本文件同名目录。
#[path = "learning_differential/events.rs"]
mod events;
#[path = "learning_differential/harness.rs"]
mod harness;
#[path = "learning_differential/index.rs"]
mod index;
#[path = "learning_differential/journal.rs"]
mod journal;
#[path = "learning_differential/pure.rs"]
mod pure;
#[path = "learning_differential/transcript.rs"]
mod transcript;

use hux_test_support::open_golden;

use harness::Harness;
use transcript::run;

#[test]
fn learning_transcript_is_bit_exact() {
    let harness = Harness::new();
    let records = run(harness, open_golden("goldens/learning.tsv.gz"));
    assert!(records > 10_000, "transcript too short: {records} records");
    println!("learning: {records} golden records verified");
}
