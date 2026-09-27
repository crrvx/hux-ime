// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! 学习相关用例：mode 串（含单字重码开关）、索引版本去重与会话生命周期。

use super::super::assets::role;
use super::*;
use hux_core::learning::LearningIndex;

#[test]
fn learning_mode_follows_config_and_rules() {
    // mode 的输入都在配置袋里（Tab 学习 / 高频上限 / 单字重码选项值），由方案自算。
    let mut scheme = fixture_scheme();
    scheme
        .apply_config(&full_bag(&[
            (role::HIGH_FREQ_LIMIT, Value::Count(1500)),
            (role::LEARNING_ON_TAB, Value::Bool(true)),
            (role::ALLOW_DUPLICATE_SINGLE, Value::Bool(true)),
        ]))
        .expect("全角色袋");
    assert_eq!(
        scheme.learning_mode(),
        format!(
            "sentence-v2|rules={}|optimal=1500|dup=1",
            scheme.learning_rules
        )
    );
    assert!(scheme.learning_mode().starts_with("sentence-v2|rules="));

    // 格式归属方案（平台只看不透明串）⇒ 单字重码关闭时的 `dup=0` 也在本文件钉住。
    let mut no_duplicate = fixture_scheme();
    no_duplicate
        .apply_config(&full_bag(&[
            (role::HIGH_FREQ_LIMIT, Value::Count(1500)),
            (role::LEARNING_ON_TAB, Value::Bool(true)),
            (role::ALLOW_DUPLICATE_SINGLE, Value::Bool(false)),
        ]))
        .expect("全角色袋");
    assert_eq!(
        no_duplicate.learning_mode(),
        format!(
            "sentence-v2|rules={}|optimal=1500|dup=0",
            no_duplicate.learning_rules
        )
    );

    let mut off = fixture_scheme();
    off.apply_config(&full_bag(&[
        (role::LEARNING_ON_TAB, Value::Bool(false)),
        (role::ALLOW_DUPLICATE_SINGLE, Value::Bool(true)),
    ]))
    .expect("全角色袋");
    assert_eq!(off.learning_mode(), "", "关闭 Tab 学习 → 空串 = 不记录");
}

#[test]
fn apply_learning_index_records_version_once() {
    // 对应平台原先的 `engine_applies_learning_after_key`：已应用版本属方案状态。
    let mut scheme = fixture_scheme();
    let mut context = Context::new();
    let session = scheme.new_session(&mut context);
    let index = LearningIndex::build(&[], 0.0);
    scheme.apply_learning_index(session, 7, &index);
    assert_eq!(scheme.applied_learning, Some(7));
    scheme.apply_learning_index(session, 7, &index);
    assert_eq!(scheme.applied_learning, Some(7), "同版本不重复应用");
    scheme.apply_learning_index(session, 8, &index);
    assert_eq!(scheme.applied_learning, Some(8));
}

#[test]
fn session_lifecycle_and_learning_events() {
    let mut scheme = fixture_scheme();
    let mut context = Context::new();
    let session = scheme.new_session(&mut context);
    assert!(scheme.take_learning_events(session).is_empty());
    scheme
        .apply_config(&full_bag(&[
            (role::LEARNING_ON_TAB, Value::Bool(true)),
            (role::ALLOW_DUPLICATE_SINGLE, Value::Bool(false)),
        ]))
        .expect("全角色袋");
    assert!(scheme.learning_mode().starts_with("sentence-v2|rules="));
    scheme.set_store_ready(true);
    scheme.reset_session(session, &mut context);
    scheme.free_session(session);
    assert!(scheme.sessions.is_empty());
    // 未知会话：按键/点击/重建均安全转发或忽略。
    assert_eq!(
        scheme
            .process_key(session, &mut context, &KeyEvent::new(0x61, 0), 0.0)
            .expect("process"),
        KeyOutcome::Forward
    );
    assert!(
        !scheme
            .select_candidate(session, &mut context, 0, 0.0)
            .expect("select")
    );
    assert!(scheme.rebuild(session, &mut context, false).is_ok());
}
