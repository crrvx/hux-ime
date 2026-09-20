// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! 差分测试共用工具。
//!
//! cargo 约定：`tests/` 子目录不构成独立测试目标；各测试文件以 `mod common;` 引入。
//! P5 由 `hux-test-support` crate 承载（见 `docs/refactor.md` §6），届时本文件移除。
#![allow(dead_code)] // 各测试二进制只使用其中一部分

use flate2::read::GzDecoder;
use hux_core::decode::Decoder;
use hux_core::lexicon::{Lexicon, Supplement};
use hux_core::ngram::MobileModel;
use std::fs::File;
use std::io::BufReader;
use std::path::PathBuf;

pub fn repo_path(relative: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(relative)
}

/// transcript 字符串参数：`-` 表示空串，其余为 UTF-8 字节的小写十六进制。
pub fn decode_hex(text: &str) -> String {
    String::from_utf8(decode_bytes(text)).expect("valid UTF-8")
}

pub fn decode_bytes(text: &str) -> Vec<u8> {
    if text == "-" {
        return Vec::new();
    }
    assert!(
        text.len() % 2 == 0 && text.bytes().all(|byte| byte.is_ascii_hexdigit()),
        "bad hex field: {text:?}"
    );
    (0..text.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&text[i..i + 2], 16).expect("hex digit"))
        .collect()
}

/// `0x` + hi/lo 两个 u32 半字（对应 Lua `string.unpack("<I4I4", string.pack("<d", v))`）。
pub fn parse_bits(text: &str) -> u64 {
    let digits = text.strip_prefix("0x").expect("0x prefix");
    assert_eq!(digits.len(), 16, "bits must be 16 hex digits: {text}");
    let hi = u64::from_str_radix(&digits[..8], 16).expect("hex digit");
    let lo = u64::from_str_radix(&digits[8..], 16).expect("hex digit");
    hi << 32 | lo
}

/// 字节串 → transcript 十六进制（空串为 `-`）。
pub fn hex(bytes: &[u8]) -> String {
    if bytes.is_empty() {
        return "-".to_string();
    }
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

pub fn field<'a>(part: &'a str, name: &str) -> &'a str {
    part.strip_prefix(name)
        .unwrap_or_else(|| panic!("expected {name}=..., got {part}"))
}

pub fn open_golden(relative: &str) -> BufReader<GzDecoder<File>> {
    try_open_golden(relative).unwrap_or_else(|| panic!("open golden {relative}"))
}

pub fn try_open_golden(relative: &str) -> Option<BufReader<GzDecoder<File>>> {
    let file = File::open(repo_path(relative)).ok()?;
    Some(BufReader::new(GzDecoder::new(file)))
}

/// decode 差分用解码器：`goldens/lexicon` 数据 + `data/` 词先验位图
/// （与金样生成时的参照数据目录一致）。
pub fn make_decoder(model: Option<MobileModel>) -> Decoder {
    let data_dir = repo_path("goldens/lexicon");
    let lexical_dir = repo_path("data");
    let lexicon = Lexicon::load(&[data_dir.clone(), lexical_dir], 1500);
    let supplement = Supplement::load_default(Some(&data_dir));
    Decoder::new(lexicon, supplement, model)
}
