// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! decode 冷路径基准：重放 `goldens/decode.tsv.gz` 的输入语料，报告单次耗时分布。
//!
//! ```sh
//! cargo run --release --example decode_bench -- [--model <bin>] [--lexical <bin>] [--repeat N]
//! ```
//!
//! - 数据：`goldens/lexicon`（夹具码表）+ 可选 `goldens/ngram_fixture.bin` 与 `data/*.lexical.bin`；
//! - 语料：金样 `decode` 行的十六进制输入（与差分测试同一批输入，便于对照）；
//! - 输出：JSON（`ops` / `mean_us` / `p50_us` / `p95_us` / `checksum`），供前后对比。

use hux_scheme_tiger::decode::Decoder;
use hux_scheme_tiger::lexical;
use hux_scheme_tiger::lexicon::{Lexicon, Supplement};
use hux_scheme_tiger::ngram::MobileModel;
use hux_test_support::bench::{flag, quantile_us};
use hux_test_support::{decode_hex, open_golden, repo_path};
use std::io::BufRead;
use std::time::Instant;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let repeat = parse_repeat(&args);
    let lexical_path = flag(&args, "--lexical");

    let mut decoder = build_decoder(&args, lexical_path.as_deref());
    let corpus = load_corpus();
    assert!(!corpus.is_empty(), "语料为空");

    warmup(&mut decoder, &corpus);
    let Samples {
        mut samples,
        mut buckets,
        checksum,
    } = measure(&mut decoder, &corpus, repeat);
    samples.sort_unstable();
    report_summary(&corpus, repeat, lexical_path.is_some(), &samples, checksum);
    // 按输入长度分桶（1-2 / 3-5 / 6-10 / 11-20 / >20 字符）：尾部代价来自哪里。
    report_buckets(&mut buckets);
}

/// 解析 `--repeat`（默认 20）。
fn parse_repeat(args: &[String]) -> usize {
    flag(args, "--repeat")
        .map(|value| value.parse().expect("--repeat"))
        .unwrap_or(20)
}

/// 装载码表 / 追加表与可选模型，返回可复用的解码器。
fn build_decoder(args: &[String], lexical_path: Option<&str>) -> Decoder {
    let data_dir = repo_path("goldens/lexicon");
    let lexicon = Lexicon::load(&[data_dir.clone(), repo_path("data")], 1500);
    let supplement = Supplement::load_default(Some(&data_dir));
    let model = flag(args, "--model").and_then(|path| match MobileModel::load(&path, None) {
        Ok(model) => Some(model),
        Err(error) => {
            eprintln!("model: {error}");
            None
        }
    });
    let mut decoder = Decoder::new(lexicon, supplement, model);
    if let Some(path) = lexical_path {
        let (model, error) = lexical::load_first(&[std::path::PathBuf::from(path)]);
        decoder.set_lexical_model(model);
        if let Some(error) = error {
            eprintln!("lexical: {error}");
        }
    }
    decoder
}

fn load_corpus() -> Vec<String> {
    // 语料：金样里的 decode 输入（十六进制 → 原始输入串）。
    let mut corpus = Vec::new();
    for line in open_golden("goldens/decode.tsv.gz").lines() {
        let line = line.expect("line");
        if let Some(rest) = line.strip_prefix("decode\t") {
            let hex = rest.split('\t').next().expect("hex field");
            corpus.push(decode_hex(hex));
        }
    }
    corpus
}

fn warmup(decoder: &mut Decoder, corpus: &[String]) {
    // 预热（页缓存 / 索引缓存就位），再计时。
    for code in corpus {
        let _ = decoder
            .decode_with_lock(code, false, "", None)
            .expect("decode");
    }
}

/// 输入码长度 → 分桶下标（0..=4）。
fn bucket_of(code: &str) -> usize {
    match code.chars().count() {
        0..=2 => 0,
        3..=5 => 1,
        6..=10 => 2,
        11..=20 => 3,
        _ => 4,
    }
}

/// 计时采样：全部样本 + 分桶样本 + 输出校验和。
struct Samples {
    samples: Vec<u64>,
    buckets: Vec<Vec<u64>>,
    checksum: u64,
}

/// 重放 `repeat` 轮语料，逐条计时并累计校验和。
fn measure(decoder: &mut Decoder, corpus: &[String], repeat: usize) -> Samples {
    let mut samples = Vec::with_capacity(corpus.len() * repeat);
    let mut buckets: Vec<Vec<u64>> = vec![Vec::new(); 5];
    let mut checksum = 0u64;
    for _ in 0..repeat {
        for code in corpus {
            let started = Instant::now();
            let output = decoder
                .decode_with_lock(code, false, "", None)
                .expect("decode");
            let elapsed = started.elapsed().as_nanos() as u64;
            buckets[bucket_of(code)].push(elapsed);
            samples.push(elapsed);
            checksum ^= output.items.len() as u64;
            if let Some(first) = output.items.first() {
                checksum = checksum.rotate_left(1) ^ first.score.to_bits();
            }
        }
    }
    Samples {
        samples,
        buckets,
        checksum,
    }
}

/// 打印总览 JSON（`ops` / `mean_us` / `p50_us` / `p95_us` / `max_us` / `checksum`）。
fn report_summary(corpus: &[String], repeat: usize, lexical: bool, samples: &[u64], checksum: u64) {
    let total: u64 = samples.iter().sum();
    println!(
        "{{\"corpus\":{},\"repeat\":{},\"lexical\":{},\"ops\":{},\"mean_us\":{:.2},\"p50_us\":{:.2},\"p95_us\":{:.2},\"max_us\":{:.2},\"checksum\":\"0x{:016x}\"}}",
        corpus.len(),
        repeat,
        lexical,
        samples.len(),
        total as f64 / samples.len() as f64 / 1000.0,
        quantile_us(samples, 0.50),
        quantile_us(samples, 0.95),
        samples[samples.len() - 1] as f64 / 1000.0,
        checksum,
    );
}

/// 逐桶打印分位耗时（空桶跳过）。
fn report_buckets(buckets: &mut [Vec<u64>]) {
    let names = ["1-2", "3-5", "6-10", "11-20", ">20"];
    for (index, bucket) in buckets.iter_mut().enumerate() {
        if bucket.is_empty() {
            continue;
        }
        bucket.sort_unstable();
        println!(
            "  len {:<6} n={:<6} p50={:>8.2}us p95={:>9.2}us max={:>9.2}us",
            names[index],
            bucket.len(),
            quantile_us(bucket, 0.50),
            quantile_us(bucket, 0.95),
            bucket[bucket.len() - 1] as f64 / 1000.0
        );
    }
}
