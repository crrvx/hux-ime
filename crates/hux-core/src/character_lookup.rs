//! 字查音+虎（⑧-2）：查**光标左侧**汉字的**拼音（上排）与虎码（下排）**。
//!
//! 与音查虎同机制：触发键推入组合（本段标签 [`TAG`]）；**仅当触发键为单字符键**时给出
//! 默认可上屏候选（触发字符，按标点表取半/全角，空格上屏）。上排 auxUp = 光标左侧
//! [`BEFORE`] 个字的**拼音**（排头「咅」），下排 auxDown = 其**虎码**（排头「虍」）；
//! ←/→ **交应用处理**（应用光标随动；本层不消费）。

use crate::lexicon::Lexicon;
use crate::pinyin_lookup::PinyinIndex;

/// 组合段标签（同音查虎段的 `pinyin_lookup` 对应）。
pub const TAG: &str = "character_lookup";
/// 光标左侧保留的字符数（上排拼音、下排虎码）。
pub const BEFORE: usize = 1;
/// 生成两排提示：`(拼音排, 虎码排)`——内容为光标左侧最多 [`BEFORE`] 个字。
pub fn rows(
    index: &PinyinIndex,
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
    use std::path::PathBuf;

    fn index() -> PinyinIndex {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../goldens/pinyin_lookup/tiger_sentence.pinyin.bin");
        PinyinIndex::load(&path).expect("fixture index")
    }

    #[test]
    fn rows_show_left_char_with_head_marks() {
        let lexicon = Lexicon::load(
            &[PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../goldens/pinyin_lookup")],
            0,
        );
        let index = index();
        // 夹具 PY_c 无单字「欧」（仅出现在词条里）→ 音为 ?；码表有「欧」→ nbe/nbeq。
        let (pinyin_row, code_row) = rows(&index, &lexicon, "中欧中兴", 2);
        assert_eq!(pinyin_row, "咅 ?");
        assert_eq!(code_row, "虍 nbe/nbeq");
        let (pinyin_row, code_row) = rows(&index, &lexicon, "中欧中兴", 1);
        assert_eq!(pinyin_row, "咅 zhong");
        assert_eq!(code_row, "虍 d/dg/dgs");
        let (pinyin_row, code_row) = rows(&index, &lexicon, "中欧中兴", 0);
        assert_eq!(pinyin_row, "咅 ");
        assert_eq!(code_row, "虍 ");
        // 空白跳过显示、仍占位置（按字符计数）。
        let (pinyin_row, _) = rows(&index, &lexicon, "中 欧兴", 3);
        assert_eq!(pinyin_row, "咅 ?");
        let (pinyin_row, _) = rows(&index, &lexicon, "中 欧兴", 4);
        assert_eq!(pinyin_row, "咅 ?");
        // 码表/词典都缺 → 音码皆 ?。
        let (pinyin_row, code_row) = rows(&index, &lexicon, "龘", 1);
        assert_eq!(pinyin_row, "咅 ?");
        assert_eq!(code_row, "虍 ?");
    }
}
