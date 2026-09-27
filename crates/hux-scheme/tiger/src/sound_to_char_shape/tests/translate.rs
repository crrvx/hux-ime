// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! 音 → 字反查与分隔符行为的用例。

use super::*;

#[test]
fn translate_matches_abbrev_and_pruning_candidates() {
    let index = fixture_index();
    let lexicon = Lexicon::load(&[], 0);
    let texts = |input: &[u8]| -> Vec<String> {
        translate(
            &index,
            &lexicon,
            input,
            '`',
            0,
            input.len(),
            None,
            &mut PairState::default(),
            false,
            crate::decode::CANDIDATE_LIMIT,
        )
        .into_iter()
        .map(|candidate| candidate.text)
        .collect()
    };
    // 缩写路径（「zh」+「o」）与剪枝（zhou 全拼可达 → 缩写全弃）。
    assert_eq!(
        texts(b"`zho"),
        ["中哦", "中龘", "中欧", "找哦", "兆欧", "找欧"]
    );
    assert_eq!(texts(b"`zhou"), ["周", "轴"]);
    assert_eq!(texts(b"`zhong"), ["中", "重", "种", "钟", "垚"]);
    assert!(texts(b"`zhon").is_empty());
    assert!(texts(b"`zuo").is_empty());
}

#[test]
fn translate_segments_preedit_by_syllable() {
    let index = fixture_index();
    let lexicon = Lexicon::load(&[], 0);
    let preedits = |input: &[u8]| -> Vec<String> {
        translate(
            &index,
            &lexicon,
            input,
            '`',
            0,
            input.len(),
            None,
            &mut PairState::default(),
            false,
            crate::decode::CANDIDATE_LIMIT,
        )
        .into_iter()
        .map(|candidate| candidate.preedit)
        .collect()
    };
    // 预编辑「按音节分码」：全拼段之后插空格；缩写段与后续合并。
    assert_eq!(preedits(b"`zhong")[0], "`zhong");
    assert_eq!(preedits(b"`zhongguo")[0], "`zhong guo");
    assert_eq!(preedits(b"`zhongg")[0], "`zhong g");
    assert_eq!(preedits(b"`zho")[0], "`zho");
}

/// 音节分隔符在匹配拼写键时透明跳过，但**强制**断音：音节与尾部补全都不得跨段。
#[test]
fn translate_honors_syllable_delimiter() {
    let index = delimiter_index();
    // 无分隔符：`xian` 同时可达 [xian]（先）与 [xi][an]（西安）。
    assert_eq!(candidate_texts(&index, b"`xian"), ["先", "西安"]);
    // 强制断音：只剩 [xi][an]（西安），跨段的 [xian]（先）被剔除。
    assert_eq!(candidate_texts(&index, b"`xi'an"), ["西安"]);
    // 末尾分隔符等价于无分隔符；首部、连续分隔符等价于单个分隔符。
    assert_eq!(candidate_texts(&index, b"`xi"), ["西"]);
    assert_eq!(
        candidate_texts(&index, b"`xi'"),
        candidate_texts(&index, b"`xi")
    );
    assert_eq!(
        candidate_texts(&index, b"`'xi'an"),
        candidate_texts(&index, b"`xi'an")
    );
    assert_eq!(
        candidate_texts(&index, b"`xi'an'"),
        candidate_texts(&index, b"`xi'an")
    );
    assert_eq!(
        candidate_texts(&index, b"`xi''an"),
        candidate_texts(&index, b"`xi'an")
    );
    // 尾部补全不得跨段：`xia'n` 的 `an` 跨过末尾分隔符 ⇒ 无候选
    //（补全若跨段，[xi] + 补全 `an` 就会错出「西安」）。
    assert!(candidate_texts(&index, b"`xia'n").is_empty());
    // 某段拼不出音节 ⇒ 整段无候选（不报错、不 panic）。
    assert!(candidate_texts(&index, b"`xi'qan").is_empty());
    assert!(candidate_texts(&index, b"`zhq'guo").is_empty());
    // 裸分隔符（无音节）同样只是无候选。
    assert!(candidate_texts(&index, b"`'").is_empty());
    assert!(candidate_texts(&index, b"`''").is_empty());
}

/// 分隔符在预编辑里**原样保留为撇号**（与输入同形），即便前一音节是缩写/补全匹配；
/// 只有音节边界才插空格。
#[test]
fn translate_keeps_delimiter_in_preedit() {
    let index = delimiter_index();
    // 全拼 + 全拼：`xi'an` → [xi][an]，预编辑与输入同形。
    assert_eq!(candidate_preedits(&index, b"`xi'an")[0], "`xi'an");
    // 无分隔符时 `xian` 是一个音节，预编辑同样与输入同形（没有可插空格的边界）。
    assert_eq!(candidate_preedits(&index, b"`xian")[0], "`xian");
    let index = fixture_index();
    // 缩写 + 全拼：`zh'guo` → [zh][guo]（中国），分隔符保留。
    assert_eq!(candidate_texts(&index, b"`zh'guo"), ["中国"]);
    assert_eq!(candidate_preedits(&index, b"`zh'guo")[0], "`zh'guo");
    // 对照：音节边界（无分隔符）仍是空格。
    assert_eq!(candidate_preedits(&index, b"`zhongguo")[0], "`zhong guo");
    // 分隔符透明：`zhong'g` 与 `zhongg` 同候选，但预编辑各自保留分隔符 / 走空格规则。
    assert_eq!(
        candidate_texts(&index, b"`zhong'g"),
        candidate_texts(&index, b"`zhongg")
    );
    assert_eq!(candidate_preedits(&index, b"`zhong'g")[0], "`zhong'g");
    assert_eq!(candidate_preedits(&index, b"`zhongg")[0], "`zhong g");
    // 段首 / 段尾的分隔符同样原样可见（掩码两侧都记）。
    let index = delimiter_index();
    assert_eq!(candidate_preedits(&index, b"`'xi'an")[0], "`'xi'an");
    assert_eq!(candidate_preedits(&index, b"`xi'an'")[0], "`xi'an'");
    // 连续分隔符落在同一界上 ⇒ 预编辑里只出现一个。
    assert_eq!(candidate_preedits(&index, b"`xi''an")[0], "`xi'an");
    // 段尾分隔符：全拼未完成也当场可见（夹具里的 `zh` 是 `zhong` 的缩写）。
    let index = fixture_index();
    assert_eq!(candidate_preedits(&index, b"`zh'")[0], "`zh'");
}
