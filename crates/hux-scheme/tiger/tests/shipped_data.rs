// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! 随包数据（`data/`）自检：主表 + 追加码表装出来的字集、「只追加」不变量，
//! 以及两个字集开关（[`LexiconOptions`]）的效果。
//!
//! 与差分金样互补：差分用 `goldens/` 夹具（稳定、可比对上游），这里用真正随包的那份数据，
//! 保证「生僻字可打」与「主表 rank / 派生标志不变」两条不因追加表而回归。

use hux_scheme_tiger::lexicon::{Lexicon, LexiconOptions};
use hux_test_support::repo_path;

/// 某个码下的候选文本（按 rank）。
fn texts(lexicon: &Lexicon, code: &str) -> Vec<String> {
    lexicon
        .probe(code)
        .unwrap_or_else(|| panic!("码 {code} 不存在"))
        .iter()
        .map(|entry| entry.text.clone())
        .collect()
}

/// 字集开关的四种组合（出厂缺省 = 两者皆开）。
fn options(extra_code_tables: bool, filter_non_han: bool) -> LexiconOptions {
    LexiconOptions {
        extra_code_tables,
        filter_non_han,
    }
}

#[test]
fn shipped_data_loads_primary_and_extra_code_tables() {
    let data = repo_path("data");
    let lexicon = Lexicon::load(std::slice::from_ref(&data), 0);
    assert!(lexicon.built);
    assert!(lexicon.errors.is_empty(), "{:?}", lexicon.errors);

    // 主表 15,369 条 + 追加表 102,332 条 = 117,701 行；出厂口径过滤掉追加表里 939 行
    // 非汉字（931 个不同字符）⇒ 116,762 条。区间而非绝对值：数据换了也不至于只因条数抖动而红。
    assert!(
        (116_000..117_000).contains(&lexicon.codes_entries),
        "随包码表条目数不在出厂口径区间：{}（追加表没装上，或过滤没生效？）",
        lexicon.codes_entries
    );
    assert_eq!(
        lexicon.extra_code_tables().len(),
        1,
        "追加表应被装载（诊断口径按文件名给出）"
    );

    // 追加表独有码 + 扩展 B 生僻字：主表里没有这个码，只有追加表能给。
    assert_eq!(texts(&lexicon, "aaad")[0], "𬻄");
    assert_eq!(texts(&lexicon, "aaae")[0], "𰀰");
}

/// 主表 rank 逐位不变，且追加表只补**主表没有的字**：主表已有的字（`ot` 是/题、`sh` 说/办）
/// 其官方码不并入，免得改它们的最优码；垫后的一律是新字。
#[test]
fn shipped_data_keeps_primary_ranks_for_shared_codes() {
    let data = repo_path("data");
    let lexicon = Lexicon::load(std::slice::from_ref(&data), 0);

    let ot = texts(&lexicon, "ot");
    assert_eq!(ot[0], "是", "主表 rank 1 被追加表顶掉了：{ot:?}");
    assert!(
        !ot.contains(&"题".to_string()),
        "主表已有的字不该被追加表补官方码（会改它的最优码）：{ot:?}"
    );

    let sh = texts(&lexicon, "sh");
    assert_eq!(sh[0], "说", "主表 rank 1 被追加表顶掉了：{sh:?}");
    assert!(
        !sh.contains(&"办".to_string()),
        "主表已有的字不该被追加表补官方码（会改它的最优码）：{sh:?}"
    );

    // 主表 `aaaa` = 卍/卐；追加表只补**主表没有的字**，故 𨷾 垫在其后。
    let aaaa = texts(&lexicon, "aaaa");
    assert_eq!(aaaa[0], "卍", "主表 rank 1 被追加表顶掉了：{aaaa:?}");
    assert_eq!(aaaa[1], "卐", "主表 rank 2 被追加表顶掉了：{aaaa:?}");
    assert!(
        aaaa.contains(&"𨷾".to_string()),
        "追加表的新字应垫后：{aaaa:?}"
    );

    // `ab` = 交/疒 + 追加表垫后的 `𤕫`（扩展 B 汉字）与新字；出厂口径**不含**部首 `⽧`
    // （过滤只作用于追加表，见 `shipped_data_charset_options_change_the_loaded_set`）。
    let ab = texts(&lexicon, "ab");
    assert_eq!(ab[0], "交");
    assert_eq!(ab[1], "疒");
    assert!(ab.contains(&"𤕫".to_string()), "追加表的汉字应垫后：{ab:?}");
    assert!(!ab.contains(&"⽧".to_string()), "部首应被过滤：{ab:?}");
}

/// 两个字集开关各自的效果（出厂缺省 = 全字集开 + 过滤开）：
/// 关掉全字集只装主表；关掉过滤则追加表里的非汉字行入词库。
#[test]
fn shipped_data_charset_options_change_the_loaded_set() {
    let data = repo_path("data");
    let default = Lexicon::load(std::slice::from_ref(&data), 0);

    // 关掉过滤：追加表的非汉字行（部首/笔画/注音/假名）全部入词库——现数据 939 行。
    let unfiltered = Lexicon::load_with(std::slice::from_ref(&data), 0, options(true, false));
    assert_eq!(
        unfiltered.codes_entries,
        default.codes_entries + 939,
        "过滤掉的应是追加表里的非汉字行"
    );
    assert!(
        texts(&unfiltered, "ab").contains(&"⽧".to_string()),
        "关掉过滤后部首应出现在候选里：{:?}",
        texts(&unfiltered, "ab")
    );

    // 关掉全字集：只剩主表（15,369 条），追加表独有码消失、诊断口径为空。
    let primary = Lexicon::load_with(std::slice::from_ref(&data), 0, options(false, true));
    assert_eq!(primary.codes_entries, 15_369, "应回到主表规模");
    assert!(
        primary.probe("aaad").is_none(),
        "追加表独有码不该出现在只装主表的视图里"
    );
    assert!(primary.extra_code_tables().is_empty());
    assert!(
        !texts(&primary, "ab").contains(&"𤕫".to_string()),
        "追加表的汉字同样不该出现：{:?}",
        texts(&primary, "ab")
    );
    // 没有追加表可过滤 ⇒ 过滤开关在此组合下无影响。
    let primary_unfiltered =
        Lexicon::load_with(std::slice::from_ref(&data), 0, options(false, false));
    assert_eq!(primary_unfiltered.codes_entries, 15_369);
}

/// 规则指纹（学习库分区）在出厂口径下与「把各表拼成一个大串再哈希」**逐位一致**：
/// 指纹取的是码表**原始内容**的拼接（主表 + 各追加表，逐表剥 BOM），与解析后丢掉哪些条目
/// 无关——过滤非汉字只改词库内容，故升级不重置既有学习库分区。
#[test]
fn shipped_data_learning_rules_match_the_merged_content_hash() {
    let data = repo_path("data");
    let lexicon = Lexicon::load(std::slice::from_ref(&data), 0);
    let read = |name: &str| std::fs::read_to_string(data.join(name)).expect("随包数据");
    let mut merged = read("tiger_sentence.codes.txt");
    for name in lexicon.extra_code_tables() {
        let extra = read(name);
        merged.push('\n');
        merged.push_str(extra.strip_prefix('\u{feff}').unwrap_or(&extra));
    }
    assert_eq!(
        lexicon.learning_rules,
        hux_core::learning::hash(&format!(
            "{merged}\0{}\0{}",
            read("tiger_sentence.char_ranks.txt"),
            read("tiger_sentence.full_code_whitelist.txt")
        )),
        "规则指纹的合成口径变了（学习库分区会随之改变）"
    );
}

/// 主表条目的**派生标志**同样不许被追加表改动：逐码比对「仅主表」与「主表 + 追加表」两份装载，
/// 主表条目必须是合并视图的严格前缀，且 `text` / `rank` / `optimal_single` 完全一致。
///
/// 只查 rank 会漏掉一类回归：追加表若给主表**已有的字**补了更短的码，该字的 `optimal_single`
/// 会由 true 变 false（「整串直出」奖励不再可达）——所以生成器只收主表没有的字，这里把它钉住。
#[test]
fn shipped_data_does_not_change_primary_entry_flags() {
    let data = repo_path("data");
    let merged = Lexicon::load(std::slice::from_ref(&data), 0);

    // 仅主表：把主表与字频/白名单复制到临时目录，**不放**追加表。
    let only = hux_test_support::temp_dir("shipped-primary-only");
    for name in [
        "tiger_sentence.codes.txt",
        "tiger_sentence.char_ranks.txt",
        "tiger_sentence.full_code_whitelist.txt",
    ] {
        std::fs::copy(data.join(name), only.join(name)).expect("复制随包数据");
    }
    let primary = Lexicon::load(std::slice::from_ref(&only), 0);

    let mut checked = 0;
    for (code, entries) in primary.codes.iter() {
        let base = merged
            .probe(code)
            .unwrap_or_else(|| panic!("合并视图缺码 {code}"));
        assert!(
            base.len() >= entries.len(),
            "码 {code} 的候选比仅主表时还少"
        );
        for (index, want) in entries.iter().enumerate() {
            let got = &base[index];
            assert_eq!(got.text, want.text, "码 {code} 第 {index} 条文本变了");
            assert_eq!(got.rank, want.rank, "码 {code} 第 {index} 条 rank 变了");
            assert_eq!(
                got.optimal_single, want.optimal_single,
                "码 {code} 第 {index} 条（{}）的 optimal_single 被追加表改动了",
                want.text
            );
        }
        checked += entries.len();
    }
    assert!(
        checked > 15_000,
        "只比对了 {checked} 条主表条目，夹具不对？"
    );
    std::fs::remove_dir_all(&only).ok();
}
