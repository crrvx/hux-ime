// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;

/// 缓冲前缀属性（**唯一**仍写出的会话属性：`select` / `early_commit` / `learning_glue`
/// 与内核视图都读它；其余会话状态改由方案侧 `SentenceState` 承载，见 `state.rs` 的 `save`）。
pub const K_BUFFERED: &str = "tiger_sentence_buffered_text";
/// 音反查触发键（内部属性：宿主按设置写入逗号分隔的 rime 键名；空/缺省 = 关闭）。
pub const K_SOUND_TO_CHAR_SHAPE_KEY: &str = "_sound_to_char_shape_key";
/// 字反查触发键（内部属性：宿主按设置写入逗号分隔的 rime 键名；空/缺省 = 关闭）。
pub const K_CHAR_TO_SOUND_SHAPE_KEY: &str = "_char_to_sound_shape_key";

/// 音反查触发键列表（解析 [`K_SOUND_TO_CHAR_SHAPE_KEY`]；空/非法项忽略）。
pub fn sound_to_char_shape_triggers(context: &Context) -> Vec<KeyEvent> {
    trigger_keys(context, K_SOUND_TO_CHAR_SHAPE_KEY)
}

/// 字反查触发键列表（解析 [`K_CHAR_TO_SOUND_SHAPE_KEY`]；空/非法项忽略）。
pub fn char_to_sound_shape_triggers(context: &Context) -> Vec<KeyEvent> {
    trigger_keys(context, K_CHAR_TO_SOUND_SHAPE_KEY)
}

/// 音反查组合前缀字符集合（= 各触发键产生的字符，去重保序）。
pub fn sound_to_char_shape_prefixes(context: &Context) -> Vec<char> {
    trigger_chars(&sound_to_char_shape_triggers(context))
}

/// 字反查组合触发字符集合（= 各触发键产生的字符，去重保序）。
pub fn char_to_sound_shape_keys(context: &Context) -> Vec<char> {
    trigger_chars(&char_to_sound_shape_triggers(context))
}

/// 解析属性中的 rime 键名列表（逗号分隔；空/非法项忽略）。
pub fn trigger_keys(context: &Context, property: &str) -> Vec<KeyEvent> {
    context
        .get_property(property)
        .unwrap_or("")
        .split(',')
        .filter_map(KeyEvent::from_repr)
        .collect()
}

pub(crate) fn trigger_chars(keys: &[KeyEvent]) -> Vec<char> {
    let mut chars = Vec::new();
    for key in keys {
        if let Some(ch) = key_char(key)
            && !chars.contains(&ch)
        {
            chars.push(ch);
        }
    }
    chars
}

/// 按键「实际产生的字符」（字符归一；供触发键匹配与单字符判定）。
/// 兼容前端上报 `grave+Shift` 或 `asciitilde`（US 布局的 `~`）。
pub fn key_char(key_event: &KeyEvent) -> Option<char> {
    let code = key_event.keycode;
    if code == 0x60 {
        return Some(if key_event.shift() { '~' } else { '`' });
    }
    if code == 0x7e {
        return Some('~');
    }
    if code > 0x20 && code < 0x7f {
        char::from_u32(code as u32)
    } else {
        None
    }
}

/// 输入恰为单个字符时取其字符。
pub(crate) fn single_char(input: &[u8]) -> Option<char> {
    let text = std::str::from_utf8(input).ok()?;
    let mut chars = text.chars();
    let ch = chars.next()?;
    chars.next().is_none().then_some(ch)
}

/// 触发键命中：修饰位（Ctrl/Alt/Super）一致且字符归一后相同（Shift 交由字符归一）。
pub fn key_matches(key_event: &KeyEvent, configured: &KeyEvent) -> bool {
    let want_modifiers = configured.modifier & (K_CONTROL_MASK | K_ALT_MASK | K_SUPER_MASK);
    let have_modifiers = key_event.modifier & (K_CONTROL_MASK | K_ALT_MASK | K_SUPER_MASK);
    want_modifiers == have_modifiers
        && key_char(key_event).is_some()
        && key_char(key_event) == key_char(configured)
}

/// 单字符触发键（无 Ctrl/Alt/Super）产生的字符；带修饰时返回 None
/// ——「只有单字符快捷键才提供默认可上屏候选」。
pub fn single_char_trigger(key: &KeyEvent) -> Option<char> {
    if key.ctrl() || key.alt() || key.super_modifier() {
        return None;
    }
    key_char(key)
}

/// 参照 `is_modifier_repr`：独立的修饰键事件（不消耗小数点待发状态）。
pub fn is_modifier_repr(repr: &str) -> bool {
    repr.starts_with("Shift")
        || repr.starts_with("Control")
        || repr.starts_with("Alt")
        || repr.starts_with("Super")
        || repr.starts_with("Meta")
        || repr == "Caps_Lock"
        || repr == "Num_Lock"
        || repr.starts_with("ISO_Level")
        || repr == "Mode_switch"
}

/// 参照 `is_plain_char_key`：只接受无 Ctrl/Alt/Super 的字符输入。
pub fn is_plain_char_key(key_event: &KeyEvent, repr: &str) -> Option<char> {
    if key_event.ctrl() || key_event.alt() || key_event.super_modifier() {
        return None;
    }
    if repr.len() == 1
        && let Some(ch) = repr.chars().next()
        && ch.is_ascii_lowercase()
    {
        return Some(ch);
    }
    match repr {
        "semicolon" => return Some(';'),
        "apostrophe" => return Some('\''),
        _ => {}
    }
    if repr.len() == 1
        && let Some(ch) = repr.chars().next()
        && ch.is_ascii_digit()
    {
        return Some(ch);
    }
    if let Some(digit) = repr.strip_prefix("KP_")
        && digit.len() == 1
        && let Some(ch) = digit.chars().next()
        && ch.is_ascii_digit()
    {
        return Some(ch);
    }
    None
}

/// 参照 `Recognizer::ProcessKeyEvent`：可被音反查模式接受的字符（`ch > 0x20 && ch < 0x80`，
/// 排除 Ctrl/Alt/Super；空格由 `use_space=false` 排除）。
pub(crate) fn recognizer_char(key_event: &KeyEvent) -> Option<char> {
    if key_event.ctrl() || key_event.alt() || key_event.super_modifier() {
        return None;
    }
    let code = key_event.keycode;
    if code > 0x20 && code < 0x7f {
        char::from_u32(code as u32)
    } else {
        None
    }
}
