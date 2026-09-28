// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! 反查触发键与 `recognizer` 链（音反查段内继续接受模式内按键）。

use super::super::*;

/// 触发键（音反查 / 字反查）：空闲时进入组合、段内再按则退出（同参照的标签语义）。
/// 命中即返回 `Some`；未命中返回 `None`，交由后续处理器。
pub(super) fn handle_reverse_lookup_triggers(
    key_event: &KeyEvent,
    context: &mut Context,
) -> Option<ProcessorResult> {
    for (triggers, tag) in [
        (
            sound_to_char_shape_triggers(context),
            sound_to_char_shape::SOUND_TO_CHAR_SHAPE_TAG,
        ),
        (
            char_to_sound_shape_triggers(context),
            char_to_sound_shape::TAG,
        ),
    ] {
        let Some(configured) = triggers
            .iter()
            .find(|configured| key_matches(key_event, configured))
        else {
            continue;
        };
        // 触发字符取命中键实际产生的字符（多触发键各自字符可不同）。
        let Some(prefix) = key_char(configured) else {
            continue;
        };
        let active = context
            .composition
            .back()
            .is_some_and(|segment| segment.has_tag(tag));
        if active {
            context.clear();
            return Some(ProcessorResult::Consume);
        }
        if !context.is_composing() {
            let mut buffer = [0u8; 4];
            context.push_input(prefix.encode_utf8(&mut buffer).as_bytes());
            return Some(ProcessorResult::Consume);
        }
        // 组合中：交由后续处理器（标点等）处理。
    }
    None
}

/// 参照处理器链 `recognizer`（位于 speller/标点之前）：音反查段内继续接受模式内按键。
/// 消费该键时返回 `true`。
pub(super) fn handle_recognizer(key_event: &KeyEvent, context: &mut Context) -> bool {
    if context
        .composition
        .back()
        .is_some_and(|segment| segment.has_tag(sound_to_char_shape::SOUND_TO_CHAR_SHAPE_TAG))
        && let Some(ch) = recognizer_char(key_event)
    {
        let prefixes = sound_to_char_shape_prefixes(context);
        let mut next = context.input().to_vec();
        next.push(ch as u8);
        if prefixes
            .iter()
            .any(|prefix| sound_to_char_shape::matches_pattern(&next, *prefix))
        {
            // 连续的音节分隔符只保留第一个：判定与语义都在音反查模块。
            if sound_to_char_shape::repeats_delimiter(context.input(), ch) {
                return true;
            }
            context.push_input(&[ch as u8]);
            return true;
        }
    }
    false
}
