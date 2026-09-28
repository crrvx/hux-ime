// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! 选项键来源：角色 → 键由方案声明给出；运行时选项值经配置袋下发到方案。

use super::*;

/// 运行时选项值经配置袋下发到方案：单字重码开关决定学习 mode 的 `dup` 位。
#[test]
fn runtime_option_value_reaches_scheme_learning_mode() {
    let _guard = serial();
    let dir = temp_user_dir("duplicate-mode");
    let mut engine = TestEngine::new(host(), fixture_dirs(), None, Some(dir.clone()));
    // mode 串对平台不透明 ⇒ 只断言「随配置变化而变化」与「重启后一致」，
    // 具体格式（`dup=1` / `dup=0` 的映射）由方案自己的用例钉住
    // （`tiger` 的 `learning_mode_follows_config_and_rules`）。
    let default_mode = engine.engine.scheme.learning_mode().to_string();
    assert!(!default_mode.is_empty(), "学习开启时 mode 串非空");
    assert!(engine.set_option_value("tiger_sentence_allow_duplicate_single", false));
    let disabled_mode = engine.engine.scheme.learning_mode().to_string();
    assert_ne!(
        disabled_mode, default_mode,
        "运行时关掉单字重码后方案自算的 mode 串必须随之变化"
    );
    // 持久化值经「存储 → 会话 → 配置袋」在按键路径生效（重启后同样）。
    let mut restarted = TestEngine::new(host(), fixture_dirs(), None, Some(dir.clone()));
    restarted.key(u32::from(b'a'), 0, false);
    assert_eq!(
        restarted.engine.scheme.learning_mode(),
        disabled_mode,
        "options.yaml 的值优先于设置缺省（重启后 mode 与关掉时一致）"
    );
    std::fs::remove_dir_all(&dir).ok();
}

/// 选项角色 → 键：宿主（C++）据此构造状态菜单与面板序号，不再硬编码方案选项名。
#[test]
fn option_role_keys_follow_scheme_declarations() {
    let _guard = serial();
    let engine = TestEngine::new(host(), fixture_dirs(), None, None);
    assert_eq!(
        hux_engine_option_role_count(),
        hux_cfg::roles::RUNTIME_OPTION_ROLES.len() as i32
    );
    // 角色序（ABI 角色序）与**已解析**的键表逐项一致：菜单项与 ABI 索引不会错位
    // （宿主标准项 `full_shape` 由配置层自持，不在方案声明里）。
    assert_eq!(
        hux_cfg::roles::RUNTIME_OPTION_ROLES
            .iter()
            .map(|role| engine.engine.option_roles.key(role))
            .collect::<Vec<_>>(),
        vec![
            Some("tiger_sentence_early_commit"),
            Some("tiger_sentence_early_commit_to_preedit"),
            Some("tiger_sentence_allow_duplicate_single"),
            Some("full_shape"),
            Some("tiger_sentence_digit_select"),
            Some("tiger_sentence_full_charset"),
            Some("tiger_sentence_filter_non_han"),
        ],
        "角色序（含宿主标准项 full_shape）↔ 方案声明的键"
    );
    let keys: Vec<String> = (0..hux_cfg::roles::RUNTIME_OPTION_ROLES.len() as i32)
        .map(|role| unsafe {
            let key = hux_engine_option_key(&engine.engine, role);
            assert!(!key.is_null(), "角色 {role} 应有选项键");
            std::ffi::CStr::from_ptr(key).to_string_lossy().into_owned()
        })
        .collect();
    assert_eq!(
        keys,
        vec![
            "tiger_sentence_early_commit",
            "tiger_sentence_early_commit_to_preedit",
            "tiger_sentence_allow_duplicate_single",
            "full_shape",
            "tiger_sentence_digit_select",
            "tiger_sentence_full_charset",
            "tiger_sentence_filter_non_han",
        ]
    );
    // 角色序与状态菜单白名单同源（改方案时两者一起变）。
    assert_eq!(engine.engine.runtime_options().to_vec(), keys);
    // 越界与空指针安全。
    assert!(
        unsafe {
            hux_engine_option_key(
                &engine.engine,
                hux_cfg::roles::RUNTIME_OPTION_ROLES.len() as i32,
            )
        }
        .is_null()
    );
    assert!(unsafe { hux_engine_option_key(&engine.engine, -1) }.is_null());
    assert!(unsafe { hux_engine_option_key(std::ptr::null(), 0) }.is_null());
}
