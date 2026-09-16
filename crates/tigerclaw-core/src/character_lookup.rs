//! 字查音+虎（⑧-2）：读取周边文本窗口，生成每字「音·虎码」提示。
//!
//! 契约（用户裁决）：显示**光标前 2 + 后 2 个字符**（窗口 4 字、光标居中），
//! ←/→ 以 **2 字符**为步长滚动窗口；仅提示、不上屏；不改应用文本与光标。
//! 提示形如 `中 zhong·d/dg/dgs`（音多读、码多码以 `/` 连接；缺数据为 `?`），
//! 字符之间两空格分隔；空白字符跳过显示（仍占窗口位置）。

use crate::lexicon::Lexicon;
use crate::pinyin_lookup::PinyinIndex;

/// 光标前保留的字符数。
pub const BEFORE: usize = 2;
/// 光标后保留的字符数。
pub const AFTER: usize = 2;
/// 滚动步长（字符）。
pub const STEP: usize = 2;

/// 环绕光标（字符制）的默认窗口起点（`光标 - BEFORE`，夹紧到 0）。
pub fn default_start(cursor_chars: usize) -> usize {
    cursor_chars.saturating_sub(BEFORE)
}

/// 按 `STEP` 滚动窗口起点（`forward` 为向更后的文本），夹紧到 `[0, len]`。
pub fn scroll(text_chars: usize, start: usize, forward: bool) -> usize {
    let start = if forward {
        start.saturating_add(STEP)
    } else {
        start.saturating_sub(STEP)
    };
    start.min(text_chars)
}

/// 窗口内的字符（`start` 起最多 [`BEFORE`] + [`AFTER`] 个字符）。
pub fn window_chars<'a>(text: &'a str, start: usize) -> impl Iterator<Item = char> + 'a {
    text.chars().skip(start).take(BEFORE + AFTER)
}

/// 生成提示文本（窗口内每字「音·虎码」；空白跳过）。
pub fn hint(index: &PinyinIndex, lexicon: &Lexicon, text: &str, start: usize) -> String {
    let mut parts: Vec<String> = Vec::new();
    for ch in window_chars(text, start) {
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
        parts.push(format!("{ch} {reading}·{codes}"));
    }
    parts.join("  ")
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
    fn window_and_scroll_are_char_based() {
        assert_eq!(default_start(0), 0);
        assert_eq!(default_start(1), 0);
        assert_eq!(default_start(2), 0);
        assert_eq!(default_start(5), 3);
        assert_eq!(scroll(10, 3, true), 5);
        assert_eq!(scroll(10, 3, false), 1);
        assert_eq!(scroll(10, 9, true), 10); // 夹紧到文本长度
        assert_eq!(scroll(10, 0, false), 0);
        assert_eq!(
            window_chars("甲乙丙丁戊己", 1).collect::<String>(),
            "乙丙丁戊"
        );
    }

    #[test]
    fn hint_formats_reading_and_codes() {
        // 夹具码表：中 = d/dg/dgs；PY_c：中 = zhong。
        let lexicon = Lexicon::load(
            &[PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../goldens/pinyin_lookup")],
            0,
        );
        let index = index();
        // 夹具 PY_c 无单字「欧」（仅出现在词条里）→ 音为 ?；码表有「欧」→ 码为 nbe/nbeq。
        let text = "中欧";
        assert_eq!(
            hint(&index, &lexicon, text, 0),
            "中 zhong·d/dg/dgs  欧 ?·nbe/nbeq"
        );
        // 空白跳过显示，仍占窗口位置；码表/词典都缺 → 音码皆 ?。
        assert_eq!(
            hint(&index, &lexicon, "中 欧", 0),
            "中 zhong·d/dg/dgs  欧 ?·nbe/nbeq"
        );
        assert_eq!(
            hint(&index, &lexicon, "龘中", 0),
            "龘 ?·?  中 zhong·d/dg/dgs"
        );
        assert_eq!(hint(&index, &lexicon, "", 0), "");
    }
}
