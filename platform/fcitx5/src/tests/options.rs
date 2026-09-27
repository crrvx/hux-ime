// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;

/// 运行时开关（状态菜单）：白名单读写往返，未知选项拒绝。
#[test]
fn runtime_option_roundtrip_and_whitelist() {
    let _guard = serial();
    let mut engine = TestEngine::new(host(), fixture_dirs(), None, None);
    // 循环体在返回空列表时一次都不执行 ⇒ 角色表整体失效会「空转通过」，
    // 故先钉住长度（与 ABI 角色序同源）。
    let roles = engine.runtime_options().to_vec();
    assert_eq!(
        roles.len(),
        hux_cfg::roles::RUNTIME_OPTION_ROLES.len(),
        "运行时选项表必须与 RUNTIME_OPTION_ROLES 等长：{roles:?}"
    );
    for name in &roles {
        let value = engine.option_value(name).expect("白名单选项");
        assert!(engine.set_option_value(name, !value), "{name} 应可设置");
        assert_eq!(engine.option_value(name), Some(!value));
    }
    assert_eq!(engine.option_value("not_an_option"), None);
    assert!(!engine.set_option_value("not_an_option", true));
}

/// 运行时开关落盘到 `tiger_sentence.options.yaml`，重启后保持。
#[test]
fn runtime_option_persists_to_store() {
    let _guard = serial();
    let dir = temp_user_dir("runtime-option");
    let mut engine = TestEngine::new(host(), fixture_dirs(), None, Some(dir.clone()));
    assert!(engine.set_option_value("full_shape", true));
    let text = std::fs::read_to_string(dir.join(OPTIONS_FILE)).expect("options.yaml");
    assert!(text.contains("full_shape: true"), "{text}");
    let restarted = TestEngine::new(host(), fixture_dirs(), None, Some(dir.clone()));
    assert!(
        restarted.session().context.get_option("full_shape"),
        "重启后应保持"
    );
    std::fs::remove_dir_all(&dir).ok();
}

/// 配置页与状态菜单的开关是**单一事实来源**：配置页推送覆盖 `options.yaml` 的同名旧值
/// （此前该旧值会压制设置值，「配置页改了不生效」），并被写回存储供状态菜单读取。
///
/// 判据用宿主可见的输出（标点全/半角）而非上下文选项本身：`/` 在夹具标点表里
/// 半角 = `、`、全角 = `／`。
#[test]
fn apply_settings_overrides_store_values_and_immediately_applies() {
    let _guard = serial();
    COMMITS.lock().unwrap().clear();
    let dir = temp_user_dir("settings-order");
    // 状态菜单此前把「全角标点」关掉了（`options.yaml` 里是 false）。
    std::fs::write(dir.join(OPTIONS_FILE), "options:\n  full_shape: false\n").expect("write");
    let mut engine = TestEngine::new(host(), fixture_dirs(), None, Some(dir.clone()));
    assert!(!engine.session().context.get_option("full_shape"));
    assert!(engine.key(0x2f, 0, false), "slash 应被消费");
    assert_eq!(COMMITS.lock().unwrap().last().unwrap(), "、");

    // 配置页打开全角标点：必须立即生效（不再被 options.yaml 压制）。
    engine.apply_settings(Settings {
        full_shape: true,
        ..Default::default()
    });
    assert!(
        engine.session().context.get_option("full_shape"),
        "配置页推送应即时生效"
    );
    assert!(engine.key(0x2f, 0, false));
    assert_eq!(
        COMMITS.lock().unwrap().last().unwrap(),
        "／",
        "行为应随配置页推送立即变化"
    );
    // 状态菜单读同一份值；`options.yaml` 也被写回（另一侧立刻反映）。
    assert_eq!(engine.option_value("full_shape"), Some(true));
    let text = std::fs::read_to_string(dir.join(OPTIONS_FILE)).expect("options.yaml");
    assert!(text.contains("full_shape: true"), "{text}");
    std::fs::remove_dir_all(&dir).ok();
}

/// 状态菜单改动 → 配置读取侧反映：同一个引擎里 `option_value`（配置页读的运行时值）
/// 与 `options.yaml`（持久化值）都立即是新值，重新构造（模拟配置页重开/重启）也一致。
#[test]
fn store_toggle_is_reflected_by_the_config_read_path() {
    let _guard = serial();
    let dir = temp_user_dir("settings-mirror");
    let mut engine = TestEngine::new(host(), fixture_dirs(), None, Some(dir.clone()));
    assert!(engine.set_option_value("tiger_sentence_early_commit", false));
    assert_eq!(
        engine.option_value("tiger_sentence_early_commit"),
        Some(false),
        "状态菜单改动应立即可读"
    );
    // 配置页读到的等价路径：存储值（宿主 schema 缺省时由引擎补齐，见 `hux.cpp`）。
    let store = engine.options.as_ref().expect("存储");
    assert_eq!(store.value("tiger_sentence_early_commit"), Some(false));
    let reopened = TestEngine::new(host(), fixture_dirs(), None, Some(dir.clone()));
    assert_eq!(
        reopened.option_value("tiger_sentence_early_commit"),
        Some(false),
        "配置读取（重新打开）应反映同一值"
    );
    std::fs::remove_dir_all(&dir).ok();
}

/// FFI：运行时开关读写（含未知选项）。
#[test]
fn ffi_runtime_option_roundtrip() {
    let _guard = serial();
    let dir = temp_user_dir("ffi-option");
    let mut engine = TestEngine::new(host(), fixture_dirs(), None, Some(dir.clone()));
    let name = CString::new("full_shape").expect("name");
    assert_eq!(
        unsafe { hux_engine_option_value(&mut *engine, name.as_ptr()) },
        0
    );
    assert_eq!(
        unsafe { hux_engine_set_option(&mut *engine, name.as_ptr(), 1) },
        1
    );
    assert_eq!(
        unsafe { hux_engine_option_value(&mut *engine, name.as_ptr()) },
        1
    );
    let unknown = CString::new("not_an_option").expect("name");
    assert_eq!(
        unsafe { hux_engine_option_value(&mut *engine, unknown.as_ptr()) },
        -1
    );
    assert_eq!(
        unsafe { hux_engine_set_option(&mut *engine, unknown.as_ptr(), 1) },
        0
    );
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn apply_settings_switches_context_options() {
    let _guard = serial();
    let mut engine = TestEngine::new(host(), fixture_dirs(), None, None);
    engine.apply_settings(Settings {
        full_shape: true,
        ascii_punct: true,
        ..Default::default()
    });
    assert!(engine.session().context.get_option("full_shape"));
    assert!(engine.session().context.get_option("ascii_punct"));
}

#[test]
fn apply_settings_disables_learning_mode() {
    let _guard = serial();
    let mut engine = TestEngine::new(host(), fixture_dirs(), None, None);
    engine.apply_settings(Settings {
        learning_on_tab: false,
        ..Default::default()
    });
    assert!(
        engine.engine.scheme.learning_mode().is_empty(),
        "关闭 Tab 学习 → 学习 mode 为空"
    );
}

/// 高频字上限经**配置页入口**下发即重建词库：引擎先按内建缺省（1500）装配方案，
/// 宿主随后才 `apply_settings`；只在 `load` 时生效等于「设置永不生效」。
///
/// 判据用宿主可见的候选列表（不是词库内部状态）：夹具码表里 `jvn` = 主码 `华` +
/// 高频字 `仍` 的非主码，上限放开后 `仍` 才能参与组句。
#[test]
fn apply_settings_rebuilds_the_lexicon_for_a_new_high_freq_limit() {
    let _guard = serial();
    UPDATES.lock().unwrap().clear();
    // 夹具词库（`goldens/lexicon`：码表 + 字频 + 白名单），自带字频文件才谈得上过滤。
    let dirs = vec![hux_test_support::repo_path("goldens/lexicon")];
    let mut engine = TestEngine::new(host(), dirs, None, Some(temp_user_dir("high-freq")));
    let type_code = |engine: &mut TestEngine, code: &[u8]| {
        for key in code {
            engine.key(u32::from(*key), 0, false);
        }
        let candidates = last_update().2;
        engine.reset();
        candidates
    };
    assert_eq!(
        type_code(&mut engine, b"jvn"),
        vec!["华".to_string()],
        "缺省上限 1500 下 `仍` 的非主码被过滤"
    );
    // 配置页把上限调成 0（不限制）→ 词库必须重建，解码结果随之变化。
    engine.apply_settings(Settings {
        high_freq_limit: 0,
        ..Default::default()
    });
    assert_eq!(
        type_code(&mut engine, b"jvn"),
        vec!["华".to_string(), "仍".to_string()],
        "放开上限后 `仍` 应参与组句"
    );
    // 再收紧回 1500 → 同样重建（不是「只放开一次」）。
    engine.apply_settings(Settings {
        high_freq_limit: 1500,
        ..Default::default()
    });
    assert_eq!(type_code(&mut engine, b"jvn"), vec!["华".to_string()]);
}

/// 设置缺省不得被当成「用户改动」落盘：否则 `options.yaml` 会把设置值钉死，
/// 之后配置页对这些开关永久失效（参照 `M.options.sync` 的 `live.syncing` 抑制语义）。
#[test]
fn setting_defaults_are_not_persisted_as_user_options() {
    let _guard = serial();
    let dir = temp_user_dir("options-seed");
    let mut engine = TestEngine::new(host(), fixture_dirs(), None, Some(dir.clone()));
    // 首个按键会排空会话初始化时入队的选项事件（原缺陷在此落盘 5 个键）。
    engine.key(u32::from(b'a'), 0, false);
    let path = dir.join(hux_cfg::OPTIONS_FILE);
    assert!(
        !path.exists(),
        "设置缺省不应写入 options.yaml：{:?}",
        std::fs::read_to_string(&path).ok()
    );
    // 配置页改动必须生效（不得被 options.yaml 回滚）。
    engine.apply_settings(Settings {
        full_shape: true,
        ..Default::default()
    });
    assert!(
        engine.session().context.get_option("full_shape"),
        "配置页改动应生效"
    );
    // 状态菜单改动仍须落盘（其后配置页推送也写同一份文件，两处不互相压制）。
    assert!(engine.set_option_value("tiger_sentence_early_commit", false));
    let text = std::fs::read_to_string(&path).expect("options.yaml");
    assert!(
        text.contains("tiger_sentence_early_commit: false"),
        "{text}"
    );
    assert!(
        text.contains("full_shape: true"),
        "配置页推送应写回同一份存储：{text}"
    );
    std::fs::remove_dir_all(&dir).ok();
}

/// 无会话时状态菜单切换仍须落盘：原先无会话直接 `return true` 而丢弃改动。
#[test]
fn option_change_without_sessions_persists() {
    let _guard = serial();
    let dir = temp_user_dir("option-no-session");
    let mut engine = Engine::new_with_dirs(host(), fixture_dirs(), None, Some(dir.clone()));
    assert!(engine.sessions.is_empty(), "本用例须无会话");
    assert!(engine.set_option_value("tiger_sentence_early_commit", false));
    let text = std::fs::read_to_string(dir.join(hux_cfg::OPTIONS_FILE)).expect("options.yaml");
    assert!(
        text.contains("tiger_sentence_early_commit: false"),
        "无会话切换应落盘：{text}"
    );
    assert_eq!(
        engine.option_value("tiger_sentence_early_commit"),
        Some(false)
    );
    std::fs::remove_dir_all(&dir).ok();
}
