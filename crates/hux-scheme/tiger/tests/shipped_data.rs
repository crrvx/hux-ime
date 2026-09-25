// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! 随包数据（`data/`）自检：主表 + 追加码表装出来的字集，以及「只追加」不变量。
//!
//! 与差分金样互补：差分用 `goldens/` 夹具（稳定、可比对上游），这里用真正随包的那份数据，
//! 保证「生僻字可打」与「主表 rank / 派生标志不变」两条不因追加表而回归。

use hux_scheme_tiger::lexicon::Lexicon;
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

#[test]
fn shipped_data_loads_primary_and_extra_code_tables() {
    let data = repo_path("data");
    let lexicon = Lexicon::load(std::slice::from_ref(&data), 0);
    assert!(lexicon.built);
    assert!(lexicon.errors.is_empty(), "{:?}", lexicon.errors);

    // 主表 15,369 条 + 追加表 102,414 条（“只追加” ⇒ 主表行序不动）。
    assert!(
        lexicon.codes_entries > 117_000,
        "随包码表条目太少：{}（追加表没装上？）",
        lexicon.codes_entries
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

    // 垫后的是新字（`ab` = 交/疒 + 追加 ⽧/𤕫），不是主表已有的字。
    let ab = texts(&lexicon, "ab");
    assert_eq!(ab[0], "交");
    assert_eq!(ab[1], "疒");
    assert!(ab.contains(&"⽧".to_string()), "追加表的新字应垫后：{ab:?}");
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
