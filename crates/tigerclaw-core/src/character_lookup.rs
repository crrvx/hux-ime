//! 字查音+虎（⑧-2）：查**光标左侧**汉字的**拼音（上排）与虎码（下排）**。
//!
//! 触发键（默认 `~`）与音查虎同机制：按键被推入组合（本段标签 [`TAG`]），候选含
//! **默认可上屏项**（触发字符，按标点表取半/全角，空格上屏）；上排 auxUp = 光标左侧
//! 最多 [`BEFORE`] 个字的拼音，下排 auxDown = 其虎码；←/→ 以 [`STEP`] 字符步长移动锚点；
//! 不修改应用文本与光标。

use crate::lexicon::Lexicon;
use crate::pinyin_lookup::PinyinIndex;

/// 组合段标签（同音查虎段的 `pinyin_lookup` 对应）。
pub const TAG: &str = "character_lookup";
/// 光标左侧保留的字符数（上排拼音、下排虎码）。
pub const BEFORE: usize = 1;
/// 锚点滚动步长（字符）。
pub const STEP: usize = 1;

/// 默认锚点 = 光标（字符制）。
pub fn default_anchor(cursor_chars: usize) -> usize {
    cursor_chars
}

/// 按 `STEP` 移动锚点（`forward` 为向更后的文本），夹紧到 `[0, text_chars]`。
pub fn scroll(text_chars: usize, anchor: usize, forward: bool) -> usize {
    let anchor = if forward {
        anchor.saturating_add(STEP)
    } else {
        anchor.saturating_sub(STEP)
    };
    anchor.min(text_chars)
}

/// 生成两排提示：`(拼音排, 虎码排)`——内容为光标左侧最多 [`BEFORE`] 个字。
pub fn rows(index: &PinyinIndex, lexicon: &Lexicon, text: &str, anchor: usize) -> (String, String) {
    let anchor = anchor.min(text.chars().count());
    let start = anchor.saturating_sub(BEFORE);
    let mut pinyin_parts: Vec<String> = Vec::new();
    let mut code_parts: Vec<String> = Vec::new();
    for ch in text.chars().skip(start).take(anchor - start) {
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
        pinyin_parts.push(format!("{ch} {reading}"));
        code_parts.push(format!("{ch} {codes}"));
    }
    (pinyin_parts.join("  "), code_parts.join("  "))
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
    fn anchor_scroll_is_char_based() {
        assert_eq!(default_anchor(5), 5);
        assert_eq!(scroll(10, 3, true), 4);
        assert_eq!(scroll(10, 3, false), 2);
        assert_eq!(scroll(10, 9, true), 10); // 夹紧到文本长度
        assert_eq!(scroll(10, 0, false), 0);
    }

    #[test]
    fn rows_show_left_side_pinyin_and_codes() {
        let lexicon = Lexicon::load(
            &[PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../goldens/pinyin_lookup")],
            0,
        );
        let index = index();
        // 夹具 PY_c 无单字「欧」（仅出现在词条里）→ 音为 ?；码表有「欧」→ nbe/nbeq。
        let (pinyin_row, code_row) = rows(&index, &lexicon, "中欧中兴", 2);
        assert_eq!(pinyin_row, "欧 ?");
        assert_eq!(code_row, "欧 nbe/nbeq");
        let (pinyin_row, code_row) = rows(&index, &lexicon, "中欧中兴", 1);
        assert_eq!(pinyin_row, "中 zhong");
        assert_eq!(code_row, "中 d/dg/dgs");
        let (pinyin_row, code_row) = rows(&index, &lexicon, "中欧中兴", 0);
        assert!(pinyin_row.is_empty() && code_row.is_empty());
        // 空白跳过显示、仍占位置（按字符计数）。
        let (pinyin_row, _) = rows(&index, &lexicon, "中 欧兴", 3);
        assert_eq!(pinyin_row, "欧 ?");
        let (pinyin_row, _) = rows(&index, &lexicon, "中 欧兴", 4);
        assert_eq!(pinyin_row, "兴 ?");
        // 码表/词典都缺 → 音码皆 ?。
        let (pinyin_row, code_row) = rows(&index, &lexicon, "龘", 1);
        assert_eq!(pinyin_row, "龘 ?");
        assert_eq!(code_row, "龘 ?");
    }
}
