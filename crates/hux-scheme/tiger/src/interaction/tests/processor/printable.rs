// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! 可打印字符路径：光标处插入、菜单可见时的标点确认与学习暂存，
//! 以及翻页键（`-`/`=`）让位宿主 key_binder 的偏离分支。

use super::*;

/// 参照 `abad411`：**菜单可见**（不要求缓冲态）时遇可打印 ASCII 标点，必须先按当前选中项
/// stage 学习、确认组合（`_auto_commit` 下即上屏），再把原键交标点表。
///
/// 参照依据（提交信息）：标点段一旦被 punctuator 追加进组合，`learning_selection` 就再也
/// 解不出该输入（如 `zhhbi,`）或取不回句子的选中项；故判据由 `state.buffered_text ~= ""`
/// 改为 `context:has_menu()`。金样覆盖不到该学习路径（探针 `store_ready == false` 短路），
/// 故这里经 `processor` 端到端钉住：本用例的 `buffered_text` 为空，旧判据**不**成立。
#[test]
fn processor_menu_punctuation_stages_learning_before_the_punctuator() {
    let mode = "sentence-v2|rules=|optimal=1500|dup=1";
    let mut h = Harness::new();
    h.context.set_option("_auto_commit", true);
    h.live.mode = mode.to_string();
    h.live.store_ready = true;
    for repr in ["a", "b", "a", "b"] {
        assert_eq!(h.press(repr), ProcessorResult::Consume);
    }
    // 真实会话里菜单由 translator 建立（交交 / 交疒 = 两条 2 码边，均为 composed-only）。
    h.push_segment(b"abab", &["交交", "交疒"]);
    assert!(h.state.buffered_text.is_empty(), "旧判据在此不成立");
    h.context.highlight(1); // 人工纠错：选中第二项
    let selection = learning_selection(&mut h.decoder, &h.context, &h.state).expect("selection");
    let selected = selection.selected.clone().expect("第 2 个候选");
    let first = selection.first.clone().expect("首个候选");
    let expected = learning::diff(
        b"abab",
        Some(&first.diff),
        Some(&selected.diff),
        0,
        mode,
        0.0,
    );
    assert_eq!(expected.len(), 1, "夹具前提：交交 / 交疒 仅末段不同");
    // 逗号：确认组合（上屏「交疒」）后原键仍交标点表。
    assert_eq!(h.press("comma"), ProcessorResult::Forward);
    assert!(h.context.input().is_empty());
    assert_eq!(h.context.last_commit_text(), "交疒");
    assert!(h.live.pending.is_empty(), "提交点已消费 pending");
    assert_eq!(
        h.live.submitted.len(),
        expected.len(),
        "标点路径的纠错必须落学习"
    );
    for (got, want) in h.live.submitted.iter().zip(expected.iter()) {
        assert_eq!(got.time, want.time);
        assert_eq!(got.mode, want.mode);
        assert_eq!(got.code, want.code);
        assert_eq!(got.text, want.text);
        assert_eq!(got.context, want.context);
    }
    assert_eq!(h.live.submitted[0].mode, mode);
}

/// **本仓有意偏离上游 `abad411`**（用户决定 B：菜单可见时翻页键一律拦截）：
/// 菜单可见时，凡会落入宿主翻页绑定的标点键都不由标点分支消费。
///
/// 上游对「菜单可见 + 可打印 ASCII 标点」一律「暂存学习 + 确认组合 + 交标点表」，于是
/// `-/=`（以及 schema 绑到翻页的 `[/]`）的 key_binder 绑定被永久遮蔽（最小复现 `j a equal`）。
/// 本仓在分支入口先问宿主同一套判据 `hux_core::host::paging_action`：
/// `=`（下翻）与 `-`（上翻，判据同为「菜单可见」）都让给宿主翻页，
/// 不确认组合、不暂存学习；`ascii_mode` 打开时判据不成立 ⇒ 回到上游路径。
/// 代价：菜单可见时 `-`/`=`/`[`/`]` 不能作为标点打出；相关上游金样用例按 `DEVIATIONS` 登记。
#[test]
fn processor_menu_paging_keys_bypass_the_punctuation_branch() {
    let mut h = Harness::new();
    h.context.set_option("_auto_commit", true);
    h.live.store_ready = true;
    h.push_segment(b"ab", &["交", "疒"]);
    assert!(h.context.has_menu());
    h.context.highlight(1); // 人工纠错候选：若走上游标点分支会立刻上屏「疒」
    assert_eq!(h.context.last_commit_text(), "");

    // `=`：`when: has_menu` 成立 ⇒ 让给宿主下翻页（不消费、不确认组合、不 stage 学习）。
    assert_eq!(h.press("equal"), ProcessorResult::Forward);
    assert_eq!(h.context.last_commit_text(), "", "`=` 不得确认组合");
    assert_eq!(h.context.input(), b"ab", "`=` 后组合原样保留（交宿主翻页）");
    assert!(h.context.has_menu(), "`=` 后菜单仍在（交宿主翻页）");

    // `-`：菜单可见即判上翻页 ⇒ 同样让给宿主。
    assert_eq!(h.press("minus"), ProcessorResult::Forward);
    assert_eq!(
        h.context.last_commit_text(),
        "",
        "菜单可见时 `-` 不得确认组合"
    );
    assert_eq!(h.context.input(), b"ab", "`-` 后组合原样保留（交宿主翻页）");
    assert!(h.context.has_menu(), "`-` 后菜单仍在（交宿主翻页）");
    assert!(
        h.live.submitted.is_empty(),
        "让给宿主的键不暂存学习（否则标点路径会记一次人工纠错）"
    );

    // 判据的另一半：`ascii_mode` 打开时翻页键**不**拦截 ⇒ `-` 回到上游标点路径
    // （确认组合后交标点表；本用例不跑宿主链，故只断言处理器的确认行为）。
    let mut ascii = Harness::new();
    ascii.context.set_option("_auto_commit", true);
    ascii.context.set_option("ascii_mode", true);
    ascii.live.store_ready = true;
    ascii.push_segment(b"ab", &["交", "疒"]);
    ascii.context.highlight(1);
    assert_eq!(ascii.press("minus"), ProcessorResult::Forward);
    assert_eq!(
        ascii.context.last_commit_text(),
        "疒",
        "`ascii_mode` 下 `-` 退回上游标点路径（确认组合）"
    );
    assert!(ascii.context.input().is_empty());
}

#[test]
fn processor_inserts_at_caret() {
    let mut h = Harness::new();
    h.context.set_input(b"ab");
    h.context.set_caret(1);
    assert_eq!(h.press("c"), ProcessorResult::Consume);
    assert_eq!(h.context.input(), b"acb");
    assert_eq!(h.context.caret(), 2);
}
