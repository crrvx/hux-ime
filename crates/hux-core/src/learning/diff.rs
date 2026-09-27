// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! 路径链差分：按字符边界把「前/后选择」分解为落库事件（`diff`）。

use super::model::{DiffEvent, DiffItem};
use super::text::{context, static_text};
use hashbrown::HashMap;

/// 参照 `M.diff`（`time` 由调用方传入，金样不比对）。
pub fn diff(
    raw: &[u8],
    before: Option<&DiffItem>,
    selected: Option<&DiffItem>,
    floor: usize,
    mode: &str,
    now: f64,
) -> Vec<DiffEvent> {
    let (Some(before), Some(selected)) = (before, selected) else {
        return Vec::new();
    };
    if before.text == selected.text {
        return Vec::new();
    }
    let Some((a, ends)) = boundaries(before, raw) else {
        return Vec::new();
    };
    let Some((b, _)) = boundaries(selected, raw) else {
        return Vec::new();
    };
    let mut result = Vec::new();
    let mut first = 0usize;
    for &last in &ends {
        if let Some(&b_last) = b.get(&last) {
            let b_first = b.get(&first).copied().unwrap_or(0);
            let a_first = a.get(&first).copied().unwrap_or(0);
            let a_last = a.get(&last).copied().unwrap_or(0);
            let text = slice(&selected.text, b_first, b_last);
            if first >= floor && text != slice(&before.text, a_first, a_last) && static_text(&text)
            {
                let code = String::from_utf8_lossy(&raw[first..last]).to_ascii_lowercase();
                result.push(DiffEvent {
                    time: now,
                    mode: mode.to_string(),
                    code,
                    text,
                    context: context(selected.text.get(..b_first).unwrap_or("")),
                    raw_start: first,
                    raw_end: last,
                    text_start: b_first,
                    text_end: b_last,
                });
            }
            first = last;
        }
    }
    result
}

fn slice(text: &str, start: usize, end: usize) -> String {
    text.get(start..end).unwrap_or("").to_string()
}

fn boundaries(item: &DiffItem, raw: &[u8]) -> Option<(HashMap<usize, usize>, Vec<usize>)> {
    let mut map: HashMap<usize, usize> = HashMap::new();
    map.insert(0, 0);
    let mut ends = Vec::new();
    for node in &item.path {
        if node.raw_length > 0 {
            map.insert(node.raw_length, node.text_length);
            ends.push(node.raw_length);
        }
    }
    ends.sort_unstable();
    let (mut r, mut t) = (0usize, 0usize);
    for &last in &ends {
        let mapped = *map.get(&last)?;
        if last <= r || mapped <= t || mapped > item.text.len() {
            return None;
        }
        r = last;
        t = mapped;
    }
    if r != raw.len() || t != item.text.len() {
        return None;
    }
    Some((map, ends))
}
