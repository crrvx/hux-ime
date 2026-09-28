// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! 差分测试：重放 lexical 金样（TCSLEX01 读取 / Bloom / 最大权词覆盖打分）逐位比对。
//!
//! 金样由 `tools/generators/gen_lexical_golden.lua` 生成：参照 main ≥ `35a10b9` 的词先验模块 +
//! 真实位图 `data/tiger_sentence.lexical.bin`；语料取自参照码表（正例）与确定性
//! 采样（负例），全量记录查询与结果，故重放不依赖任何外部词表。

// 集成测试目标里 `mod x;` 相对 `tests/` 解析，故用显式路径指向本文件同名目录。
#[path = "lexical_differential/transcript.rs"]
mod transcript;

use hux_scheme_tiger::lexical;
use hux_test_support::{open_golden, repo_path};

#[test]
fn lexical_transcript_is_bit_exact() {
    let model = lexical::load(&repo_path("data/tiger_sentence.lexical.bin"))
        .expect("load real lexical model");
    let (records, positives) = transcript::run(&model, open_golden("goldens/lexical.tsv.gz"));
    assert!(records > 700, "transcript too short: {records}");
    assert!(positives > 50, "too few positive samples: {positives}");
    println!("lexical: {records} golden records verified ({positives} positives)");
}
