//! 与 `tools/probes/bench_ngram.lua` 对齐的基准：加载模型后重放 transcript 中的全部
//! logp 查询，输出加载/查询耗时与结果位模式校验和（xor）。
//!
//!   cargo run --release --example ngram_bench -- <model.bin> <transcript.tsv>

use hux_core::ngram::MobileModel;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::time::Instant;

fn decode(text: &str) -> String {
    if text == "-" {
        return String::new();
    }
    let bytes: Vec<u8> = (0..text.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&text[i..i + 2], 16).expect("hex digit"))
        .collect();
    String::from_utf8(bytes).expect("valid utf-8")
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    assert_eq!(
        args.len(),
        3,
        "usage: ngram_bench <model.bin> <transcript.tsv>"
    );

    let started = Instant::now();
    let mut model = MobileModel::load(&args[1], None).expect("load model");
    let loaded = started.elapsed();

    let mut queries = 0u64;
    let mut checksum = 0u64;
    let query_started = Instant::now();
    let reader = BufReader::new(File::open(&args[2]).expect("open transcript"));
    for line in reader.lines() {
        let line = line.expect("read line");
        let Some(rest) = line.strip_prefix("logp\t") else {
            continue;
        };
        let mut parts = rest.split('\t');
        let prev2 = decode(parts.next().expect("arg"));
        let prev1 = decode(parts.next().expect("arg"));
        let target = decode(parts.next().expect("arg"));
        let value = model.logp(&prev2, &prev1, &target).expect("logp");
        checksum ^= value.to_bits();
        queries += 1;
    }
    let query = query_started.elapsed();

    println!(
        "{{\"load_ms\":{:.1},\"query_ms\":{:.1},\"queries\":{},\"checksum\":\"0x{:016x}\"}}",
        loaded.as_secs_f64() * 1000.0,
        query.as_secs_f64() * 1000.0,
        queries,
        checksum
    );
}
