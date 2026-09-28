// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! 配置页「提前上屏至预编辑」的全链路回归：开启时文本留在本层缓冲、
//! 关闭时同一串按键直接上屏；并核对配置页推送的值活过重启。

use super::*;

// 夹具词库：`ni` 的首选是 `玉`，再打一键证据即成熟（对照 `goldens/lexicon`）。
fn early_commit_dirs() -> Vec<std::path::PathBuf> {
    vec![
        hux_test_support::repo_path("goldens/lexicon"),
        hux_test_support::repo_path("data"),
    ]
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
    let code = b"nihaoma";
    let dir = temp_user_dir("early-commit-to-preedit");

    early_commit_buffers_into_preedit(dir.clone(), code);
    early_commit_off_commits_directly(dir.clone(), code);

    std::fs::remove_dir_all(&dir).ok();
}

fn early_commit_buffers_into_preedit(dir: std::path::PathBuf, code: &[u8]) {
    // ---- 开启（配置页勾选）：早提交进本层缓冲，不上屏 ----
    COMMITS.lock().unwrap().clear();
    UPDATES.lock().unwrap().clear();
    let mut engine = Engine::new_with_dirs(host(), early_commit_dirs(), None, Some(dir.clone()));
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
    let restarted = Engine::new_with_dirs(host(), early_commit_dirs(), None, Some(dir.clone()));
    assert_eq!(
        restarted.option_value("tiger_sentence_early_commit_to_preedit"),
        Some(true),
        "重启后（无会话）仍应读到配置页推送的值"
    );
}

fn early_commit_off_commits_directly(dir: std::path::PathBuf, code: &[u8]) {
    // ---- 关闭（同一入口推 false）：同一串按键直接上屏 ----
    COMMITS.lock().unwrap().clear();
    UPDATES.lock().unwrap().clear();
    let mut engine = Engine::new_with_dirs(host(), early_commit_dirs(), None, Some(dir.clone()));
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
}
