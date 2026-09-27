// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

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
#[test]
fn menu_paging_keys_are_not_shadowed_by_the_punctuation_branch() {
    let _guard = serial();
    COMMITS.lock().unwrap().clear();
    UPDATES.lock().unwrap().clear();
    let mut engine = TestEngine::new(host(), fixture_dirs(), None, None);
    for code in *b"ja" {
        engine.key(u32::from(code), 0, false);
    }
    let first_page = last_update().2;
    assert!(first_page.len() >= 2, "夹具 ja 应有可翻页的多页候选");

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
        selected_before + hux_core::host::DEFAULT_PAGE_SIZE as i32,
        "`=` 应下翻一页（高亮前进一页）"
    );
    assert_eq!(engine.session().context.input(), b"ja", "翻页不改动输入");

    // ② 已翻页后按 `-`：上翻一页、仍不提交。
    assert!(engine.key(0x2d, 0, false), "翻页后 `-` 应被消费（上翻页）");
    assert!(COMMITS.lock().unwrap().is_empty(), "上翻页不得提交组合");
    assert_eq!(last_update().3, selected_before, "`-` 应回到第一页");

    // ③ **首屏**按 `-`：菜单可见即判上翻页（不再要求 `when: paging` 标签）——
    //    先把高亮挪到第 2 项，再按 `-` ⇒ 归零高亮（参照 `PreviousPage` 的三元式）、不提交。
    let mut first_page_engine = TestEngine::new(host(), fixture_dirs(), None, None);
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

    // ④ 显式 `Page_Down` 仍是翻页路径（不受本偏离影响）。
    let mut page_engine = TestEngine::new(host(), fixture_dirs(), None, None);
    for code in *b"ja" {
        page_engine.key(u32::from(code), 0, false);
    }
    let page_start = last_update().3;
    COMMITS.lock().unwrap().clear();
    assert!(page_engine.key(0xff56, 0, false), "Page_Down 应被消费");
    assert!(COMMITS.lock().unwrap().is_empty(), "Page_Down 不提交");
    assert_eq!(
        last_update().3,
        page_start + hux_core::host::DEFAULT_PAGE_SIZE as i32,
        "Page_Down 翻到下一页"
    );

    // ⑤ 无菜单（空闲）时 `=`/`-` 仍落标点：不进任何翻页路径。
    let mut idle_engine = TestEngine::new(host(), fixture_dirs(), None, None);
    COMMITS.lock().unwrap().clear();
    assert!(idle_engine.key(0x3d, 0, false), "空闲 `=` 由标点表消费");
    assert_eq!(COMMITS.lock().unwrap().clone(), vec!["=".to_string()]);
    COMMITS.lock().unwrap().clear();
    assert!(idle_engine.key(0x2d, 0, false), "空闲 `-` 由标点表消费");
    assert_eq!(COMMITS.lock().unwrap().clone(), vec!["-".to_string()]);

    // ⑥ **负向对照（用户决定 B 的另一半）**：`ascii_mode` 打开 ⇒ `menu_available` 不成立，
    //    翻页键一律不拦截，`-` 退回上游标点路径（确认组合 + 落标点）。
    //    平台侧无 `ascii_mode` 设置项（它是宿主/rime 标准选项，真机由 fcitx5 的
    //    V 模式直接写入会话上下文），故与参照探针同为「直接设置会话选项」。
    let mut ascii_engine = TestEngine::new(host(), fixture_dirs(), None, None);
    for code in *b"ja" {
        ascii_engine.key(u32::from(code), 0, false);
    }
    let ascii_sentence = last_update().2.first().cloned().expect("ja 候选");
    let session = ascii_engine.session;
    ascii_engine
        .sessions
        .get_mut(&session)
        .expect("会话")
        .context
        .set_option("ascii_mode", true);
    COMMITS.lock().unwrap().clear();
    assert!(ascii_engine.key(0x2d, 0, false), "`-` 由标点表消费");
    assert_eq!(
        COMMITS.lock().unwrap().clone(),
        vec![ascii_sentence, "-".to_string()],
        "`ascii_mode` 下 `-` 不判为翻页：确认组合 + 落标点"
    );
}

#[test]
fn modified_keys_pass_through() {
    let _guard = serial();
    let mut engine = TestEngine::new(host(), fixture_dirs(), None, None);
    assert!(!engine.key(u32::from(b'a'), FCITX_CTRL, false)); // Ctrl+a 交宿主
}

#[test]
fn key_releases_pass_through() {
    let _guard = serial();
    let mut engine = TestEngine::new(host(), fixture_dirs(), None, None);
    assert!(!engine.key(u32::from(b'a'), 0, true));
}

#[test]
fn idle_return_passes_through() {
    let _guard = serial();
    let mut engine = TestEngine::new(host(), fixture_dirs(), None, None);
    assert!(!engine.key(0xff0d, 0, false)); // Return 空闲交宿主
}

#[test]
fn idle_editing_keys_pass_through() {
    let _guard = serial();
    let mut engine = TestEngine::new(host(), fixture_dirs(), None, None);
    // BackSpace/Delete/Left/Right/Up/Down/Home/End/Page_Up/Page_Down/Escape/Tab
    for keysym in [
        0xff08, 0xffff, 0xff51, 0xff53, 0xff52, 0xff54, 0xff50, 0xff57, 0xff55, 0xff56, 0xff1b,
        0xff09,
    ] {
        assert!(
            !engine.key(keysym, 0, false),
            "keysym {keysym:#x} 空闲时应交宿主"
        );
    }
}

#[test]
fn composing_left_right_move_caret_and_toggle_candidates() {
    let _guard = serial();
    UPDATES.lock().unwrap().clear();
    let mut engine = TestEngine::new(host(), fixture_dirs(), None, None);
    assert!(engine.key(u32::from(b'a'), 0, false));
    assert!(engine.key(u32::from(b'b'), 0, false));
    // ←：光标左移；组合按 caret 前缀重建（候选清空）
    assert!(engine.key(0xff51, 0, false), "组合中 Left 应被消费");
    let (preedit, cursor, candidates, _, _, _) = last_update();
    assert_eq!(preedit, "ab");
    assert_eq!(cursor, 1);
    assert!(candidates.is_empty(), "光标在输入中间时无候选");
    // →：回到末尾，候选恢复
    assert!(engine.key(0xff53, 0, false));
    let (_, cursor, candidates, _, _, _) = last_update();
    assert_eq!(cursor, 2);
    assert_eq!(candidates, vec!["甲".to_string(), "乙".to_string()]);
}

#[test]
fn composing_up_down_move_highlight() {
    let _guard = serial();
    UPDATES.lock().unwrap().clear();
    let mut engine = TestEngine::new(host(), fixture_dirs(), None, None);
    assert!(engine.key(u32::from(b'a'), 0, false));
    assert!(engine.key(u32::from(b'b'), 0, false));
    // ↓：高亮下移；↑ 到首项
    assert!(engine.key(0xff54, 0, false));
    let (_, _, _, selected, _, _) = last_update();
    assert_eq!(selected, 1);
    assert!(engine.key(0xff52, 0, false));
    let (_, _, _, selected, _, _) = last_update();
    assert_eq!(selected, 0);
}

#[test]
fn composing_backspace_deletes_input_and_clears_composition() {
    let _guard = serial();
    UPDATES.lock().unwrap().clear();
    let mut engine = TestEngine::new(host(), fixture_dirs(), None, None);
    assert!(engine.key(u32::from(b'a'), 0, false));
    assert!(engine.key(u32::from(b'b'), 0, false));
    // 退格：删除输入
    assert!(engine.key(0xff08, 0, false));
    let (preedit, cursor, candidates, _, _, _) = last_update();
    assert_eq!(preedit, "a");
    assert_eq!(cursor, 1);
    assert!(candidates.is_empty());
    // 再退格清空组合
    assert!(engine.key(0xff08, 0, false));
    let (preedit, _, candidates, _, _, _) = last_update();
    assert!(preedit.is_empty());
    assert!(candidates.is_empty());
}

#[test]
fn punctuation_commits_when_idle() {
    let _guard = serial();
    COMMITS.lock().unwrap().clear();
    let mut engine = TestEngine::new(host(), fixture_dirs(), None, None);
    // symbols.yaml half_shape："." → 。
    assert!(engine.key(0x2e, 0, false), "period 应被消费");
    assert_eq!(COMMITS.lock().unwrap().last().unwrap(), "。");
}

#[test]
fn punctuation_appends_to_composition() {
    let _guard = serial();
    COMMITS.lock().unwrap().clear();
    let mut engine = TestEngine::new(host(), fixture_dirs(), None, None);
    assert!(engine.key(u32::from(b'a'), 0, false));
    assert!(engine.key(u32::from(b'b'), 0, false));
    assert!(engine.key(0x2c, 0, false), "comma 应被消费");
    // 参照 `abad411`：菜单可见时处理器先确认组合（librime `ConcreteEngine::OnSelect`
    // 在 `_auto_commit` 下同步 `Commit()`），标点随后独立落字；提交文本合计不变。
    assert_eq!(
        COMMITS.lock().unwrap().clone(),
        vec!["甲".to_string(), "，".to_string()]
    );
    assert!(engine.session().context.input().is_empty());
}

#[test]
fn punctuation_pair_alternates() {
    let _guard = serial();
    COMMITS.lock().unwrap().clear();
    let mut engine = TestEngine::new(host(), fixture_dirs(), None, None);
    // apostrophe：'‘' / '’'
    for text in ["‘", "’"] {
        assert!(engine.key(0x27, 0, false));
        assert_eq!(COMMITS.lock().unwrap().last().unwrap(), text);
    }
}

#[test]
fn punctuation_passes_unmapped_space() {
    let _guard = serial();
    let mut engine = TestEngine::new(host(), fixture_dirs(), None, None);
    assert!(!engine.key(0x20, 0, false), "空闲空格交宿主");
}

#[test]
fn uppercase_commits_composition_and_requests_forward() {
    let _guard = serial();
    // 用户报告：组合中收到大写字母时，应先上屏当前候选（而非把字母插到预编辑之前）。
    // 核心语义保持「提交 + 不消费」（同 librime）；宿主层据 `forward_after_commit`
    // 消费该键并以 forwardKey 重发，保证「候选 → 字母」送达顺序。
    COMMITS.lock().unwrap().clear();
    UPDATES.lock().unwrap().clear();
    let mut engine = TestEngine::new(host(), fixture_dirs(), None, None);
    assert!(engine.key(u32::from(b'a'), 0, false));
    assert!(!engine.forward_after_commit, "普通输入不应请求转发");
    assert!(engine.key(u32::from(b'b'), 0, false));
    assert!(
        !engine.key(0x41, FCITX_SHIFT, false),
        "大写字母应交宿主（不消费）"
    );
    assert!(
        engine.forward_after_commit,
        "提交且未消费 → 宿主应消费并重发该键"
    );
    assert_eq!(COMMITS.lock().unwrap().last().unwrap(), "甲");
    assert!(
        engine.session().context.input().is_empty(),
        "组合已提交并清空"
    );
}

/// `forward_after_commit` 是**粘性输出标志**，未知 / 已释放会话
/// 不得沿用上一次按键的取值。此前 `with_session` 返回 `None` 时直接 `unwrap_or(false)`，
/// 标志保留 ⇒ `hux_engine_key` 只回 `HUX_KEY_FORWARD_AFTER_COMMIT`（无 CONSUMED），
/// 宿主会 `filterAndAccept` + `forwardKey` 一个并不存在的提交。
#[test]
fn unknown_session_does_not_reuse_the_sticky_forward_flag() {
    let _guard = serial();
    let mut engine = TestEngine::new(host(), fixture_dirs(), None, None);
    // 先造出「提交 + 未消费」（组合中按大写字母）：此时转发位为真。
    engine.key(u32::from(b'a'), 0, false);
    engine.key(u32::from(b'b'), 0, false);
    assert!(!engine.key(0x41, FCITX_SHIFT, false));
    assert!(engine.forward_after_commit);
    // 未知会话按键：必须清位（且不消费）。
    let unknown = engine.session + 1000;
    assert!(!engine.engine.key(unknown, u32::from(b'x'), 0, false));
    assert!(
        !engine.engine.forward_after_commit,
        "未知会话按键不得沿用上一次的转发位"
    );
    // 候选点击路径同理（重新置位后再走未知会话）。
    assert!(engine.key(u32::from(b'a'), 0, false));
    assert!(engine.key(u32::from(b'b'), 0, false));
    assert!(!engine.key(0x41, FCITX_SHIFT, false));
    assert!(engine.forward_after_commit);
    assert!(!engine.engine.select_candidate(unknown, 0));
    assert!(
        !engine.engine.forward_after_commit,
        "未知会话的候选点击不得沿用上一次的转发位"
    );
    // 已释放会话同理。
    assert!(engine.key(u32::from(b'a'), 0, false));
    assert!(engine.key(u32::from(b'b'), 0, false));
    assert!(!engine.key(0x41, FCITX_SHIFT, false));
    let released = engine.session;
    engine.engine.session_free(released);
    assert!(!engine.engine.key(released, u32::from(b'x'), 0, false));
    assert!(
        !engine.engine.forward_after_commit,
        "已释放会话按键不得沿用上一次的转发位"
    );
}

#[test]
fn idle_uppercase_does_not_request_forward() {
    let _guard = serial();
    let mut engine = TestEngine::new(host(), fixture_dirs(), None, None);
    assert!(!engine.key(0x41, FCITX_SHIFT, false));
    assert!(!engine.forward_after_commit);
}

/// 配置页「提前上屏至预编辑」的全链路回归：C++ 壳（`applyConfig` → `hux_engine_apply_settings`）
/// → 引擎（`apply_settings` 写回选项存储并 `sync` 进会话上下文）→ 方案（`submit_early` 进缓冲）。
///
/// 该开关此前**只在方案的 `submit_early` 里被读一次**，上面任一段断开都表现为「勾选后没有效果」；
/// 故判据取宿主可见的两端（不是 `set_option` 的桩）：开启时**没有 host_commit**、文本留在
/// `buffered_text`（并出现在预编辑里）；关闭时同一串按键**直接上屏**。两半互相钉桩——
/// 开关被忽略（两半都提交）或恒开（两半都不提交）都会失败。
#[test]
fn config_page_early_commit_to_preedit_buffers_instead_of_committing() {
    let _guard = serial();
    // 夹具词库：`ni` 的首选是 `玉`，再打一键证据即成熟（对照 `goldens/lexicon`）。
    let dirs = || {
        vec![
            hux_test_support::repo_path("goldens/lexicon"),
            hux_test_support::repo_path("data"),
        ]
    };
    let code = b"nihaoma";

    // ---- 开启（配置页勾选）：早提交进本层缓冲，不上屏 ----
    let dir = temp_user_dir("early-commit-to-preedit");
    COMMITS.lock().unwrap().clear();
    UPDATES.lock().unwrap().clear();
    let mut engine = Engine::new_with_dirs(host(), dirs(), None, Some(dir.clone()));
    let options = HuxOptions {
        early_commit: 1,
        early_commit_to_preedit: 1,
        ..ffi_options()
    };
    assert_eq!(
        unsafe { hux_engine_apply_settings(&mut engine, &options) },
        1
    );
    let session = engine.session_new();
    assert!(
        engine
            .sessions
            .get(&session)
            .expect("session")
            .context
            .get_option("tiger_sentence_early_commit_to_preedit"),
        "配置页推送 true 后会话上下文必须为 true（否则方案侧永远读到 false）"
    );
    let mut buffered = String::new();
    for ch in code {
        engine.key(session, u32::from(*ch), 0, false);
        buffered = engine
            .scheme
            .buffered_text(&engine.sessions.get(&session).expect("session").context);
        if !buffered.is_empty() {
            break;
        }
    }
    assert_eq!(buffered, "玉", "证据成熟的早提交文本应进本层缓冲");
    assert!(
        COMMITS.lock().unwrap().is_empty(),
        "缓冲分支不得交给应用：{:?}",
        COMMITS.lock().unwrap()
    );
    let (preedit, ..) = last_update();
    assert!(
        preedit.contains("玉"),
        "缓冲文本应以预编辑呈现：{preedit:?}"
    );
    // 配置页的值须活过重启（引擎把它写回 `options.yaml`，宿主 `adoptStoredRuntimeOptions`
    // 据此补齐 schema）：否则下次启动会把配置页的改动静默压回。
    let restarted = Engine::new_with_dirs(host(), dirs(), None, Some(dir.clone()));
    assert_eq!(
        restarted.option_value("tiger_sentence_early_commit_to_preedit"),
        Some(true),
        "重启后（无会话）仍应读到配置页推送的值"
    );

    // ---- 关闭（同一入口推 false）：同一串按键直接上屏 ----
    COMMITS.lock().unwrap().clear();
    UPDATES.lock().unwrap().clear();
    let mut engine = Engine::new_with_dirs(host(), dirs(), None, Some(dir.clone()));
    let options = HuxOptions {
        early_commit: 1,
        early_commit_to_preedit: 0,
        ..ffi_options()
    };
    assert_eq!(
        unsafe { hux_engine_apply_settings(&mut engine, &options) },
        1
    );
    let session = engine.session_new();
    for ch in code {
        engine.key(session, u32::from(*ch), 0, false);
        if !COMMITS.lock().unwrap().is_empty() {
            break;
        }
    }
    assert_eq!(
        COMMITS.lock().unwrap().clone(),
        vec!["玉".to_string()],
        "关闭时早提交应直接交给应用"
    );
    std::fs::remove_dir_all(&dir).ok();
}
