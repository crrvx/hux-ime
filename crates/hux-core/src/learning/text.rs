// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! 文本与帧工具：UTF-8 校验、字符计数、静态文本判定、上下文截取与帧编解码。

const MAX_FRAME_PART: usize = 8192;

// ---------------------------------------------------------------- 工具函数

/// 严格 UTF-8 序列长度（对齐参照 `chars`/`character_count` 的接受集合）。
fn utf8_len_at(bytes: &[u8], index: usize) -> Option<usize> {
    let a = *bytes.get(index)?;
    if a < 0x80 {
        return Some(1);
    }
    let cont = |offset: usize| {
        bytes
            .get(index + offset)
            .map(|byte| (0x80..=0xBF).contains(byte))
            .unwrap_or(false)
    };
    if (0xC2..=0xDF).contains(&a) && cont(1) {
        return Some(2);
    }
    if (0xE0..=0xEF).contains(&a) && cont(1) && cont(2) {
        let b = bytes[index + 1];
        if !(a == 0xE0 && b < 0xA0) && !(a == 0xED && b >= 0xA0) {
            return Some(3);
        }
        return None;
    }
    if (0xF0..=0xF4).contains(&a) && cont(1) && cont(2) && cont(3) {
        let b = bytes[index + 1];
        if !(a == 0xF0 && b < 0x90) && !(a == 0xF4 && b >= 0x90) {
            return Some(4);
        }
        return None;
    }
    None
}

/// 参照 `chars`：返回字符列表；非法 UTF-8 返回 None。
pub fn chars(text: &str) -> Option<Vec<char>> {
    let bytes = text.as_bytes();
    let mut result = Vec::new();
    let mut index = 0usize;
    while index < bytes.len() {
        let length = utf8_len_at(bytes, index)?;
        result.push(text[index..index + length].chars().next()?);
        index += length;
    }
    Some(result)
}

/// 参照 `character_count`：非法输入返回 0。
pub fn character_count(text: &str) -> usize {
    let bytes = text.as_bytes();
    let mut count = 0usize;
    let mut index = 0usize;
    while index < bytes.len() {
        let Some(length) = utf8_len_at(bytes, index) else {
            return 0;
        };
        index += length;
        count += 1;
    }
    count
}

/// 参照 `static`：1..16 字符，且无控制字符/花括号/私用区序列。
pub fn static_text(text: &str) -> bool {
    let Some(list) = chars(text) else {
        return false;
    };
    if list.is_empty() || list.len() > 16 {
        return false;
    }
    let bytes = text.as_bytes();
    for (index, byte) in bytes.iter().enumerate() {
        if *byte == 0
            || (0x01..=0x1F).contains(byte)
            || *byte == 0x7F
            || *byte == b'{'
            || *byte == b'}'
        {
            return false;
        }
        if *byte == 0xEE
            && index + 2 < bytes.len()
            && (0x80..=0xBF).contains(&bytes[index + 1])
            && (0x80..=0xBF).contains(&bytes[index + 2])
        {
            return false;
        }
        if *byte == 0xEF
            && index + 2 < bytes.len()
            && (0x80..=0xA3).contains(&bytes[index + 1])
            && (0x80..=0xBF).contains(&bytes[index + 2])
        {
            return false;
        }
    }
    true
}

/// 参照 `context`：保留最后 2 个字符（单字符文本保留其自身）。
pub fn context(text: &str) -> String {
    let Some(list) = chars(text) else {
        return String::new();
    };
    if list.is_empty() {
        return String::new();
    }
    let start = list.len().saturating_sub(2);
    list[start..].iter().collect()
}

/// 参照 `key`：`#value:value` 拼接。
pub(super) fn key(parts: &[&str]) -> String {
    let mut out = String::new();
    for part in parts {
        out.push_str(&format!("{}:{}", part.len(), part));
    }
    out
}

/// 参照 `frame`。
pub fn frame(values: &[String]) -> String {
    let refs: Vec<&str> = values.iter().map(String::as_str).collect();
    key(&refs)
}

/// 参照 `unframe`：帧的**长度前缀是字节数**，逐段切出。
///
/// 越界与「落点不在 UTF-8 字符边界」都返回 `None`（参照 Lua 的 `sub` 同样不 panic）。
/// 本函数是坏值的唯一错值通道，而调用方在 `extern "C"` 的构造路径上（读持久化库），
/// 故**不得 panic**：坏帧只能是「跳过该条记录 + 诊断」。
pub fn unframe(value: &str) -> Option<Vec<String>> {
    let bytes = value.as_bytes();
    let mut result = Vec::new();
    let mut position = 0usize;
    while position < bytes.len() {
        let mut digit_end = position;
        while digit_end < bytes.len() && bytes[digit_end].is_ascii_digit() {
            digit_end += 1;
        }
        if digit_end == position || digit_end >= bytes.len() || bytes[digit_end] != b':' {
            return None;
        }
        let length: usize = std::str::from_utf8(&bytes[position..digit_end])
            .ok()?
            .parse()
            .ok()?;
        if length > MAX_FRAME_PART || digit_end + 1 + length > bytes.len() {
            return None;
        }
        // `get(a..b)` 同时兜住「越界」与「非字符边界」——`&value[a..b]` 在后者 panic。
        result.push(
            value
                .get(digit_end + 1..digit_end + 1 + length)?
                .to_string(),
        );
        position = digit_end + 1 + length;
    }
    Some(result)
}
