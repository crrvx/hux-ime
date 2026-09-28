// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! 差分测试：重放 Lua 参照实现生成的金样 transcript，逐位比对。
//!
//! 金样由 `tools/generators/gen_ngram_golden.lua` 生成：
//! * fixture 模式入库（`goldens/ngram_fixture.*`）；
//! * sample 模式对真实模型抽样，仅本地（`goldens/local/`，不入库；缺失即跳过）。

// 集成测试目标里 `mod x;` 相对 `tests/` 解析，故用显式路径指向本文件同名目录。
#[path = "ngram_differential/transcript.rs"]
mod transcript;

use hux_scheme_tiger::ngram::MobileModel;
use hux_test_support::{open_golden, repo_path, try_open_golden};
use std::path::PathBuf;

/// 真实模型样例差分（`goldens/local/`，不入库）缺失时的处理。
///
/// 缺省**跳过**（CI 无 448 MiB 模型与本地抽样，`cargo test --workspace` 仍应全绿——
/// 该取舍是有意的：CI 对真实模型路径零守护）；
/// 置 `HUX_REQUIRE_SAMPLE=1` 时改为**失败**：本地复验 / 专项 CI 用它强制覆盖真实路径
/// （配合 `tools/generators/` 下的 sample 生成器）。
fn sample_missing(reason: &str) {
    assert!(
        std::env::var_os("HUX_REQUIRE_SAMPLE").as_deref() != Some(std::ffi::OsStr::new("1")),
        "HUX_REQUIRE_SAMPLE=1：真实模型差分必须可跑，但 {reason}"
    );
    eprintln!("skip: {reason}");
}

#[test]
fn ngram_fixture_transcript_is_bit_exact() {
    let path = repo_path("goldens/ngram_fixture.bin");
    let mut model = MobileModel::load(&path, None).expect("load fixture model");
    let reader = open_golden("goldens/ngram_fixture.tsv.gz");
    let records = transcript::run(&mut model, reader);
    assert!(records > 29_000, "transcript too short: {records} records");
    println!("fixture: {records} golden records verified bit-exact");
}

#[test]
fn ngram_sample_transcript_is_bit_exact_when_present() {
    let Some(reader) = try_open_golden("goldens/local/ngram_sample.tsv.gz") else {
        sample_missing("goldens/local/ngram_sample.tsv.gz not present (local-only sample)");
        return;
    };
    let Some(model_path) = std::env::var_os("HUX_NGRAM_MODEL")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME").map(|home| {
                PathBuf::from(home)
                    .join(".local/share/fcitx5/rime/models/sentence-ngram-mobile.bin")
            })
        })
    else {
        sample_missing("no sample model path (set HUX_NGRAM_MODEL)");
        return;
    };
    if !model_path.is_file() {
        sample_missing(&format!(
            "sample model not found at {}",
            model_path.display()
        ));
        return;
    }
    let mut model = MobileModel::load(&model_path, None).expect("load sample model");
    let records = transcript::run(&mut model, reader);
    assert!(records > 60_000, "transcript too short: {records} records");
    println!("sample: {records} golden records verified bit-exact");
}
