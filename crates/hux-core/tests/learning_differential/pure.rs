// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! 纯函数记录：不依赖任何已构建索引的金样记录。

use hux_core::learning;
use hux_test_support::{decode_bytes, decode_hex, parse_bits};

/// `hash`：文本哈希。
pub fn check_hash(rest: &str) {
    let fields: Vec<&str> = rest.split('\t').collect();
    let text = decode_hex(fields[0]);
    assert_eq!(
        learning::hash(&text),
        fields[1],
        "hash mismatch for {text:?}"
    );
}

/// `maturity`：成熟度逐位比对。
pub fn check_maturity(rest: &str) {
    let fields: Vec<&str> = rest.split('\t').collect();
    let score = f64::from_bits(parse_bits(fields[0]));
    let expected = parse_bits(fields[1]);
    assert_eq!(
        learning::early_commit_maturity(score).to_bits(),
        expected,
        "maturity mismatch for {score}"
    );
}

/// `contribution`：贡献度逐位比对。
pub fn check_contribution(rest: &str) {
    let fields: Vec<&str> = rest.split('\t').collect();
    let score = f64::from_bits(parse_bits(fields[0]));
    let expected = parse_bits(fields[1]);
    assert_eq!(
        learning::early_commit_contribution(score).to_bits(),
        expected,
        "contribution mismatch for {score}"
    );
}

/// `fusionmode`：融合模式名。
pub fn check_fusion_mode(rest: &str) {
    let fields: Vec<&str> = rest.split('\t').collect();
    let mode = decode_hex(fields[0]);
    assert_eq!(
        learning::fusion_mode(&mode),
        decode_hex(fields[1]),
        "fusion_mode mismatch for {mode:?}"
    );
}

/// `paircode`：融合配对码。
pub fn check_pair_code(rest: &str) {
    let fields: Vec<&str> = rest.split('\t').collect();
    let raw = decode_bytes(fields[0]);
    let direct = decode_hex(fields[1]);
    let composed = decode_hex(fields[2]);
    assert_eq!(
        learning::fusion_pair_code(&raw, &direct, &composed),
        decode_hex(fields[3]),
        "fusion_pair_code mismatch for ({raw:?},{direct:?},{composed:?})"
    );
}
