// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! 字反查：查**光标左侧**汉字的**拼音（上排）与虎码（下排）**。
//!
//! 与音反查同机制：触发键推入组合（本段标签 [`TAG`]）；**仅当触发键为单字符键**时给出
//! 默认可上屏候选（触发字符，按标点表取半/全角，空格上屏）。上排 auxUp = 光标左侧
//! [`BEFORE`] 个字的**拼音**（排头「咅」），下排 auxDown = 其**虎码**（排头「虍」）；
//! ←/→ **交应用处理**（应用光标随动；本层不消费）——判定见 [`navigation_forwarded`]。

use crate::lexicon::Lexicon;
use crate::sound_to_char_shape::SoundToCharShapeIndex;
use hux_core::key::KeyEvent;
use hux_core::session::Context;

/// 组合段标签（同音反查段的 `sound_to_char_shape` 对应）。
pub const TAG: &str = "char_to_sound_shape";
/// 光标左侧保留的字符数（上排拼音、下排虎码）。
pub const BEFORE: usize = 1;

/// 当前组合**末段**是否为字反查段（触发键推入的那段）。
///
/// 该判据的**唯一归属**：宿主查询（aux 排、导航键走向）与交互层（新普通键先清空组合）
/// 都以此为准。
pub fn tagged(context: &Context) -> bool {
    context
        .composition
        .back()
        .is_some_and(|segment| segment.has_tag(TAG))
}

/// 字反查段的**导航键**（←/→/↑/↓ 的非放键）是否交由**应用/宿主**处理：段内它们不参与
/// 选词，本层不消费也不改输入。
///
/// 判定收在本模块，**落点由调用方决定**：`Scheme::process_key` 里直接交平台，而交互层的
/// `Forward` 还会再走宿主链。
pub fn navigation_forwarded(key_event: &KeyEvent, context: &Context) -> bool {
    !key_event.release()
        && tagged(context)
        && matches!(key_event.repr().as_str(), "Left" | "Right" | "Up" | "Down")
}
/// 生成两排提示：`(拼音排, 虎码排)`——内容为光标左侧最多 [`BEFORE`] 个字。
pub fn rows(
    index: &SoundToCharShapeIndex,
    lexicon: &Lexicon,
    text: &str,
    cursor_chars: usize,
) -> (String, String) {
    let cursor = cursor_chars.min(text.chars().count());
    let start = cursor.saturating_sub(BEFORE);
    let mut pinyin_parts: Vec<String> = Vec::new();
    let mut code_parts: Vec<String> = Vec::new();
    for ch in text.chars().skip(start).take(cursor - start) {
        if ch.is_whitespace() {
            continue;
        }
        let reading = {
            let readings = index.character_pinyin(ch);
            if readings.is_empty() {
                "?".to_string()
            } else {
                readings.join("/")
            }
        };
        let codes = {
            let codes = lexicon.character_codes.get(&ch.to_string());
            match codes {
                Some(codes) if !codes.is_empty() => codes.join("/"),
                _ => "?".to_string(),
            }
        };
        pinyin_parts.push(reading);
        code_parts.push(codes);
    }
    (
        format!("咅 {}", pinyin_parts.join("  ")),
        format!("虍 {}", code_parts.join("  ")),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn index() -> SoundToCharShapeIndex {
        let path =
            hux_test_support::repo_path("goldens/sound_to_char_shape/tiger_sentence.pinyin.bin");
        SoundToCharShapeIndex::load(&path).expect("fixture index")
    }

    fn fixture() -> (SoundToCharShapeIndex, Lexicon) {
        let lexicon = Lexicon::load(
            &[hux_test_support::repo_path("goldens/sound_to_char_shape")],
            0,
        );
        (index(), lexicon)
    }

    /// 反查的双排契约：上排拼音、下排虎码，音缺可以只让上排变 ?，码是独立来源。
    #[test]
    fn rows_show_pinyin_and_code_for_left_char() {
        let (index, lexicon) = fixture();
        // 夹具 PY_c 无单字「欧」（仅出现在词条里）→ 音为 ?；码表有「欧」→ nbe/nbeq。
        let (pinyin_row, code_row) = rows(&index, &lexicon, "中欧中兴", 2);
        assert_eq!(pinyin_row, "咅 ?");
        assert_eq!(code_row, "虍 nbe/nbeq");
        let (pinyin_row, code_row) = rows(&index, &lexicon, "中欧中兴", 1);
        assert_eq!(pinyin_row, "咅 zhong");
        assert_eq!(code_row, "虍 d/dg/dgs");
    }

    /// 光标在行首没有左侧字，两排只留排头，不得向右取字充数。
    #[test]
    fn rows_are_empty_at_start() {
        let (index, lexicon) = fixture();
        let (pinyin_row, code_row) = rows(&index, &lexicon, "中欧中兴", 0);
        assert_eq!(pinyin_row, "咅 ");
        assert_eq!(code_row, "虍 ");
    }

    /// 光标与字符位置按字符计数：空白占位但不出现在任何一排。
    #[test]
    fn rows_treat_whitespace_as_position() {
        let (index, lexicon) = fixture();
        // 空白跳过显示、仍占位置（按字符计数）。
        for cursor in [3, 4] {
            let (pinyin_row, _) = rows(&index, &lexicon, "中 欧兴", cursor);
            assert_eq!(pinyin_row, "咅 ?");
        }
    }

    /// 音码双缺也要显式给 ? 占位，避免宿主把空排当成没有反查数据。
    #[test]
    fn rows_use_question_mark_when_data_missing() {
        let (index, lexicon) = fixture();
        // 码表/词典都缺 → 音码皆 ?。
        let (pinyin_row, code_row) = rows(&index, &lexicon, "龘", 1);
        assert_eq!(pinyin_row, "咅 ?");
        assert_eq!(code_row, "虍 ?");
    }
}
