// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! 哈希与融合键：双累加器 FNV 变体（`hash`/`hash_bytes`/`hash_parts`）与 `fusion_*` 事件。

use super::model::DiffEvent;

/// 双累加器 FNV 变体的初值（`a` / `b`）。
const HASH_SEED: (u64, u64) = (2166136261, 5381);

/// 把一段字节累加进两累加器（[`hash_bytes`] / [`hash_parts`] 的唯一实现处）。
fn hash_accumulate(state: &mut (u64, u64), text: &[u8]) {
    for byte in text {
        state.0 = (state.0 * 65599 + u64::from(*byte)) % 4294967296;
        state.1 = (state.1 * 33 + u64::from(*byte)) % 4294967296;
    }
}

/// 双累加器 FNV 变体的输出格式（`%08x%08x`）。
fn hash_hex(state: (u64, u64)) -> String {
    format!("{:08x}{:08x}", state.0, state.1)
}

/// 参照 `M.hash`：双累加器 FNV 变体，输出 `%08x%08x`（按字节，允许非 UTF-8 输入）。
pub fn hash_bytes(text: &[u8]) -> String {
    let mut state = HASH_SEED;
    hash_accumulate(&mut state, text);
    hash_hex(state)
}

/// 参照 `M.hash`。
pub fn hash(text: &str) -> String {
    hash_bytes(text.as_bytes())
}

/// 多段文本按序拼接后的哈希（`hash_parts(&["ab", "c"]) == hash("abc")`）。
///
/// 调用方按段持有内容时（例如码表逐表装载）不必为了求哈希先把各段拼成一个大串。
pub fn hash_parts(parts: &[&str]) -> String {
    let mut state = HASH_SEED;
    for part in parts {
        hash_accumulate(&mut state, part.as_bytes());
    }
    hash_hex(state)
}

/// 参照 `M.fusion_mode`：空模式串保持空（= 不学习），否则加 `fusion-v1|` 前缀。
pub fn fusion_mode(mode: &str) -> String {
    if mode.is_empty() {
        String::new()
    } else {
        format!("fusion-v1|{mode}")
    }
}

/// 参照 `M.fusion_pair_code`：成对偏好记录的键。
///
/// 原始输入按 `raw .. "\0D\0" .. direct .. "\0C\0" .. composed` 逐字节拼接后哈希；
/// `~f` 前缀把融合码移出 raw 码的前缀索引命名空间（虎码 raw 只含 `a-z`，而 `~` > `z`）。
pub fn fusion_pair_code(raw: &[u8], direct: &str, composed: &str) -> String {
    let mut framed = Vec::with_capacity(raw.len() + direct.len() + composed.len() + 6);
    framed.extend_from_slice(raw);
    framed.extend_from_slice(b"\0D\0");
    framed.extend_from_slice(direct.as_bytes());
    framed.extend_from_slice(b"\0C\0");
    framed.extend_from_slice(composed.as_bytes());
    format!("~f{}", hash_bytes(&framed))
}

/// 参照 `M.fusion_event`：成对偏好事件的构造（`time` 由调用方注入，参照取 `os.time()`）。
///
/// `direct_wins` 决定 `text` 取 `"D"`（Direct 胜）还是 `"C"`；`context` 恒为空串
/// （只命中 `exact[""]`）；`raw_end` 只用于提交点筛选，不落库。`mode == ""` → `None`。
pub fn fusion_event(
    mode: &str,
    raw: &[u8],
    direct: &str,
    composed: &str,
    direct_wins: bool,
    raw_end: usize,
    now: f64,
) -> Option<DiffEvent> {
    if mode.is_empty() {
        return None;
    }
    Some(DiffEvent {
        time: now,
        mode: fusion_mode(mode),
        code: fusion_pair_code(raw, direct, composed),
        text: if direct_wins { "D" } else { "C" }.to_string(),
        context: String::new(),
        raw_start: 0,
        raw_end,
        text_start: 0,
        text_end: 1,
    })
}
