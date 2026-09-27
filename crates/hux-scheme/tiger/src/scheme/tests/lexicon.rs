// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! 词库装载与重建的用例：补充短语目录查找、高频上限与字集开关的重建。

use super::super::assets::role;
use super::*;
use crate::lexicon::SUPPLEMENT_FILE;

#[test]
fn supplement_dir_searches_all_data_dirs() {
    // 一键安装把数据装在系统级目录（用户目录在前但为空）时，补充短语仍须被找到。
    let user_dir = hux_test_support::temp_dir("supplement-user");
    let system_dir = hux_test_support::temp_dir("supplement-system");
    assert_eq!(
        supplement_dir(&[user_dir.clone(), system_dir.clone()]),
        None
    );
    std::fs::write(system_dir.join(SUPPLEMENT_FILE), "甲 乙 2\n").expect("write");
    assert_eq!(
        supplement_dir(&[user_dir.clone(), system_dir.clone()]),
        Some(system_dir.clone())
    );
    std::fs::write(user_dir.join(SUPPLEMENT_FILE), "甲 乙 2\n").expect("write");
    assert_eq!(
        supplement_dir(&[user_dir.clone(), system_dir]),
        Some(user_dir)
    );
}

#[test]
fn apply_config_rebuilds_the_lexicon_for_a_new_high_freq_limit() {
    // 平台在装配方案**之后**才把配置页设置下发（`hux_engine_new` → 宿主 `applyConfig`），
    // 故上限只在 `load` 时生效等于「设置永不生效」；本用例钉住重新下发即重建。
    //
    // 跨层分工：平台侧 `platform/fcitx5/src/tests.rs` 的
    // `apply_settings_rebuilds_the_lexicon_for_a_new_high_freq_limit` 负责引擎可见结果
    // （候选列表里非主码条目消失/回来）；本用例只断言内核独有的**容量上界**：
    // 上限决定码表里保留的 (码, 字) 槽位总数，收紧必须真的丢槽位、放开必须完整复原。
    let capacity = |scheme: &TigerScheme| -> usize {
        scheme
            .decoder
            .lexicon()
            .codes
            .iter()
            .map(|(_, entries)| entries.len())
            .sum()
    };
    let mut scheme = fixture_scheme(); // 夹具按上限 0（不过滤）装载
    let full = capacity(&scheme);
    assert!(full > 0, "夹具码表非空");
    assert_eq!(scheme.decoder.lexicon().data_status().high_freq_limit, 0);
    scheme
        .apply_config(&full_bag(&[(role::HIGH_FREQ_LIMIT, Value::Count(1500))]))
        .expect("全角色袋");
    let tightened = capacity(&scheme);
    assert!(
        tightened < full,
        "收紧上限必须丢弃非主码槽位：{tightened} 应小于 {full}"
    );
    assert_eq!(scheme.decoder.lexicon().data_status().high_freq_limit, 1500);
    // 放开上限同样重建（不是「只收紧一次」）⇒ 容量回到原值。
    scheme
        .apply_config(&full_bag(&[(role::HIGH_FREQ_LIMIT, Value::Count(0))]))
        .expect("全角色袋");
    assert_eq!(capacity(&scheme), full, "放开上限必须完整复原槽位");
    assert_eq!(scheme.decoder.lexicon().data_status().high_freq_limit, 0);
    assert_eq!(
        scheme.learning_mode(),
        format!(
            "sentence-v2|rules={}|optimal=0|dup=1",
            scheme.learning_rules
        )
    );
}

/// 夹具码表 + **一张追加码表**（一个扩展 B 汉字与一个部首，各给一个新码）：
/// 字集开关的用例要有追加表才能看出效果（[`fixture_dirs`] 里没有）。
fn charset_dirs() -> PathBuf {
    let dir = hux_test_support::temp_dir("scheme-charset");
    let fixture = fixture_dirs().remove(0);
    for name in [
        "tiger_sentence.codes.txt",
        "tiger_sentence.char_ranks.txt",
        "tiger_sentence.full_code_whitelist.txt",
    ] {
        std::fs::copy(fixture.join(name), dir.join(name)).expect("复制夹具码表");
    }
    std::fs::write(
        dir.join("tiger_sentence.codes.huma.txt"),
        "𤕫\tzzzv\n⽧\tzzzw\n",
    )
    .expect("写追加码表");
    dir
}

/// 两个字集开关都改词库内容 ⇒ 重新下发配置即重建（同高频上限的重建路径）。
///
/// 语义：关掉全字集只装主表；过滤只作用于**追加表**（主表里的非汉字照旧）。
#[test]
fn apply_config_rebuilds_the_lexicon_for_the_charset_options() {
    let dir = charset_dirs();
    // 仅主表的条目数（关掉全字集装载；下面用它作基准口径）。
    let primary_entries = primary_entry_count(&dir);

    // 出厂口径（全字集开 + 过滤开）：追加表的汉字在，部首被过滤。
    let mut scheme = scheme_of(&dir, &[]);
    assert_eq!(texts(&scheme, "zzzv"), vec!["𤕫".to_string()]);
    assert!(
        scheme.decoder.lexicon().probe("zzzw").is_none(),
        "追加表里的部首应被过滤"
    );
    assert_eq!(scheme.decoder.lexicon().extra_code_tables().len(), 1);
    let filtered_entries = scheme.decoder.lexicon().codes_entries;
    assert_eq!(
        filtered_entries,
        primary_entries + 1,
        "过滤开：追加表只剩那个汉字"
    );
    check_loaded_table_summary(&scheme, filtered_entries);
    assert!(
        scheme
            .data_info()
            .ends_with("full_charset=1 filter_non_han=1")
    );

    // 关掉全字集：追加表独有码消失、诊断口径为空（重新打开同样重建，不是「只关一次」）。
    reapply_config(&mut scheme, &[(role::FULL_CHARSET, Value::Bool(false))]);
    assert!(scheme.decoder.lexicon().probe("zzzv").is_none());
    assert!(scheme.decoder.lexicon().extra_code_tables().is_empty());
    check_primary_only_summary(&scheme, primary_entries);
    reapply_config(&mut scheme, &[(role::FULL_CHARSET, Value::Bool(true))]);
    assert_eq!(texts(&scheme, "zzzv"), vec!["𤕫".to_string()]);

    // 关掉过滤：追加表的部首入词库（条目正好多一条），主表内容不动。
    reapply_config(&mut scheme, &[(role::FILTER_NON_HAN, Value::Bool(false))]);
    assert_eq!(texts(&scheme, "zzzw"), vec!["⽧".to_string()]);
    assert_eq!(
        scheme.decoder.lexicon().codes_entries,
        primary_entries + 2,
        "过滤关：追加表两行都入词库"
    );
    assert!(
        scheme
            .data_info()
            .ends_with("full_charset=1 filter_non_han=0")
    );

    std::fs::remove_dir_all(&dir).ok();
}

/// 以夹具码表目录装载方案（`overrides` 覆盖全角色袋里的同名角色）。
fn scheme_of(dir: &PathBuf, overrides: &[(&'static str, Value)]) -> TigerScheme {
    TigerScheme::load(std::slice::from_ref(dir), None, &full_bag(overrides)).0
}

/// 码 `code` 的候选文本（码不存在即断言失败）。
fn texts(scheme: &TigerScheme, code: &str) -> Vec<String> {
    scheme
        .decoder
        .lexicon()
        .probe(code)
        .unwrap_or_else(|| panic!("码 {code} 不存在"))
        .iter()
        .map(|entry| entry.text.clone())
        .collect()
}

/// 仅主表的条目数（关掉全字集装载）。
fn primary_entry_count(dir: &PathBuf) -> usize {
    scheme_of(dir, &[(role::FULL_CHARSET, Value::Bool(false))])
        .decoder
        .lexicon()
        .codes_entries
}

/// 重新下发全角色袋（配置页设置路径）；此处必须无诊断。
fn reapply_config(scheme: &mut TigerScheme, overrides: &[(&'static str, Value)]) {
    scheme.apply_config(&full_bag(overrides)).expect("全角色袋");
}

/// 装载摘要须含实际装载的码表与过滤后的条目数。
fn check_loaded_table_summary(scheme: &TigerScheme, filtered_entries: usize) {
    assert!(
        scheme.data_info().starts_with(&format!(
            "code_tables=[tiger_sentence.codes.txt,tiger_sentence.codes.huma.txt] \
             entries={filtered_entries}"
        )),
        "装载摘要应含实际装载的码表：{}",
        scheme.data_info()
    );
}

/// 关掉全字集后的摘要：只剩主表，条目数按主表口径。
fn check_primary_only_summary(scheme: &TigerScheme, primary_entries: usize) {
    assert_eq!(
        scheme.data_info(),
        format!(
            "code_tables=[tiger_sentence.codes.txt] entries={primary_entries} \
             chars={} full_charset=0 filter_non_han=1",
            scheme.decoder.lexicon().character_codes.len()
        ),
        "关掉全字集后摘要只剩主表"
    );
}
