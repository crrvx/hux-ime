// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! 逐段翻译：分段类型判定、分支派发与 [`translate_segments`] 主体。
//!
//! 由父模块 `translate` 按原名与可见性重导出。

use super::*;

/// 未翻译段的类型：触发字符段 / 裸前缀段 / 默认段。
enum SegmentKind {
    /// 触发字符段（`char_to_sound_shape`）。
    CharToSoundShape,
    /// 裸前缀段（`sound_to_char_shape`）。
    SoundToCharShape,
    /// 默认段：交组合翻译。
    Default,
}

/// 参照 `ConcreteEngine::TranslateSegments`：仅翻译未建立菜单的段。
pub(crate) fn translate_segments(
    decoder: &mut Decoder,
    context: &mut Context,
    state: &SentenceState,
    input: &[u8],
    punct: Option<&PunctTable>,
) -> anyhow::Result<()> {
    let prefixes = sound_to_char_shape_prefixes(context);
    for index in 0..context.composition.segments.len() {
        let (kind, start, end) = {
            let segment = &context.composition.segments[index];
            if segment.translated || segment.selected {
                continue;
            }
            let (start, end) = (segment.start.min(input.len()), segment.end.min(input.len()));
            if start >= end {
                let segment = &mut context.composition.segments[index];
                segment.translated = true;
                segment.candidates.clear();
                segment.selected_index = 0;
                continue;
            }
            let kind = if segment.has_tag(char_to_sound_shape::TAG) {
                SegmentKind::CharToSoundShape
            } else if segment.has_tag(sound_to_char_shape::SOUND_TO_CHAR_SHAPE_TAG) {
                SegmentKind::SoundToCharShape
            } else {
                SegmentKind::Default
            };
            (kind, start, end)
        };
        match kind {
            SegmentKind::CharToSoundShape => {
                translate_char_to_sound_shape_segment(context, input, (start, end), punct, index);
            }
            SegmentKind::SoundToCharShape => {
                translate_sound_to_char_shape_segment(
                    decoder,
                    context,
                    input,
                    (start, end),
                    punct,
                    &prefixes,
                    index,
                );
            }
            SegmentKind::Default => {
                translate_default_segment(decoder, context, state, input, (start, end), index)?;
            }
        }
    }
    Ok(())
}

/// 触发字符段：按触发字符给出默认可上屏候选。
fn translate_char_to_sound_shape_segment(
    context: &mut Context,
    input: &[u8],
    span: (usize, usize),
    punct: Option<&PunctTable>,
    index: usize,
) {
    let (start, end) = span;
    let full_shape = context.get_option("full_shape");
    // 默认可上屏候选：仅当触发字符来自**单字符键**（无 Ctrl/Alt/Super）时提供。
    let pressed = single_char(&input[start..end]);
    let candidates = match pressed.filter(|character| {
        char_to_sound_shape_triggers(context)
            .iter()
            .any(|key| single_char_trigger(key) == Some(*character))
    }) {
        Some(character) => sound_to_char_shape::punct_candidate(
            punct,
            context.punct_pairs(),
            character,
            full_shape,
            start,
            end,
        )
        .into_iter()
        .collect(),
        None => Vec::new(),
    };
    let segment = &mut context.composition.segments[index];
    segment.translated = true;
    segment.selected_index = 0;
    segment.candidates = candidates;
}

/// 裸前缀 + 触发字符：给出默认可上屏候选。
fn push_bare_prefix_punct_candidate(
    context: &mut Context,
    punct: Option<&PunctTable>,
    span: (usize, usize),
    pressed: Option<char>,
    index: usize,
) {
    let (start, end) = span;
    let full_shape = context.get_option("full_shape");
    let candidates = match pressed.filter(|character| {
        sound_to_char_shape_triggers(context)
            .iter()
            .any(|key| single_char_trigger(key) == Some(*character))
    }) {
        Some(character) => sound_to_char_shape::punct_candidate(
            punct,
            context.punct_pairs(),
            character,
            full_shape,
            start,
            end,
        )
        .into_iter()
        .collect(),
        None => Vec::new(),
    };
    let segment = &mut context.composition.segments[index];
    segment.translated = true;
    segment.selected_index = 0;
    segment.candidates = candidates;
}

/// 裸前缀段（无编码）：先试触发字符候选，否则按前缀匹配产出编码候选并提示输入。
fn translate_sound_to_char_shape_segment(
    decoder: &mut Decoder,
    context: &mut Context,
    input: &[u8],
    span: (usize, usize),
    punct: Option<&PunctTable>,
    prefixes: &[char],
    index: usize,
) {
    let (start, end) = span;
    let full_shape = context.get_option("full_shape");
    // 裸前缀（无编码）：默认可上屏候选**仅当触发字符来自单字符键**时提供；带修饰键无候选。
    let pressed = single_char(&input[start..end]);
    if pressed.is_some_and(|character| prefixes.contains(&character)) {
        push_bare_prefix_punct_candidate(context, punct, span, pressed, index);
        return;
    }
    let slice = input[start..end].to_vec();
    let candidates = match prefixes
        .iter()
        .find(|prefix| sound_to_char_shape::matches_pattern(&slice, **prefix))
    {
        Some(prefix) => decoder.sound_to_char_shape_candidates(
            &slice,
            *prefix,
            start,
            end,
            punct,
            context.punct_pairs(),
            full_shape,
        ),
        None => Vec::new(),
    };
    let segment = &mut context.composition.segments[index];
    segment.translated = true;
    segment.selected_index = 0;
    segment.prompt = if prefixes
        .iter()
        .any(|prefix| slice.first() == Some(&(*prefix as u8)))
    {
        sound_to_char_shape::SOUND_TO_CHAR_SHAPE_TIPS.to_string()
    } else {
        String::new()
    };
    segment.candidates = candidates;
}

/// 默认段：交组合翻译产出候选并写回段。
fn translate_default_segment(
    decoder: &mut Decoder,
    context: &mut Context,
    state: &SentenceState,
    input: &[u8],
    span: (usize, usize),
    index: usize,
) -> anyhow::Result<()> {
    let (start, end) = span;
    let mut candidates = Vec::new();
    translate_composition(
        decoder,
        context,
        state,
        &input[start..end],
        start,
        end,
        &mut candidates,
    )?;
    let segment = &mut context.composition.segments[index];
    segment.translated = true;
    segment.selected_index = 0;
    segment.candidates = candidates;
    Ok(())
}
