// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! 解码工具：字节规范化、边界/选择器解析与 beam 上限。

use super::*;

pub(in crate::decode) fn logsumexp(left: f64, right: f64) -> f64 {
    let maximum = left.max(right);
    maximum + ((left - maximum).exp() + (right - maximum).exp()).ln()
}

pub(in crate::decode) fn beam_limit_at(raw_length: usize) -> usize {
    if raw_length > LONG_INPUT_FULL_BEAM_LENGTH {
        LONG_INPUT_BEAM_WIDTH
    } else {
        BEAM_WIDTH
    }
}

/// 参照 `normalize`：ASCII 小写化并去除 Lua `%s` 空白（含垂直制表符）。
pub(in crate::decode) fn normalize(raw: &str) -> Vec<u8> {
    let mut out = Vec::with_capacity(raw.len());
    for byte in raw.bytes() {
        if byte.is_ascii_whitespace() || byte == 0x0b {
            continue;
        }
        out.push(byte.to_ascii_lowercase());
    }
    out
}

pub(in crate::decode) fn has_letter(raw: &[u8]) -> bool {
    raw.iter().any(|byte| byte.is_ascii_alphabetic())
}

/// 参照 `locked.boundaries:gmatch("(%d+),(%d+);")`（失败起点逐一右移重试）。
pub(in crate::decode) fn parse_boundaries(value: &str) -> Vec<(usize, usize)> {
    let bytes = value.as_bytes();
    let mut result = Vec::new();
    let mut index = 0usize;
    while index < bytes.len() {
        let first_start = index;
        while index < bytes.len() && bytes[index].is_ascii_digit() {
            index += 1;
        }
        if index == first_start || bytes.get(index) != Some(&b',') {
            index = first_start + 1;
            continue;
        }
        let first = &value[first_start..index];
        index += 1;
        let second_start = index;
        while index < bytes.len() && bytes[index].is_ascii_digit() {
            index += 1;
        }
        if index == second_start || bytes.get(index) != Some(&b';') {
            index = first_start + 1;
            continue;
        }
        let second = &value[second_start..index];
        index += 1;
        if let (Ok(raw_length), Ok(text_length)) = (first.parse(), second.parse()) {
            result.push((raw_length, text_length));
        }
    }
    result
}

/// 参照 `parse_selector`：返回 (选中 rank, 消耗到的字节位置)；0 = 无选择器。
pub(in crate::decode) fn parse_selector(raw: &[u8], code_end: usize) -> (u64, usize) {
    let next = code_end;
    if next >= raw.len() {
        return (0, code_end);
    }
    match raw[next] {
        b';' => return (2, next + 1),
        b'\'' => return (3, next + 1),
        byte if byte.is_ascii_digit() => {
            let mut digit_end = next;
            while digit_end + 1 < raw.len() && raw[digit_end + 1].is_ascii_digit() {
                digit_end += 1;
            }
            let token = std::str::from_utf8(&raw[next..=digit_end]).unwrap_or("0");
            if token == "0" {
                return (10, digit_end + 1);
            }
            // Lua `tonumber(token)` 对超长数字得到巨大浮点，永不匹配任何 rank；
            // 溢出时取 u64::MAX，避免退化成“无选择器”。
            return (token.parse::<u64>().unwrap_or(u64::MAX), digit_end + 1);
        }
        _ => {}
    }
    (0, code_end)
}
