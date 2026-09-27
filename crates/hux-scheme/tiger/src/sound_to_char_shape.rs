// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! 音反查：`tiger_sentence.pinyin.bin[.gz]`（TCSRV01）读取与音反查翻译。
//!
//! 语义对齐 librime 1.17.0 的词典音反查（`reverse_lookup_translator` + `ReverseLookupFilter`）：
//! - 输入（去掉前缀后）按**拼写表**分段：音节本体 + 缩写（PY_c.schema.yaml 的两条
//!   `abbrev` 规则），缩写可信度罚 `log(0.5)`；
//! - 段内的**音节分隔符**（见 [`SYLLABLE_DELIMITER`]）在匹配拼写键时透明跳过、但**强制**断音：
//!   没有音节（含尾部补全）能跨过它，且它在预编辑里原样保留（含段首、段尾；只有音节边界插空格）；
//! - 输入尾部无法由拼写键消耗时，对剩余部分做**补全**（拼写表子树展开；本体拼写再罚
//!   `log(0.05)`，缩写保持自身罚）；
//! - 分段路径的音节序列必须与词条的码**完全一致**；
//! - 候选次序 = 「可信度 + ln(权重)」降序（权重序取自组内稳定排序），上限
//!   [`crate::decode::CANDIDATE_LIMIT`]（与主候选一致）；
//! - 注释（虎码）由 [`crate::lexicon::code_comment_filter`] 追加以复用现有码注释格式。
//!
//! `code_comment`/`code_comment_filter` 定义于 [`crate::lexicon`]（与主候选共用码注释格式）。
//!
//! 文件布局：本门面保留模块文档、对外常量（文件名/段标签/提示/分隔符）、模式判定、标点候选
//! 与再导出；索引格式与解析在 `sound_to_char_shape/index`，图翻译在 `sound_to_char_shape/graph`。

use hux_core::punct::{PairState, PunctTable};
use hux_core::session::Candidate;

mod graph;
mod index;

pub use graph::translate;
pub use index::{SoundToCharShapeIndex, load_first};

/// 索引文件名（发布为 `.gz`；fixture 常用未压缩）。
pub const SOUND_TO_CHAR_SHAPE_FILE: &str = "tiger_sentence.pinyin.bin";
pub const SOUND_TO_CHAR_SHAPE_FILE_GZ: &str = "tiger_sentence.pinyin.bin.gz";
/// 音反查段标签（参照 schema 的 `reverse_lookup`）。
pub const SOUND_TO_CHAR_SHAPE_TAG: &str = "reverse_lookup";
/// 音反查段提示（参照 schema `reverse_lookup/tips`）。
pub const SOUND_TO_CHAR_SHAPE_TIPS: &str = "〔拼音〕";

/// 音节分隔符（方案 schema 的 `speller/delimiter` 的撇号；段内空格是上屏/选词键，撇号才是
/// 唯一实际入口）：匹配拼写键时透明跳过，但**强制**在该处断音，并在预编辑里原样保留。
/// 判定入口：本文件的 [`repeats_delimiter`]（录入）与 [`matches_pattern`]，以及 `graph` 的 `DelimitedCode`（断音）。
///
/// 与选重后缀表**语义不同、不可合并**：撇号在此是断音，而 `;` / 数字是选重（字节表见
/// [`crate::decode::has_selection_suffix`]，由 beam 侧与交互侧共用）。
pub const SYLLABLE_DELIMITER: u8 = b'\'';

/// 追加 `ch` 是否只是重复音节分隔符：连续撇号只保留第一个，多余的**丢弃、不录入**
/// （输入串与预编辑都不会出现 `''`，也不打断反查段）。
pub(crate) fn repeats_delimiter(input: &[u8], ch: char) -> bool {
    ch == SYLLABLE_DELIMITER as char && input.last() == Some(&SYLLABLE_DELIMITER)
}

/// 裸前缀的标点候选（参照 `PunctTranslator` 与 `CreatePunctCandidate`）；
/// 字反查的「默认可上屏候选」复用同一实现。
pub(crate) fn punct_candidate(
    punct: Option<&PunctTable>,
    pairs: &mut PairState,
    prefix: char,
    full_shape: bool,
    start: usize,
    end: usize,
) -> Option<Candidate> {
    let text = punct?.resolve(prefix, full_shape, pairs)?;
    let comment = punct_shape_comment(&text);
    let mut candidate = Candidate::new("punct", start, end, &text, &comment);
    if end.saturating_sub(start) == 1 {
        candidate.preedit = text;
    }
    Some(candidate)
}

/// 参照 `CreatePunctCandidate` 的形状注释（单个 Unicode 字符时给出〔半角〕/〔全角〕）。
fn punct_shape_comment(punct: &str) -> String {
    let mut chars = punct.chars();
    let Some(ch) = chars.next() else {
        return String::new();
    };
    if chars.next().is_some() {
        return String::new();
    }
    let code = ch as u32;
    let is_ascii = (0x20..0x7f).contains(&code);
    let is_ideographic_space = code == 0x3000;
    let is_full_shape_ascii = (0xff01..=0xff5e).contains(&code);
    let is_kana = (0x30a1..=0x30fc).contains(&code)
        || [0x3001, 0x3002, 0x300c, 0x300d, 0x309b, 0x309c].contains(&code);
    let is_half_shape_kana = (0xff61..=0xff9f).contains(&code);
    let is_hangul = (0x3131..=0x3164).contains(&code);
    let is_half_shape_hangul = (0xffa0..=0xffdc).contains(&code);
    let is_full_shape_narrow_symbol =
        code == 0xff5f || code == 0xff60 || (0xffe0..=0xffe6).contains(&code);
    let is_narrow_symbol = [
        0x00a2, 0x00a3, 0x00a5, 0x00a6, 0x00ac, 0x00af, 0x2985, 0x2986,
    ]
    .contains(&code);
    let is_half_shape_wide_symbol = (0xffe8..=0xffee).contains(&code);
    let is_wide_symbol =
        (0x2190..=0x2193).contains(&code) || code == 0x2502 || code == 0x25a0 || code == 0x25cb;
    let is_half_shape = is_ascii
        || is_half_shape_kana
        || is_half_shape_hangul
        || is_narrow_symbol
        || is_half_shape_wide_symbol;
    let is_full_shape = is_ideographic_space
        || is_full_shape_ascii
        || is_kana
        || is_hangul
        || is_full_shape_narrow_symbol
        || is_wide_symbol;
    if is_half_shape {
        "〔半角〕".to_string()
    } else if is_full_shape {
        "〔全角〕".to_string()
    } else {
        String::new()
    }
}

/// 音反查输入模式：`<前缀>[a-z']*`（参照 schema `recognizer/patterns/reverse_lookup`
/// = `^` + 前缀 + `[a-z']*$`）：撇号可出现在任意位置。
///
/// 口径事实：上游 `92a0b54` 把撇号同时放进 `speller/delimiter`，使反查段内按撇号**切分
/// 音节**（依赖上游 librime 的 delimiter 修复
/// [rime/librime#1233](https://github.com/rime/librime/pull/1233)；参照仓库亦注明该切分需要
/// librime 已含该修复；而本机 librime 1.17.0 未含，故已入库金样里含撇号的反查段**无候选**）。
/// 本仓按方案 schema 的意图实现该切分（见 [`translate`]）：撇号是音节分隔符 —— 匹配拼写键时
/// 透明跳过，但**强制**断音；金样对照见 `key_sequence_differential` 的音反查重放。
/// 撇号在 abc 段一侧的效果见 `interaction::translate::SEGMENTATION_DELIMITER`（追踪反查分支 pin 的 schema）。
pub fn matches_pattern(input: &[u8], prefix: char) -> bool {
    let prefix = prefix as u8;
    let Some(rest) = input.strip_prefix(&[prefix][..]) else {
        return false;
    };
    rest.iter()
        .all(|&byte| byte.is_ascii_lowercase() || byte == SYLLABLE_DELIMITER)
}

#[cfg(test)]
mod tests;
