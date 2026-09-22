// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! 虎虚（hux-ime）测试助手（P5，克制版）：只放**与业务无关**的共性工具。
//!
//! 约定（`docs/refactor.md` §6）：本 crate 仅承载
//! ① 金样 / 夹具路径定位、② transcript 编解码、③ 临时目录；
//! **不放业务逻辑**（如具体方案的夹具装配、引擎/宿主包装）——那些留在各 crate 的
//! 单元测试或各自 `tests/` 内，避免测试助手长成第二个实现。
//!
//! 各 crate 以 `dev-dependencies` 引入：`hux-test-support = { path = "../hux-test-support" }`。
//! 本 crate 不依赖任何 hux crate（避免测试期成环）。

use flate2::read::GzDecoder;
use std::fs::File;
use std::io::BufReader;
use std::path::PathBuf;

/// 仓库根下的路径（以本 crate 的清单目录为基准，调用方无需关心自身深度）。
pub fn repo_path(relative: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(relative)
}

/// 进程内唯一的临时目录（同名残留先清空）；测试结束由调用方自行清理。
pub fn temp_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("hux-test-{}-{tag}", std::process::id()));
    std::fs::remove_dir_all(&dir).ok();
    std::fs::create_dir_all(&dir).expect("temp dir");
    dir
}

/// 打开入库的 gz 金样（缺失即 panic；需要「本地可能不存在」用 [`try_open_golden`]）。
pub fn open_golden(relative: &str) -> BufReader<GzDecoder<File>> {
    try_open_golden(relative).unwrap_or_else(|| panic!("open golden {relative}"))
}

/// 打开 gz 金样；不存在返回 `None`（本地抽样金样常用）。
pub fn try_open_golden(relative: &str) -> Option<BufReader<GzDecoder<File>>> {
    let file = File::open(repo_path(relative)).ok()?;
    Some(BufReader::new(GzDecoder::new(file)))
}

// ---------------------------------------------------------------- transcript 编解码
//
// 金样为 TSV：`#` 注释、`-` 表示空串、字符串字段为 UTF-8 字节的小写十六进制。

/// transcript 字符串参数：`-` 表示空串，其余为 UTF-8 字节的小写十六进制。
pub fn decode_hex(text: &str) -> String {
    String::from_utf8(decode_bytes(text)).expect("valid UTF-8")
}

/// 同上，保留字节串（偏移语义用）。
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

/// `0x` + hi/lo 两个 u32 半字（对应 Lua `string.unpack("<I4I4", string.pack("<d", v))`）。
pub fn parse_bits(text: &str) -> u64 {
    let digits = text.strip_prefix("0x").expect("0x prefix");
    assert_eq!(digits.len(), 16, "bits must be 16 hex digits: {text}");
    let hi = u64::from_str_radix(&digits[..8], 16).expect("hex digit");
    let lo = u64::from_str_radix(&digits[8..], 16).expect("hex digit");
    hi << 32 | lo
}

/// 取 `name=...` 形式的字段值。
pub fn field<'a>(part: &'a str, name: &str) -> &'a str {
    part.strip_prefix(name)
        .unwrap_or_else(|| panic!("expected {name}=..., got {part}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_roundtrip_and_empty_marker() {
        assert_eq!(hex(b""), "-");
        assert_eq!(hex("虎".as_bytes()), "e8998e");
        assert_eq!(decode_bytes("-"), Vec::<u8>::new());
        assert_eq!(decode_hex("e8998e"), "虎");
        assert_eq!(decode_bytes("e8998e"), "虎".as_bytes());
    }

    #[test]
    fn bits_are_hi_lo_halves() {
        let bits = 1.5f64.to_bits();
        let text = format!("0x{:08x}{:08x}", bits >> 32, bits & 0xffff_ffff);
        assert_eq!(parse_bits(&text), bits);
    }

    #[test]
    fn repo_path_points_at_repository_root() {
        assert!(repo_path("Cargo.toml").is_file());
        assert!(repo_path("goldens/README.md").is_file());
        assert!(repo_path("goldens/regenerate.md").is_file());
    }

    #[test]
    fn temp_dir_is_recreated_empty() {
        let dir = temp_dir("self-check");
        assert!(dir.is_dir());
        std::fs::write(dir.join("stale"), b"x").expect("write");
        let fresh = temp_dir("self-check");
        assert!(!fresh.join("stale").exists());
        std::fs::remove_dir_all(&fresh).ok();
    }
}
