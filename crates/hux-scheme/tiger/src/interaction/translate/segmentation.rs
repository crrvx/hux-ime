// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! 分段器与段操作：公共前缀、分段计算、matcher 与 abc/fallback 分段器、段追加。
//!
//! 由父模块 `translate` 按原名与可见性重导出。

use super::*;

/// 公共前缀字节长度。
pub(crate) fn common_prefix_length(left: &[u8], right: &[u8]) -> usize {
    left.iter()
        .zip(right.iter())
        .take_while(|(a, b)| a == b)
        .count()
}

/// 参照 `ConcreteEngine::CalculateSegmentation`。
pub(crate) fn calculate_segmentation(
    composition: &mut Composition,
    input: &[u8],
    caret: usize,
    prefixes: &[char],
    characters: &[char],
) {
    while !composition.has_finished_segmentation(input) {
        let start = composition.current_start_position();
        // 参照 segmentors 顺序：matcher → abc_segmentor → punct_segmentor → fallback。
        matcher(composition, input, prefixes, characters);
        abc_segmentor(composition, input);
        fallback_segmentor(composition, input);
        if start == composition.current_end_position() {
            break; // 无进展
        }
        if start >= caret {
            break; // 只允许 caret 之后一段
        }
        if !composition.has_finished_segmentation(input) {
            composition.forward();
        }
    }
    // 只在已确认组合末尾追加空段。
    composition.trim();
    if composition
        .back()
        .map(|segment| segment.selected)
        .unwrap_or(false)
    {
        composition.forward();
    }
}

/// 参照 `Matcher::Proceed`（`recognizer/patterns`）：活跃输入匹配
/// `^<前缀>[a-z']*$` 时，由本段独占剩余输入（标签 [`sound_to_char_shape::SOUND_TO_CHAR_SHAPE_TAG`]）。
pub(crate) fn matcher(
    composition: &mut Composition,
    input: &[u8],
    prefixes: &[char],
    characters: &[char],
) {
    let start = composition.confirmed_position();
    let Some(active) = input.get(start..) else {
        return;
    };
    // 字反查：活跃输入恰为一个触发字符（单字符段）。
    for character in characters {
        let mut buffer = [0u8; 4];
        if active == character.encode_utf8(&mut buffer).as_bytes() {
            while composition.current_start_position() > start {
                composition.segments.pop();
            }
            add_segment(composition, start, input.len(), &[char_to_sound_shape::TAG]);
            return;
        }
    }
    if prefixes.is_empty() {
        return;
    }
    if !prefixes
        .iter()
        .any(|prefix| sound_to_char_shape::matches_pattern(active, *prefix))
    {
        return;
    }
    // 参照 `GetMatch`：命中段必须覆盖到输入末尾；起点为当前末尾或既有段起点。
    if start != composition.current_end_position()
        && !composition
            .segments
            .iter()
            .any(|segment| segment.start == start)
    {
        return;
    }
    while composition.current_start_position() > start {
        composition.segments.pop();
    }
    add_segment(
        composition,
        start,
        input.len(),
        &[sound_to_char_shape::SOUND_TO_CHAR_SHAPE_TAG],
    );
}

/// 参照 `AbcSegmentor::Proceed`：从当前位置取最长合法拼写段。
pub(crate) fn abc_segmentor(composition: &mut Composition, input: &[u8]) {
    let start = composition.current_start_position();
    let mut end = start;
    let mut expecting_an_initial = true;
    while end < input.len() {
        let byte = input[end] as char;
        let is_letter = SEGMENTATION_ALPHABET.contains(byte);
        let is_delimiter = end != 0 && SEGMENTATION_DELIMITER.contains(byte);
        if !is_letter && !is_delimiter {
            break;
        }
        let is_initial = SEGMENTATION_INITIALS.contains(byte);
        let is_final = false; // schema 未设置 `speller/finals`
        if expecting_an_initial && !is_initial && !is_delimiter {
            break;
        }
        expecting_an_initial = is_final || is_delimiter;
        end += 1;
    }
    if start < end {
        add_segment(composition, start, end, &["abc"]);
    }
}

/// 参照 `FallbackSegmentor::Proceed`：无可拼写时生成（或延长）raw 段。
pub(crate) fn fallback_segmentor(composition: &mut Composition, input: &[u8]) {
    if composition.current_end_position() != composition.current_start_position() {
        return; // 本轮已有段
    }
    let k = composition.current_start_position();
    if k == input.len() {
        return;
    }
    composition.trim();
    if let Some(last) = composition.back_mut()
        && last.has_tag("raw")
    {
        last.end = k + 1;
        last.candidates.clear();
        last.selected_index = 0;
        last.translated = false;
        return;
    }
    composition.forward();
    add_segment(composition, k, k + 1, &["raw"]);
}

/// 参照 `Segmentation::AddSegment`：同起点段按长度取胜/覆盖/合并标签。
pub(crate) fn add_segment(composition: &mut Composition, start: usize, end: usize, tags: &[&str]) {
    if start != composition.current_start_position() {
        return;
    }
    if composition.segments.is_empty() {
        composition.segments.push(Segment {
            start,
            end,
            tags: tags.iter().map(|tag| tag.to_string()).collect(),
            ..Segment::default()
        });
        return;
    }
    let last = composition.segments.last_mut().expect("segment");
    if last.end > end {
        // 保留较长的旧段
    } else if last.end < end {
        *last = Segment {
            start,
            end,
            tags: tags.iter().map(|tag| tag.to_string()).collect(),
            ..Segment::default()
        };
    } else {
        for tag in tags {
            if !last.has_tag(tag) {
                last.tags.push(tag.to_string());
            }
        }
    }
}
