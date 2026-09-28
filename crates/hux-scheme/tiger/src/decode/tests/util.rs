// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! 解析工具用例：归一化、字母判定、选择器与边界解析。

use super::super::beam::{has_letter, normalize, parse_boundaries, parse_selector};

/// 归一化同时做去空白、去控制符、统一小写三件事，后续所有比较都依赖它。
#[test]
fn normalize_strips_whitespace_and_control() {
    assert_eq!(
        normalize("A B\tC\r\n"),
        b"abc",
        "空白与控制符都要剔除，字母统一小写"
    );
    assert_eq!(
        normalize("a\u{0b}b"),
        b"ab",
        "垂直制表符同属控制符，必须剔除"
    );
}

/// 字母判据决定输入走哪条解码支路，纯数字必须落到另一支。
#[test]
fn has_letter_detects_ascii_letters() {
    assert!(has_letter(b"a1"), "含字母即判真");
    assert!(!has_letter(b"123"), "纯数字不算字母");
}

/// 选择器解析对齐参照实现的匹配口径：分号取原值、撇号固定 3、0 表示第 10 项、溢出饱和。
#[test]
fn parse_selector_parses_and_saturates() {
    assert_eq!(
        parse_selector(b"ab;", 2),
        (2, 3),
        "分号选择器序号取分号前的值，游标停在其后"
    );
    assert_eq!(parse_selector(b"ab'", 2), (3, 3), "撇号选择器序号固定为 3");
    assert_eq!(parse_selector(b"ab0", 2), (10, 3), "数字 0 代表第 10 项");
    assert_eq!(parse_selector(b"ab12", 2), (12, 4), "多位数字按十进制解析");
    assert_eq!(
        parse_selector(b"ab00", 2),
        (0, 4),
        "00 解析为第 0 项，不得当成 10"
    );
    assert_eq!(
        parse_selector(b"ab99999999999999999999", 2),
        (u64::MAX, 22),
        "溢出必须饱和到 u64::MAX，不得 panic 或回绕"
    );
    assert_eq!(
        parse_selector(b"ab", 2),
        (0, 2),
        "无选择器时序号 0 且游标不动"
    );
}

/// 边界解析照抄参照实现的匹配语义：失败起点右移重试，同段取最后一组。
#[test]
fn parse_boundaries_matches_gmatch() {
    assert_eq!(
        parse_boundaries("2,3;"),
        vec![(2, 3)],
        "单段边界必须解析为一对数字"
    );
    assert_eq!(
        parse_boundaries("2,3;4,6;"),
        vec![(2, 3), (4, 6)],
        "多段边界按出现顺序解析"
    );
    assert_eq!(
        parse_boundaries(""),
        Vec::<(usize, usize)>::new(),
        "空串给空表，不得报错"
    );
    assert_eq!(
        parse_boundaries("abc"),
        Vec::<(usize, usize)>::new(),
        "无数字字段的段整体丢弃"
    );
    assert_eq!(
        parse_boundaries("2,;"),
        Vec::<(usize, usize)>::new(),
        "缺第二个数字的段丢弃"
    );
    assert_eq!(
        parse_boundaries("2,3"),
        Vec::<(usize, usize)>::new(),
        "缺分号终止符的段丢弃"
    );
    assert_eq!(
        parse_boundaries("x2,3;"),
        vec![(2, 3)],
        "前缀噪声靠右移重试找回该段"
    );
    // gmatch 语义：失败起点右移重试
    assert_eq!(
        parse_boundaries("12,34,56;"),
        vec![(34, 56)],
        "同段内多组数字时取最后一组"
    );
    assert_eq!(
        parse_boundaries("1,2,3;"),
        vec![(2, 3)],
        "三段数字同样取最后一组"
    );
}
