// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! 码表解析 / 追加码表 / 奖励曲线 / 补充匹配的单元测试。

use hux_core::collections::Map;

use super::parse::parse_codes_content;
use super::supplement::reward_for_weight;
use super::*;

#[test]
fn parses_codes_with_dedup_and_lowercasing() {
    let entries = parse_codes_content("来\tA\n那个\tab\n来\ta\n坏\tA1\n缺码\t\n");
    assert_eq!(
        entries,
        vec![
            ("来".to_string(), "a".to_string()),
            ("那个".to_string(), "ab".to_string()),
        ]
    );
}

#[test]
fn reward_matches_reference_curve() {
    assert_eq!(reward_for_weight(1000.0), 9.0);
    assert!((reward_for_weight(4000.0) - (9.0 + 2.0 * 4.0f64.ln())).abs() < 1e-12);
    assert_eq!(reward_for_weight(1e12), 16.0);
}

#[test]
fn supplement_advance_walks_trie() {
    let mut entries = Map::new();
    entries.insert("甲乙".to_string(), 4000.0);
    let matcher = Supplement::build(&entries, None);
    assert_eq!(matcher.count, 1);
    let (state, reward) = matcher.advance(1, '甲');
    assert_eq!(reward, 0.0);
    let (state, reward) = matcher.advance(state, '乙');
    assert!((reward - reward_for_weight(4000.0)).abs() < 1e-12);
    let _ = state;
}

/// 追加码表拼在主表之后：既有码上主表条目仍是 rank 1（简码归主表），重复对去重；
/// 追加表引入的新码可查。
#[test]
fn extra_code_table_appends_after_the_primary_table() {
    let dir = hux_test_support::temp_dir("lexicon-extra-codes");
    std::fs::write(dir.join(CODES_FILE), "来\ta\n").unwrap();
    std::fs::write(
        dir.join("tiger_sentence.codes.huma.txt"),
        "# 追加表：注释与空行照旧忽略\n来\ta\n\n𠀀\tfgf\n",
    )
    .unwrap();
    let lexicon = Lexicon::load(std::slice::from_ref(&dir), 0);
    let entries = lexicon.probe("a").expect("码 a");
    assert_eq!(entries.len(), 1, "与主表重复的 (字, 码) 应被去重保首见");
    assert_eq!(entries[0].text, "来");
    assert_eq!(entries[0].rank, 1);
    assert_eq!(lexicon.probe("fgf").expect("追加表的新码")[0].text, "𠀀");
    assert_eq!(lexicon.codes_entries, 2);
    std::fs::remove_dir_all(&dir).ok();
}

/// 文件名不合前后缀的（备份、空名、别的方案）都不算追加表；缺失追加表时主表照常。
#[test]
fn extra_code_table_requires_the_name_pattern() {
    let dir = hux_test_support::temp_dir("lexicon-extra-codes-pattern");
    std::fs::write(dir.join(CODES_FILE), "来\ta\n").unwrap();
    std::fs::write(dir.join("tiger_sentence.codes.txt.bak"), "不该被读\tzz\n").unwrap();
    std::fs::write(dir.join("tiger_sentence.codes..txt"), "不该被读\tzy\n").unwrap();
    std::fs::write(dir.join("别的.codes.extra.txt"), "不该被读\tzx\n").unwrap();
    let lexicon = Lexicon::load(std::slice::from_ref(&dir), 0);
    assert!(lexicon.probe("zz").is_none());
    assert!(lexicon.probe("zy").is_none());
    assert!(lexicon.probe("zx").is_none());
    assert_eq!(lexicon.codes_entries, 1);
    assert_eq!(lexicon.probe("a").unwrap()[0].text, "来");
    std::fs::remove_dir_all(&dir).ok();
}

/// 追加表只从**主表所在的数据目录**取：别的数据目录里的追加表不混进来
/// （差分夹具目录提供主表、`data/` 只提供词先验，正是这种用法）；同目录的追加表
/// 按文件名字典序拼在主表之后。
#[test]
fn extra_code_tables_only_come_from_the_primary_directory() {
    let fixture = hux_test_support::temp_dir("lexicon-extra-fixture");
    let other = hux_test_support::temp_dir("lexicon-extra-other");
    std::fs::write(fixture.join(CODES_FILE), "甲\tab\n").unwrap();
    std::fs::write(other.join("tiger_sentence.codes.zzz.txt"), "乙\tab\n").unwrap();
    let texts_of = |lexicon: &Lexicon| -> Vec<String> {
        lexicon
            .probe("ab")
            .expect("码 ab")
            .iter()
            .map(|entry| entry.text.clone())
            .collect()
    };
    let lexicon = Lexicon::load(&[fixture.clone(), other.clone()], 0);
    assert_eq!(
        texts_of(&lexicon),
        vec!["甲"],
        "别的目录里的追加表不该被读入"
    );

    std::fs::write(fixture.join("tiger_sentence.codes.bbb.txt"), "丙\tab\n").unwrap();
    std::fs::write(fixture.join("tiger_sentence.codes.aaa.txt"), "丁\tab\n").unwrap();
    let lexicon = Lexicon::load(&[fixture.clone(), other.clone()], 0);
    assert_eq!(texts_of(&lexicon), vec!["甲", "丁", "丙"]);
    assert_eq!(
        lexicon
            .extra_code_tables()
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        vec![
            "tiger_sentence.codes.aaa.txt",
            "tiger_sentence.codes.bbb.txt"
        ],
        "诊断口径应给出实际装载的追加表（按字典序）"
    );
    std::fs::remove_dir_all(&fixture).ok();
    std::fs::remove_dir_all(&other).ok();
}

/// 每张追加表各自剥 BOM：`normalize_text_content` 只剥得掉合并内容最前面那个，
/// 否则第二张表起首行的 BOM 会粘进候选文本。
#[test]
fn extra_code_table_bom_is_stripped_per_table() {
    let dir = hux_test_support::temp_dir("lexicon-extra-bom");
    std::fs::write(dir.join(CODES_FILE), "\u{feff}甲\ta\n").unwrap();
    std::fs::write(dir.join("tiger_sentence.codes.aaa.txt"), "\u{feff}乙\tab\n").unwrap();
    std::fs::write(dir.join("tiger_sentence.codes.bbb.txt"), "\u{feff}丙\tab\n").unwrap();
    let lexicon = Lexicon::load(std::slice::from_ref(&dir), 0);
    assert_eq!(lexicon.probe("a").expect("码 a")[0].text, "甲");
    let ab: Vec<&str> = lexicon
        .probe("ab")
        .expect("码 ab")
        .iter()
        .map(|entry| entry.text.as_str())
        .collect();
    assert_eq!(ab, vec!["乙", "丙"], "追加表的 BOM 未逐表剥掉");
    std::fs::remove_dir_all(&dir).ok();
}

/// 逐表解析与「拼成一个大串再解析」同语义：跨表去重按**小写化后**的码
/// （同一 `(字, 码)` 的大小写变体算重复），主表条目照旧保首见。
#[test]
fn extra_code_table_dedup_is_case_insensitive_across_tables() {
    let dir = hux_test_support::temp_dir("lexicon-extra-dedup");
    std::fs::write(dir.join(CODES_FILE), "来\tA\n").unwrap();
    std::fs::write(dir.join("tiger_sentence.codes.aaa.txt"), "来\ta\n甲\ta\n").unwrap();
    let lexicon = Lexicon::load(std::slice::from_ref(&dir), 0);
    let entries = lexicon.probe("a").expect("码 a");
    assert_eq!(
        entries
            .iter()
            .map(|entry| entry.text.as_str())
            .collect::<Vec<_>>(),
        vec!["来", "甲"],
        "追加表里与主表重复的 (字, 码) 大小写变体应被丢掉"
    );
    assert_eq!(lexicon.codes_entries, 2);
    std::fs::remove_dir_all(&dir).ok();
}

/// 关掉全字集：追加表不入词库（连读盘都不做），诊断口径为空；重新打开即恢复。
#[test]
fn extra_code_tables_can_be_disabled() {
    let dir = hux_test_support::temp_dir("lexicon-full-charset");
    std::fs::write(dir.join(CODES_FILE), "来\ta\n").unwrap();
    std::fs::write(dir.join("tiger_sentence.codes.huma.txt"), "𠀀\tfgf\n").unwrap();
    let options = LexiconOptions {
        extra_code_tables: false,
        ..LexiconOptions::default()
    };
    let mut lexicon = Lexicon::load_with(std::slice::from_ref(&dir), 0, options);
    assert!(lexicon.probe("fgf").is_none(), "关掉全字集时追加表不入词库");
    assert!(lexicon.extra_code_tables().is_empty());
    assert_eq!(lexicon.codes_entries, 1);
    assert_eq!(lexicon.options(), options);
    assert_eq!(
        lexicon.data_info(),
        "code_tables=[tiger_sentence.codes.txt] entries=1 chars=1 full_charset=0 \
         filter_non_han=1"
    );

    // 重新打开：与缺省口径（[`Lexicon::load`]）逐项一致。
    lexicon.apply_lexicon_options(0, LexiconOptions::default());
    assert_eq!(lexicon.probe("fgf").expect("追加表的码")[0].text, "𠀀");
    assert_eq!(
        lexicon.extra_code_tables(),
        ["tiger_sentence.codes.huma.txt".to_string()]
    );
    assert_eq!(lexicon.codes_entries, 2);
    assert_eq!(
        lexicon.data_info(),
        "code_tables=[tiger_sentence.codes.txt,tiger_sentence.codes.huma.txt] entries=2 \
         chars=2 full_charset=1 filter_non_han=1"
    );
    std::fs::remove_dir_all(&dir).ok();
}

/// 过滤只作用于**追加表**：主表行（差分金样夹具 = 上游主表，含 3 个非汉字）照旧，
/// 追加表里的单字符非汉字被丢掉；词与扩展区汉字不受影响。
#[test]
fn filter_non_han_only_applies_to_extra_code_tables() {
    let dir = hux_test_support::temp_dir("lexicon-filter-non-han");
    // 主表：一个非汉字（`々`）+ 一个汉字。
    std::fs::write(dir.join(CODES_FILE), "々\taa\n来\tab\n").unwrap();
    // 追加表：单字符非汉字（部首 `⽧`）/ 含非汉字的词 / 扩展 B 汉字。
    std::fs::write(
        dir.join("tiger_sentence.codes.extra.txt"),
        "⽧\tba\n々々\tbb\n𤕫\tbc\n",
    )
    .unwrap();

    let lexicon = Lexicon::load(std::slice::from_ref(&dir), 0);
    assert_eq!(
        lexicon.probe("aa").expect("主表的非汉字")[0].text,
        "々",
        "主表行不受过滤影响"
    );
    assert!(
        lexicon.probe("ba").is_none(),
        "追加表的单字符非汉字应被丢掉"
    );
    assert_eq!(lexicon.probe("bb").expect("含非汉字的词")[0].text, "々々");
    assert_eq!(lexicon.probe("bc").expect("扩展 B 汉字")[0].text, "𤕫");
    let filtered_entries = lexicon.codes_entries;

    // 关掉过滤：追加表的非汉字行入词库（正好多一条），主表条目照旧。
    let unfiltered = Lexicon::load_with(
        std::slice::from_ref(&dir),
        0,
        LexiconOptions {
            filter_non_han: false,
            ..LexiconOptions::default()
        },
    );
    assert_eq!(unfiltered.probe("ba").expect("关掉过滤")[0].text, "⽧");
    assert_eq!(unfiltered.codes_entries, filtered_entries + 1);
    assert_eq!(unfiltered.probe("aa").expect("主表的非汉字")[0].text, "々");
    assert_eq!(unfiltered.probe("bb").expect("词")[0].text, "々々");
    std::fs::remove_dir_all(&dir).ok();
}
