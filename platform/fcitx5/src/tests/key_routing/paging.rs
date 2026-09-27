// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! 翻页键不被标点分支遮蔽：菜单可见时 `=`/`-` 下翻/上翻、**首屏** `-` 亦上翻、
//! 显式 `Page_Down` 照常翻页；空闲态 `=`/`-` 落标点，`ascii_mode` 打开则退回标点路径。

use super::*;

/// 菜单可见时的翻页键不再被方案标点分支遮蔽（**本仓有意偏离上游 `abad411`**）。
///
/// 上游标点分支对菜单可见的**所有**可打印 ASCII 标点先「暂存学习 + 确认组合」再交标点表，
/// 于是 schema 的 key_binder 翻页绑定（`-`/`=`，以及绑到翻页的 `[`/`]`）在这条路径上被遮蔽
/// （`Page_Down`/`Page_Up`/`Tab` 不受影响）。本仓在标点分支入口先问**与宿主同一套**判据
/// `hux_core::host::paging_action`：判为翻页的键不由标点分支消费，落回宿主链执行翻页。
/// 最小复现：`j a equal`；
/// 见 `interaction::tests::processor_menu_paging_keys_bypass_the_punctuation_branch`。
/// 受影响的上游金样用例在差分测试中按 `DEVIATIONS` 登记（金样字节保持原样）。
///
/// **用户决定 B（语义强化）**：上翻页键与下翻页键**同前置**——只要菜单可见就判翻页，
/// 不再要求参照 `when: paging` 的「已翻过页」标签（该标签已随其唯一读取方删除）。
/// 代价：菜单可见时 `-`/`=`/`[`/`]` 不再能作为标点打出。
///
/// 覆盖：① `=` 下翻不提交；② 翻页后 `-` 上翻不提交；③ **首屏**（未翻页）`-` 同样上翻
/// （回归场景，不再依赖任何标签）；④ `Page_Down` 始终翻页；⑤ 无菜单时 `=`/`-` 落标点；
/// ⑥ **负向对照**：`ascii_mode` 打开时判据不成立 ⇒ `-` 不拦截、退回上游标点路径。
///
/// 页大小先显式设为 3（≠ 缺省 5）：翻页期望值必须来自**配置**；若沿用与实现同源的
/// `hux_core::host::DEFAULT_PAGE_SIZE`（恰等于缺省配置），就分不清「读配置」与「硬编码 5」。
#[test]
fn menu_paging_keys_are_not_shadowed_by_the_punctuation_branch() {
    let _guard = serial();
    COMMITS.lock().unwrap().clear();
    UPDATES.lock().unwrap().clear();
    let mut engine = paging_engine();
    for code in *b"ja" {
        engine.key(u32::from(code), 0, false);
    }
    let first_page = last_update().2;
    assert!(
        first_page.len() > PAGE_SIZE,
        "夹具 ja 应有超过一页（{PAGE_SIZE}）的候选：{}",
        first_page.len()
    );

    paging_keys_navigate_instead_of_committing(&mut engine);
    first_page_paging_does_not_depend_on_a_paging_label();
    explicit_page_down_still_pages();
    idle_punctuation_keys_fall_through();
    ascii_mode_disables_the_paging_rule();
}

fn paging_keys_navigate_instead_of_committing(engine: &mut TestEngine) {
    // ① 菜单可见按 `=`：下翻一页、不提交（上游会提交「…=」）。
    COMMITS.lock().unwrap().clear();
    let selected_before = last_update().3;
    assert!(engine.key(0x3d, 0, false), "`=` 应被消费（翻页）");
    assert!(
        COMMITS.lock().unwrap().is_empty(),
        "`=` 不得提交组合：翻页绑定优先于标点分支"
    );
    assert_eq!(
        last_update().3,
        selected_before + PAGE_SIZE as i32,
        "`=` 应下翻一页（高亮前进一页）"
    );
    assert_eq!(engine.session().context.input(), b"ja", "翻页不改动输入");

    // ② 已翻页后按 `-`：上翻一页、仍不提交。
    assert!(engine.key(0x2d, 0, false), "翻页后 `-` 应被消费（上翻页）");
    assert!(COMMITS.lock().unwrap().is_empty(), "上翻页不得提交组合");
    assert_eq!(last_update().3, selected_before, "`-` 应回到第一页");
}

fn first_page_paging_does_not_depend_on_a_paging_label() {
    // ③ **首屏**按 `-`：菜单可见即判上翻页（不再要求 `when: paging` 标签）——
    //    先把高亮挪到第 2 项，再按 `-` ⇒ 归零高亮（参照 `PreviousPage` 的三元式）、不提交。
    let mut first_page_engine = paging_engine();
    for code in *b"ja" {
        first_page_engine.key(u32::from(code), 0, false);
    }
    let home_page = last_update().3;
    assert!(first_page_engine.key(0xff54, 0, false), "Down 前进一项");
    assert_eq!(last_update().3, home_page + 1, "Down 移动高亮");
    COMMITS.lock().unwrap().clear();
    assert!(
        first_page_engine.key(0x2d, 0, false),
        "首屏 `-` 应被消费（上翻页）"
    );
    assert!(
        COMMITS.lock().unwrap().is_empty(),
        "首屏 `-` 不得提交组合（上游会提交「…-」）"
    );
    assert_eq!(last_update().3, home_page, "首页上翻归零高亮（留在首页）");
    assert_eq!(
        first_page_engine.session().context.input(),
        b"ja",
        "翻页不改动输入"
    );
    //    同源路径（原场景）：显式 `Page_Up` 停在首页后再按 `-`，同样不得提交。
    COMMITS.lock().unwrap().clear();
    assert!(first_page_engine.key(0xff55, 0, false), "Page_Up 应被消费");
    assert!(COMMITS.lock().unwrap().is_empty(), "Page_Up 不提交");
    assert_eq!(last_update().3, home_page, "首页上翻归零高亮（留在首页）");
    assert!(
        first_page_engine.key(0x2d, 0, false),
        "Page_Up 后 `-` 应翻页"
    );
    assert!(
        COMMITS.lock().unwrap().is_empty(),
        "`-` 不得提交组合（原场景）"
    );
    assert_eq!(
        first_page_engine.session().context.input(),
        b"ja",
        "翻页不改动输入"
    );
}

fn explicit_page_down_still_pages() {
    // ④ 显式 `Page_Down` 仍是翻页路径（不受本偏离影响）。
    let mut page_engine = paging_engine();
    for code in *b"ja" {
        page_engine.key(u32::from(code), 0, false);
    }
    let page_start = last_update().3;
    COMMITS.lock().unwrap().clear();
    assert!(page_engine.key(0xff56, 0, false), "Page_Down 应被消费");
    assert!(COMMITS.lock().unwrap().is_empty(), "Page_Down 不提交");
    assert_eq!(
        last_update().3,
        page_start + PAGE_SIZE as i32,
        "Page_Down 翻到下一页"
    );
}

fn idle_punctuation_keys_fall_through() {
    // ⑤ 无菜单（空闲）时 `=`/`-` 仍落标点：不进任何翻页路径。
    let mut idle_engine = paging_engine();
    COMMITS.lock().unwrap().clear();
    assert!(idle_engine.key(0x3d, 0, false), "空闲 `=` 由标点表消费");
    assert_eq!(COMMITS.lock().unwrap().clone(), vec!["=".to_string()]);
    COMMITS.lock().unwrap().clear();
    assert!(idle_engine.key(0x2d, 0, false), "空闲 `-` 由标点表消费");
    assert_eq!(COMMITS.lock().unwrap().clone(), vec!["-".to_string()]);
}

fn ascii_mode_disables_the_paging_rule() {
    // ⑥ **负向对照（用户决定 B 的另一半）**：`ascii_mode` 打开 ⇒ `menu_available` 不成立，
    //    翻页键一律不拦截，`-` 退回上游标点路径（确认组合 + 落标点）。
    //    平台侧无 `ascii_mode` 设置项（它是宿主/rime 标准选项，真机由 fcitx5 的
    //    V 模式直接写入会话上下文），故与参照探针同为「直接设置会话选项」。
    let mut ascii_engine = paging_engine();
    for code in *b"ja" {
        ascii_engine.key(u32::from(code), 0, false);
    }
    let session = ascii_engine.session;
    ascii_engine
        .sessions
        .get_mut(&session)
        .expect("会话")
        .context
        .set_option("ascii_mode", true);
    COMMITS.lock().unwrap().clear();
    assert!(ascii_engine.key(0x2d, 0, false), "`-` 由标点表消费");
    // 期望写成字面量：自指期望（取引擎自己的候选串）分不清「引擎选错字」与「期望跟着错」。
    // 引擎对 `ja` 的首选是「丁」，而不是码表行首「一」——码表行序不是候选序：夹具里
    // `一` 已有首选码 `cd`（`ja` 只是它的第二个码），`丁` 只有 `ja` 一个码，故「丁」排前
    // （见 `goldens/key_sequence/tiger_sentence.codes.txt` 与 `data/tiger_sentence.codes.txt`）。
    assert_eq!(
        COMMITS.lock().unwrap().clone(),
        vec!["丁".to_string(), "-".to_string()],
        "`ascii_mode` 下 `-` 不判为翻页：确认组合（提交首选「丁」）+ 落标点"
    );
}
