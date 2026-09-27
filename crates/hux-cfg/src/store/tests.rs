// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! 选项存储单测：配置页重放、装载、legacy 回退、未知键保留与保存失败属性。

use hux_core::collections::Map;
use hux_core::session::Context;

use super::*;

fn drain_option_events(store: &mut OptionsStore, context: &mut Context) {
    for event in context.drain_events() {
        if let hux_core::session::Event::Option(name) = event {
            store.observe(context, &name);
        }
    }
}

/// 配置页推送的语义是「覆盖并落盘」：改写持久化值、让 sync 可用，且保留未知键。
#[test]
fn set_values_overwrites_the_persisted_value() {
    let dir = hux_test_support::temp_dir("set-values");
    std::fs::write(
        dir.join(OPTIONS_FILE),
        "options:\n  tiger_sentence_early_commit: false\ncustom: 1\n",
    )
    .expect("write");
    let mut store = OptionsStore::load(&dir, &crate::options::test_option_keys());
    let mut context = Context::new();
    store.sync(&mut context);
    assert!(
        !context.get_option("tiger_sentence_early_commit"),
        "options.yaml 的持久化 false 未压制内建缺省：sync 后选项仍为 true"
    );
    // 配置页推送 true ⇒ 覆盖持久化值并落盘（此后 `sync` 不再压制它）。
    let values = Map::from([("tiger_sentence_early_commit".to_string(), true)]);
    assert!(store.set_values(&values), "保存应成功");
    assert_eq!(
        store.value("tiger_sentence_early_commit"),
        Some(true),
        "持久化值应被改写"
    );
    store.sync(&mut context);
    assert!(
        context.get_option("tiger_sentence_early_commit"),
        "配置页推入的 true 未生效：持久化值与 sync 结果仍是 false"
    );
    // 无变化 ⇒ 不落盘也算成功（幂等重放同一份设置）。
    assert!(
        store.set_values(&values),
        "无变化重放同一份设置必须返回 true（没有写入即视为保存成功）"
    );
    let text = std::fs::read_to_string(dir.join(OPTIONS_FILE)).expect("read");
    assert!(text.contains("tiger_sentence_early_commit: true"), "{text}");
    assert!(text.contains("custom: 1"), "未知键保留：{text}");
    // 未登记的角色（不在缺省表里）忽略：不得凭空写入。
    assert!(
        store.set_values(&Map::from([("not_declared".to_string(), true)])),
        "未登记角色必须被忽略而不是报错：调用本身要返回 true"
    );
    assert_eq!(
        store.value("not_declared"),
        None,
        "未登记角色被凭空写进了存储：该键应不可见（None）"
    );
    std::fs::remove_dir_all(&dir).ok();
}

/// 落盘失败必须如实返回 false，不得假装成功让配置页显示已生效。
#[test]
fn set_values_reports_a_failed_save() {
    let dir = hux_test_support::temp_dir("set-values-error");
    // 目标路径是目录 → 写文件失败
    std::fs::create_dir_all(dir.join(OPTIONS_FILE)).expect("blocking dir");
    let mut store = OptionsStore::load(&dir, &crate::options::test_option_keys());
    let values = Map::from([("tiger_sentence_early_commit".to_string(), false)]);
    assert!(!store.set_values(&values), "写失败必须如实返回 false");
    std::fs::remove_dir_all(&dir).ok();
}

/// 启动时从 options.yaml 装载已登记角色，未登记键不参与同步。
#[test]
fn load_applies_stored_options() {
    let dir = hux_test_support::temp_dir("load");
    std::fs::write(
        dir.join(OPTIONS_FILE),
        "options:\n  tiger_sentence_early_commit: false\n  some_other_option: true\ncustom: 1\n",
    )
    .expect("write");
    let mut store = OptionsStore::load(&dir, &crate::options::test_option_keys());
    let mut context = Context::new();
    store.sync(&mut context);
    assert!(
        !context.get_option("tiger_sentence_early_commit"),
        "启动装载未生效：options.yaml 里的 false 没有同步进上下文"
    );
    std::fs::remove_dir_all(&dir).ok();
}

/// 用户改动经 observe 记录并落盘，成功路径不得留下错误属性。
#[test]
fn observe_saves_user_change() {
    let dir = hux_test_support::temp_dir("observe");
    std::fs::write(
        dir.join(OPTIONS_FILE),
        "options:\n  tiger_sentence_early_commit: false\n",
    )
    .expect("write");
    let mut store = OptionsStore::load(&dir, &crate::options::test_option_keys());
    let mut context = Context::new();
    store.sync(&mut context);
    drain_option_events(&mut store, &mut context);
    // 用户改动 → 记录并保存
    context.set_option("tiger_sentence_early_commit", true);
    store.observe(&mut context, "tiger_sentence_early_commit");
    assert_eq!(
        store.options.revision, 1,
        "一次用户改动应恰好记一版：revision 必须从 0 增到 1"
    );
    assert_ne!(
        context.get_property(OPTIONS_ERROR_PROPERTY),
        Some(OPTIONS_ERROR_MESSAGE),
        "保存成功不应留下错误属性"
    );
    let text = std::fs::read_to_string(dir.join(OPTIONS_FILE)).expect("read");
    assert!(text.contains("tiger_sentence_early_commit: true"), "{text}");
    std::fs::remove_dir_all(&dir).ok();
}

/// 保存是就地合并而非重写：未登记的键（含顶层键）必须原样保留。
#[test]
fn save_preserves_unknown_keys() {
    let dir = hux_test_support::temp_dir("preserve");
    std::fs::write(
        dir.join(OPTIONS_FILE),
        "options:\n  some_other_option: true\ncustom: 1\n",
    )
    .expect("write");
    let mut store = OptionsStore::load(&dir, &crate::options::test_option_keys());
    let mut context = Context::new();
    store.sync(&mut context);
    context.set_option("tiger_sentence_early_commit", true);
    store.observe(&mut context, "tiger_sentence_early_commit");
    // 未知键（`options:` 内与其他顶层键）原样保留
    let text = std::fs::read_to_string(dir.join(OPTIONS_FILE)).expect("read");
    assert!(text.contains("some_other_option: true"), "{text}");
    assert!(text.contains("custom: 1"), "{text}");
    std::fs::remove_dir_all(&dir).ok();
}

/// 旧 fcitx5 用户 YAML 只作只读回退来源，迁移后不回写旧文件。
#[test]
fn legacy_user_yaml_is_read_only_fallback() {
    let dir = hux_test_support::temp_dir("legacy");
    std::fs::write(
        dir.join(LEGACY_FILE),
        "var:\n  option:\n    tiger_sentence_allow_duplicate_single: false\n",
    )
    .expect("write");
    let mut store = OptionsStore::load(&dir, &crate::options::test_option_keys());
    assert_eq!(
        store
            .options
            .values
            .get("tiger_sentence_allow_duplicate_single"),
        Some(&false),
        "legacy user.yaml 的 var/option 回退未生效：主文件缺失的键未取到 false"
    );
    let mut context = Context::new();
    store.sync(&mut context);
    assert!(
        !context.get_option("tiger_sentence_allow_duplicate_single"),
        "legacy 回退值未参与 sync：回退补上的键必须与主文件键同等生效"
    );
    std::fs::remove_dir_all(&dir).ok();
}

/// 宿主提供的缺省用来补磁盘上没有的键，补完即参与同步。
#[test]
fn provided_defaults_fill_missing_keys() {
    let dir = hux_test_support::temp_dir("defaults");
    let defaults = Map::from([("tiger_sentence_early_commit".to_string(), false)]);
    let mut store = OptionsStore::load_with_defaults(&dir, defaults);
    let mut context = hux_core::session::Context::new();
    store.sync(&mut context);
    assert!(
        !context.get_option("tiger_sentence_early_commit"),
        "缺失键应回退到传入缺省"
    );
    std::fs::remove_dir_all(&dir).ok();
}

/// 保存失败必须留下错误属性，平台状态栏据此提示用户。
#[test]
fn save_failure_sets_error_property() {
    let dir = hux_test_support::temp_dir("error");
    // 目标路径是目录 → 写文件失败
    std::fs::create_dir_all(dir.join(OPTIONS_FILE)).expect("blocking dir");
    let mut store = OptionsStore::load(&dir, &crate::options::test_option_keys());
    let mut context = Context::new();
    store.sync(&mut context);
    drain_option_events(&mut store, &mut context);
    context.set_option("tiger_sentence_early_commit", false);
    store.observe(&mut context, "tiger_sentence_early_commit");
    assert_eq!(
        context.get_property(OPTIONS_ERROR_PROPERTY),
        Some(OPTIONS_ERROR_MESSAGE),
        "写盘失败未留下错误属性：平台状态栏将无法提示保存失败"
    );
    std::fs::remove_dir_all(&dir).ok();
}
