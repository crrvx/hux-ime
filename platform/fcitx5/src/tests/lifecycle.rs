// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! 引擎与会话生命周期：学习库启用与生效、预编辑 / 候选、会话隔离与释放。
//!
//! 另有设置下发、候选点击与面板清理；夹具与 `serial()` 串行约定见父模块 `tests.rs`。

use super::*;

#[test]
fn engine_enables_learning_store() {
    let _guard = serial();
    let dir = temp_user_dir("learning");
    let engine = TestEngine::new(host(), fixture_dirs(), None, Some(dir.clone()));
    assert!(
        engine.engine.learning.store_ready(),
        "用户目录可用时学习库应就绪"
    );
    // mode 串对平台是**不透明**的（§5）——平台只承诺「原样使用方案自算的串」，
    // 格式由方案自己的用例钉住（`tiger` 的 `learning_mode_follows_config_and_rules`）。
    let mode = engine.engine.scheme.learning_mode();
    assert!(!mode.is_empty(), "学习库就绪时 mode 串非空");
    assert!(
        dir.join(format!(
            "{}.userdb",
            learning_store::store_name(hux_scheme_tiger::scheme::SCHEME_ID)
        ))
        .is_dir()
    );
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn engine_applies_learning_after_key() {
    // 学习索引的「已应用版本」属方案内部状态（见 `crates/hux-scheme/tiger` 的单测）；
    // 平台侧验证接线**且索引确实作用到解码器**（只断言
    // `store_ready` 与 mode 非空，删掉 `Engine::finish` 里的 `apply_learning_index`
    // 调用仍全绿）。判据只用**已有可观测面**：宿主提交点落库一条纠错证据后，
    // 同码候选的排序必须随库版本变化而变（候选快照即宿主真实可见的输出，
    // 不为此新增测试专用 API）。
    let _guard = serial();
    let dir = temp_user_dir("learning-apply");
    UPDATES.lock().unwrap().clear();
    COMMITS.lock().unwrap().clear();
    let mut engine = TestEngine::new(host(), fixture_dirs(), None, Some(dir.clone()));
    assert!(engine.engine.learning.store_ready(), "学习库应就绪");
    // 只看「非空」，不看具体格式（格式归方案自己的用例）。
    assert!(
        !engine.engine.scheme.learning_mode().is_empty(),
        "学习库就绪时 mode 串非空"
    );

    let baseline = baseline_candidate_order(&mut engine);
    committed_candidate_is_recorded_as_a_learning_event(&mut engine);
    learned_index_reorders_the_candidates(&mut engine, &baseline);
    std::fs::remove_dir_all(&dir).ok();
}

/// 打 `abab` 并返回基线候选序（两条同码 `ab` 边）。
fn baseline_candidate_order(engine: &mut TestEngine) -> Vec<String> {
    // 基线：`abab`（两条同码 `ab` 边）的候选序。
    for code in *b"abab" {
        engine.key(u32::from(code), 0, false);
    }
    let baseline = last_update().2;
    assert_eq!(
        baseline,
        vec![
            "甲甲".to_string(),
            "乙甲".into(),
            "甲乙".into(),
            "乙乙".into()
        ],
        "夹具基线候选序"
    );
    baseline
}

/// Tab 锁定第 2 个候选 → 大写 A 交宿主链提交：宿主提交点写入纠错学习事件。
fn committed_candidate_is_recorded_as_a_learning_event(engine: &mut TestEngine) {
    // Tab 锁定第 2 个候选 → 大写 A 交宿主链提交：宿主提交点写入纠错学习事件。
    let before = engine.engine.learning.index_version();
    assert!(engine.key(0xff09, 0, false), "Tab 应被消费");
    engine.key(0x41, 0, false);
    assert_eq!(COMMITS.lock().unwrap().last().unwrap(), "乙甲");
    assert_ne!(
        engine.engine.learning.index_version(),
        before,
        "宿主提交应写入学习库（库版本变化）"
    );
}

/// 学习后重打同一串：候选序必须随新索引而变（索引未被应用到解码器时保持不变）。
fn learned_index_reorders_the_candidates(engine: &mut TestEngine, baseline: &[String]) {
    // 学习后重打同一串：候选序必须随新索引而变——索引未被应用到解码器时保持不变。
    UPDATES.lock().unwrap().clear();
    for code in *b"abab" {
        engine.key(u32::from(code), 0, false);
    }
    let learned = last_update().2;
    assert_ne!(
        learned, baseline,
        "学习索引必须作用到解码器排序（仅 store_ready / mode 非空不足为证）"
    );
    assert_eq!(
        learned,
        vec![
            "乙乙".to_string(),
            "乙甲".into(),
            "甲乙".into(),
            "甲甲".into()
        ],
        "学习后候选序（学的 `乙甲` 一侧上浮）"
    );
    assert!(engine.engine.learning.store_ready(), "学习库应保持就绪");
}

#[test]
fn typing_shows_preedit_and_candidates() {
    let _guard = serial();
    UPDATES.lock().unwrap().clear();
    let mut engine = TestEngine::new(host(), fixture_dirs(), None, None);
    // 「甲/乙」共用码 ab：输入两个键后出现候选。
    assert!(engine.key(u32::from(b'a'), 0, false));
    assert!(engine.key(u32::from(b'b'), 0, false));
    assert_eq!(engine.session().context.input(), b"ab");
    let (preedit, cursor, candidates, selected, _, _) = last_update();
    assert_eq!(preedit, "ab");
    assert_eq!(cursor, 2);
    assert_eq!(candidates, vec!["甲".to_string(), "乙".to_string()]);
    assert_eq!(selected, 0);
}

/// 会话隔离：两个输入上下文各自维护组合，互不影响。
#[test]
fn sessions_are_isolated() {
    let _guard = serial();
    COMMITS.lock().unwrap().clear();
    UPDATES.lock().unwrap().clear();
    let mut engine = Engine::new_with_dirs(host(), fixture_dirs(), None, None);
    let first = engine.session_new();
    let second = engine.session_new();
    assert_ne!(first, second, "会话 id 应递增且不同");
    for code in *b"ab" {
        assert!(engine.key(first, u32::from(code), 0, false));
    }
    for code in *b"ja" {
        assert!(engine.key(second, u32::from(code), 0, false));
    }
    assert_eq!(engine.sessions[&first].context.input(), b"ab");
    assert_eq!(engine.sessions[&second].context.input(), b"ja");
    // 一个会话上屏不影响另一个。
    assert!(engine.key(first, 0x20, 0, false));
    assert_eq!(COMMITS.lock().unwrap().last().unwrap(), "甲");
    assert_eq!(engine.sessions[&second].context.input(), b"ja");
}

/// 重置直接丢弃组合（不提交），面板清空；失焦的提交由核心/前端处理（不在本层）。
#[test]
fn reset_discards_composition_without_commit() {
    let _guard = serial();
    COMMITS.lock().unwrap().clear();
    UPDATES.lock().unwrap().clear();
    let mut engine = Engine::new_with_dirs(host(), fixture_dirs(), None, None);
    let session = engine.session_new();
    for code in *b"ab" {
        assert!(engine.key(session, u32::from(code), 0, false));
    }
    engine.reset(session);
    assert!(COMMITS.lock().unwrap().is_empty(), "重置不应提交组合");
    assert!(engine.sessions[&session].context.input().is_empty());
    let (preedit, _, candidates, _, aux_up, aux_down) = last_update();
    assert!(preedit.is_empty(), "预编辑应清空");
    assert!(candidates.is_empty(), "候选应清空");
    assert!(aux_up.is_empty() && aux_down.is_empty(), "辅助两排应清空");
}

/// 会话释放：销毁后按键与点击被忽略。
#[test]
fn session_free_drops_state() {
    let _guard = serial();
    let mut engine = Engine::new_with_dirs(host(), fixture_dirs(), None, None);
    let session = engine.session_new();
    assert!(engine.key(session, u32::from(b'a'), 0, false));
    engine.session_free(session);
    assert!(!engine.sessions.contains_key(&session));
    assert!(
        !engine.key(session, u32::from(b'a'), 0, false),
        "已释放会话忽略按键"
    );
    assert!(!engine.select_candidate(session, 0), "已释放会话忽略点击");
}

/// 运行时开关（状态菜单）作用于全部会话；新会话继承当前值（含无存储情形）。
#[test]
fn runtime_option_applies_to_all_sessions() {
    let _guard = serial();
    let mut engine = Engine::new_with_dirs(host(), fixture_dirs(), None, None);
    let first = engine.session_new();
    let second = engine.session_new();
    assert!(engine.set_option_value("full_shape", true));
    assert!(engine.sessions[&first].context.get_option("full_shape"));
    assert!(engine.sessions[&second].context.get_option("full_shape"));
    let third = engine.session_new();
    assert!(
        engine.sessions[&third].context.get_option("full_shape"),
        "新会话应继承当前开关"
    );
}

/// 配置变更（设置页）作用于已存在的会话。
#[test]
fn apply_settings_reaches_existing_sessions() {
    let _guard = serial();
    let mut engine = Engine::new_with_dirs(host(), fixture_dirs(), None, None);
    let session = engine.session_new();
    engine.apply_settings(Settings {
        full_shape: true,
        ascii_punct: true,
        ..Default::default()
    });
    assert!(engine.sessions[&session].context.get_option("full_shape"));
    assert!(engine.sessions[&session].context.get_option("ascii_punct"));
}

#[test]
fn space_commits_highlighted_candidate() {
    let _guard = serial();
    COMMITS.lock().unwrap().clear();
    let mut engine = TestEngine::new(host(), fixture_dirs(), None, None);
    assert!(engine.key(u32::from(b'a'), 0, false));
    assert!(engine.key(u32::from(b'b'), 0, false));
    assert!(engine.key(0x20, 0, false)); // space
    assert_eq!(COMMITS.lock().unwrap().last().unwrap(), "甲");
    assert!(engine.session().context.input().is_empty());
}

/// 候选点击（面板 `CandidateWord::select`）：按全局索引选中并上屏。
#[test]
fn candidate_click_commits_selected_candidate() {
    let _guard = serial();
    COMMITS.lock().unwrap().clear();
    UPDATES.lock().unwrap().clear();
    let mut engine = TestEngine::new(host(), fixture_dirs(), None, None);
    for code in *b"ab" {
        engine.key(u32::from(code), 0, false);
    }
    let (_, _, candidates, _, _, _) = last_update();
    assert!(candidates.len() >= 2, "夹具 ab 应有至少 2 个候选");
    let second = candidates[1].clone();
    assert!(engine.select_candidate(1), "点击页内候选应被处理");
    assert_eq!(COMMITS.lock().unwrap().last().unwrap(), &second);
    assert!(
        engine.session().context.input().is_empty(),
        "上屏后组合应清空"
    );
}

/// 候选点击越界（如列表已被刷新）：忽略，不产生提交、组合不变。
#[test]
fn candidate_click_out_of_range_ignored() {
    let _guard = serial();
    COMMITS.lock().unwrap().clear();
    UPDATES.lock().unwrap().clear();
    let mut engine = TestEngine::new(host(), fixture_dirs(), None, None);
    for code in *b"ab" {
        engine.key(u32::from(code), 0, false);
    }
    assert!(!engine.select_candidate(99));
    assert!(COMMITS.lock().unwrap().is_empty());
    assert_eq!(engine.session().context.input(), b"ab");
}

#[test]
fn reset_clears_panel() {
    let _guard = serial();
    UPDATES.lock().unwrap().clear();
    let mut engine = TestEngine::new(host(), fixture_dirs(), None, None);
    engine.key(u32::from(b'a'), 0, false);
    engine.reset();
    let (preedit, _, candidates, _, _, _) =
        UPDATES.lock().unwrap().last().cloned().expect("update");
    assert!(preedit.is_empty());
    assert!(candidates.is_empty());
    assert!(engine.session().context.input().is_empty());
}
