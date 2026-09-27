// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;

/// **每个角色都必须被方案声明**：装配处按角色解析选项键，缺一即报错。
#[test]
fn every_configured_role_is_declared_by_the_scheme() {
    let _guard = serial();
    let engine = TestEngine::new(host(), fixture_dirs(), None, None);
    let declarations = engine.engine.scheme.option_declarations();
    // 正例：真实方案的声明齐备，装配处不产生诊断（`options:` 前缀只用于装配/保存错误）。
    let (roles, error) = crate::engine::resolve_option_roles(declarations);
    assert_eq!(error, None, "tiger 的声明应覆盖全部角色");
    for role in hux_cfg::roles::SCHEME_OPTION_ROLES {
        assert!(
            roles.key(role).is_some(),
            "角色 {role} 必须被方案声明（否则状态菜单与持久化静默失效）"
        );
    }
    assert!(
        !engine
            .engine
            .status
            .to_str()
            .unwrap_or("")
            .contains("options:"),
        "装配正常时状态串不含角色诊断"
    );

    // 负例：缺角色 → 报错且不接线（不落到角色名字面量上）。
    let incomplete = [
        OptionDecl {
            role: hux_cfg::roles::ROLE_EARLY_COMMIT,
            key: "scheme_early_commit",
        },
        OptionDecl {
            role: "not_a_configured_role",
            key: "scheme_unknown",
        },
    ];
    let (roles, error) = crate::engine::resolve_option_roles(&incomplete);
    let error = error.expect("缺角色应报错");
    assert!(error.contains("方案未声明角色"), "诊断文案：{error}");
    assert!(error.contains(hux_cfg::roles::ROLE_DIGIT_SELECT));
    assert_eq!(roles.key(hux_cfg::roles::ROLE_EARLY_COMMIT), None);
    assert_eq!(roles.key(hux_cfg::roles::ROLE_FULL_SHAPE), None);
}

/// 角色常量值 ↔ 方案读取的角色：两侧**同值同序**（任一侧改名即失败）。
///
/// 这是角色一致性的核心守护：`Config::parse` 对未知角色 `unwrap_or(0/false)` **静默回退**，
/// 故单侧改名过去可让 `min_retained_raw_length`（→0，不再限制保留量）/ `high_freq_limit`
/// （→0，高频过滤全放开）静默失效而全绿。方案不依赖 `hux-cfg`、配置层不依赖方案，
/// 两侧只能在此（装配根）对齐：清单逐项比对 + 真实装配路径无诊断 + 改名必报诊断。
#[test]
fn scheme_config_roles_match_the_scheme() {
    let _guard = serial();
    assert_eq!(
        hux_cfg::roles::SCHEME_CONFIG_ROLES,
        hux_scheme_tiger::scheme::SCHEME_CONFIG_ROLES,
        "cfg 的配置角色清单必须与方案读取的角色同值同序"
    );
    // 正例：真实装配路径（配置层装袋 → 方案读袋）无角色漂移诊断。
    let engine = TestEngine::new(host(), fixture_dirs(), None, Some(temp_user_dir("roles")));
    let status = engine.engine.status.to_str().unwrap_or("").to_string();
    assert!(
        !status.contains("config:"),
        "角色一致时状态串不应有配置诊断：{status}"
    );

    // 负例：把 `high_freq_limit` 单侧改名（等价于改 `ROLE_HIGH_FREQ_LIMIT` 的值）
    // → 方案点名「未识别的角色 / 缺少角色」，用户侧不再是「设置没生效」的哑失败。
    let mut renamed = hux_core::scheme::SchemeConfig::new();
    for role in hux_cfg::roles::SCHEME_CONFIG_ROLES {
        if *role != hux_cfg::roles::ROLE_HIGH_FREQ_LIMIT {
            renamed = renamed.with(role, hux_core::scheme::Value::Count(1));
        }
    }
    renamed = renamed
        .with(
            "high_freq_limit_X",
            hux_core::scheme::Value::Count(hux_cfg::DEFAULT_HIGH_FREQ_LIMIT),
        )
        // 平台追加的运行时选项角色照常装入（漂移项只有 `high_freq_limit`）。
        .with(
            hux_cfg::roles::ROLE_ALLOW_DUPLICATE_SINGLE,
            hux_core::scheme::Value::Bool(true),
        );
    let (_, notes) = hux_scheme_tiger::scheme::TigerScheme::load(&fixture_dirs(), None, &renamed);
    assert!(
        notes
            .iter()
            .any(|note| note == "config: 未识别的角色 high_freq_limit_X"),
        "改名后应报未识别角色：{notes:?}"
    );
    assert!(
        notes
            .iter()
            .any(|note| note == "config: 缺少角色 high_freq_limit"),
        "改名后应报缺少角色：{notes:?}"
    );
}

/// 角色表的分组 / 顺序 / 包含关系：手工清单必须钉在角色表上。
///
/// 此前三条不变式（方案开关 ⊆ 运行时角色、存储缺省 ≡ 运行时角色、会话缺省 ⊇ 运行时角色）
/// 只是「今天恰好成立」——新增一个方案开关角色时不会有任何断言失败。
#[test]
fn runtime_role_tables_cover_the_declared_roles() {
    let _guard = serial();
    let engine = TestEngine::new(host(), fixture_dirs(), None, Some(temp_user_dir("roles2")));
    let roles = &engine.engine.option_roles;
    // 方案声明的角色序 = 配置层的方案开关名单（顺序错位会让状态菜单与开关对不上）。
    assert_eq!(
        engine
            .engine
            .scheme
            .option_declarations()
            .iter()
            .map(|decl| decl.role)
            .collect::<Vec<_>>(),
        hux_cfg::roles::SCHEME_OPTION_ROLES.to_vec(),
        "方案声明顺序必须等于 SCHEME_OPTION_ROLES"
    );
    // 运行时角色表与 ABI 角色数同源（空表也不得让本用例空转）。
    let runtime = engine.engine.runtime_options().to_vec();
    assert_eq!(runtime.len(), hux_cfg::roles::RUNTIME_OPTION_ROLES.len());
    let settings = Settings::default();
    let store = settings.store_defaults(roles);
    let defaults = settings.session_option_defaults(roles);
    assert_eq!(store.len(), hux_cfg::roles::RUNTIME_OPTION_ROLES.len());
    for role in hux_cfg::roles::RUNTIME_OPTION_ROLES {
        let key = roles
            .key(role)
            .unwrap_or_else(|| panic!("角色 {role} 应有键"));
        assert!(
            store.contains_key(key),
            "运行时角色 {role} 必须可持久化（否则菜单能切但不落盘）"
        );
        assert!(
            defaults.iter().any(|(name, _)| *name == key),
            "运行时角色 {role} 必须有会话缺省（否则新会话回退到别处）"
        );
    }
    // 宿主标准项 `ascii_punct` 只作会话初始选项，不入运行时菜单、不落盘。
    assert!(
        defaults
            .iter()
            .any(|(name, _)| *name == hux_cfg::roles::ROLE_ASCII_PUNCT)
    );
    assert!(!store.contains_key(hux_cfg::roles::ROLE_ASCII_PUNCT));
}

#[test]
fn option_role_order_matches_the_abi_header() {
    let members = abi_enum_members("HUX_OPTION");

    let expected: Vec<String> = hux_cfg::roles::RUNTIME_OPTION_ROLES
        .iter()
        .map(|role| format!("HUX_OPTION_{}", role.to_ascii_uppercase()))
        .collect();
    assert_eq!(
        members.len(),
        expected.len() + 1,
        "枚举 = 角色序 + 计数哨兵（实际：{members:?}）"
    );
    assert_eq!(
        members[..expected.len()]
            .iter()
            .map(|(name, _)| name.as_str())
            .collect::<Vec<_>>(),
        expected.iter().map(String::as_str).collect::<Vec<_>>(),
        "hux_abi.h 的角色序（顺序 / 个数 / 名字）必须等于 RUNTIME_OPTION_ROLES"
    );
    // 下标连续 0..=n：重复 / 跳号会让宿主按角色取到错位的键。
    assert_eq!(
        members.iter().map(|(_, value)| *value).collect::<Vec<_>>(),
        (0..=expected.len() as i32).collect::<Vec<_>>()
    );
    let (sentinel, count) = &members[expected.len()];
    assert_eq!(sentinel, "HUX_OPTION_COUNT");
    assert_eq!(*count as usize, hux_cfg::roles::RUNTIME_OPTION_ROLES.len());
}

/// `hux_abi.h` 的 `HUX_CANDIDATE_LAYOUT_*` / `HUX_PREEDIT_MODE_*` ↔ `crate::abi` 常量（名字与取值）。
///
/// 两组取值是 **ABI**：壳侧 `shell/hux.cpp` 用同名宏填充（`static_assert` 守 C++ 枚举 ↔ 宏），
/// Rust 侧 `abi.rs` 用具名常量读取（`hux_engine_apply_settings`）。头文件 ↔ Rust 常量的名字与
/// 取值由本用例从**头文件源码**逐项比对——改名 / 改值都在此失败。
#[test]
fn option_value_enums_match_the_abi_header() {
    let layout = abi_enum_members("HUX_CANDIDATE_LAYOUT");
    assert_eq!(
        layout,
        vec![
            // 默认档在 Rust 侧不具名：它是 `hux_engine_apply_settings` 里 `_` 分支的兜底。
            ("HUX_CANDIDATE_LAYOUT_FOLLOW_GLOBAL".to_string(), 0),
            (
                "HUX_CANDIDATE_LAYOUT_HORIZONTAL".to_string(),
                crate::abi::CANDIDATE_LAYOUT_HORIZONTAL
            ),
            (
                "HUX_CANDIDATE_LAYOUT_VERTICAL".to_string(),
                crate::abi::CANDIDATE_LAYOUT_VERTICAL
            ),
            ("HUX_CANDIDATE_LAYOUT_COUNT".to_string(), 3),
        ]
    );
    let preedit = abi_enum_members("HUX_PREEDIT_MODE");
    assert_eq!(
        preedit,
        vec![
            ("HUX_PREEDIT_MODE_CANDIDATE_CODE".to_string(), 0),
            (
                "HUX_PREEDIT_MODE_RAW_INPUT".to_string(),
                crate::abi::PREEDIT_MODE_RAW_INPUT
            ),
            (
                "HUX_PREEDIT_MODE_HIDDEN".to_string(),
                crate::abi::PREEDIT_MODE_HIDDEN
            ),
            ("HUX_PREEDIT_MODE_COUNT".to_string(), 3),
        ]
    );
    // 哨兵 = 取值个数（壳侧 static_assert 守同一条不变式）。
    assert_eq!(layout.last().unwrap().1 as usize, layout.len() - 1);
    assert_eq!(preedit.last().unwrap().1 as usize, preedit.len() - 1);
}

/// 设置 → 角色袋必须覆盖 `hux-cfg` 声明的**全部**配置角色（漏一个即失败）。
#[test]
fn scheme_config_covers_every_declared_role() {
    let bag = crate::engine::scheme_config(&Settings::default());
    assert_eq!(
        bag.roles().collect::<Vec<_>>(),
        hux_cfg::roles::SCHEME_CONFIG_ROLES.to_vec(),
        "配置袋的角色与顺序即 `hux-cfg` 的角色全集"
    );
    let settings = Settings::default();
    assert_eq!(
        bag.count(hux_cfg::roles::ROLE_HIGH_FREQ_LIMIT),
        Some(settings.high_freq_limit)
    );
    assert_eq!(
        bag.count(hux_cfg::roles::ROLE_MIN_RETAINED_INPUT_LENGTH),
        Some(settings.min_retained())
    );
    assert_eq!(
        bag.texts(hux_cfg::roles::ROLE_REVERSE_LOOKUP_PRONUNCIATION_KEYS),
        Some(settings.reverse_lookup_pronunciation_keys.as_slice())
    );
    assert_eq!(
        bag.bool(hux_cfg::roles::ROLE_LEARNING_ON_TAB),
        Some(settings.learning_on_tab)
    );
    // 钳制在装配处生效（页大小 / 最短保留码数上限）。
    let clamped = crate::engine::scheme_config(&Settings {
        page_size: 999,
        min_retained_input_length: 999,
        ..Default::default()
    });
    assert_eq!(
        clamped.count(hux_cfg::roles::ROLE_PAGE_SIZE),
        Some(hux_core::host::MAX_PAGE_SIZE)
    );
    assert_eq!(
        clamped.count(hux_cfg::roles::ROLE_MIN_RETAINED_INPUT_LENGTH),
        Some(hux_cfg::MAX_MIN_RETAINED_INPUT_LENGTH)
    );
}

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
