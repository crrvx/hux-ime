// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! 配置角色映射与诊断的用例：选项声明持久化键、全角色落位、单侧改名、配置落地与翻页键解绑。

use super::super::assets::role;
use super::super::config::config_role_notes;
use super::*;
use crate::interaction::K_CHAR_TO_SOUND_SHAPE_KEY;

#[test]
fn option_declarations_are_stable_persisted_keys() {
    // 这些字符串是**持久化契约**（`options.yaml` / legacy `user.yaml` 的键；
    // 也是状态菜单与设置页的对接键）：改名会让老用户的设置静默失效。
    let declarations = fixture_scheme().option_declarations();
    let pairs: Vec<(&str, &str)> = declarations
        .iter()
        .map(|decl| (decl.role, decl.key))
        .collect();
    assert_eq!(
        pairs,
        vec![
            ("early_commit", "tiger_sentence_early_commit"),
            (
                "early_commit_to_preedit",
                "tiger_sentence_early_commit_to_preedit"
            ),
            (
                "allow_duplicate_single",
                "tiger_sentence_allow_duplicate_single"
            ),
            ("digit_select", "tiger_sentence_digit_select"),
            ("full_charset", "tiger_sentence_full_charset"),
            ("filter_non_han", "tiger_sentence_filter_non_han"),
        ]
    );
}

#[test]
fn config_bag_maps_every_role_and_defaults_are_unchanged() {
    // 全角色袋 → 逐字段落位（角色名与 `hux-cfg` 的常量一致，由平台测试守护）。
    let full = bag_with_a_value_for_every_role();
    let (config, errors) = Config::parse(&full);
    assert_eq!(errors, Vec::<ConfigError>::new(), "全角色袋无诊断");
    check_values_for_every_role_land(&config);

    // 空袋 → 与迁移前的 `SchemeConfig::default()` 逐字段同值（缺角色回退不变），
    // 但每个角色都产出「缺少角色」诊断（不再静默）。
    let (empty, empty_errors) = Config::parse(&SchemeConfig::default());
    // 配置角色 + 三个运行时选项角色（单字重码 / 全字集 / 过滤非汉字）。
    assert_eq!(empty_errors.len(), SCHEME_CONFIG_ROLES.len() + 3);
    assert!(
        empty_errors
            .iter()
            .all(|error| matches!(error, ConfigError::Missing { .. })),
        "空袋应逐角色报缺失：{empty_errors:?}"
    );
    assert_eq!(empty.high_freq_limit, 0);
    assert_eq!(empty.min_retained_input_length, 0);
    assert_eq!(empty.page_size, 0);
    assert!(!empty.page_cycle);
    assert_eq!(empty.page_up_keys, None, "缺角色 ≠ 显式空列表");
    assert_eq!(empty.page_down_keys, None);
    assert!(empty.reverse_lookup_pronunciation_keys.is_empty());
    assert!(empty.reverse_lookup_character_keys.is_empty());
    assert!(!empty.learning_on_tab);
    assert!(!empty.allow_duplicate_single);
    // 字集开关缺角色时回退**出厂缺省（开）**，不退化成「只装主表 / 不过滤」。
    assert!(empty.full_charset);
    assert!(empty.filter_non_han);

    // 未知角色被忽略（契约是通用容器：将来新增角色不破坏本方案）。
    let future = bag_with_an_unknown_role();
    assert_eq!(future.text("future_role"), Some("x"));
    let (parsed, errors) = Config::parse(&future);
    assert_eq!(parsed.page_size, 9);
    assert!(
        errors.iter().all(|error| error.role() != role::PAGE_SIZE),
        "已装配且类型正确的角色不得报诊断：{errors:?}"
    );
}

/// 角色一致性守护的方案侧一半：装配方按 `hux-cfg` 的角色名装袋，本方案按自己的角色名读袋，
/// 单侧改名必须**可见**（进状态串诊断），不得静默回退默认值。
#[test]
fn config_role_notes_expose_single_sided_role_renames() {
    // 正例：本方案声明的全角色袋（+ 平台追加的运行时选项角色）无诊断。
    let full = bag_of(&|_| true, &[]);
    assert_eq!(config_role_notes(&full), Vec::<String>::new());
    assert_eq!(diagnostics(&full), Vec::<String>::new());

    // 负例：装袋侧把 `tab_learning` 改名（模拟 `hux-cfg` 单侧漂移）——
    // 未识别与缺少两侧都点名，用户可在状态串看到「设置没生效」的原因。
    let renamed = bag_of(
        &|role| role != role::LEARNING_ON_TAB,
        &[("tab_learning_X", Value::Bool(true))],
    );
    assert_eq!(
        config_role_notes(&renamed),
        vec!["config: 未识别的角色 tab_learning_X".to_string()]
    );
    assert_eq!(
        diagnostics(&renamed),
        vec![
            "config: 未识别的角色 tab_learning_X".to_string(),
            "config: 缺少角色 tab_learning".to_string(),
        ]
    );
    // 取值仍按缺省回退（可观测行为不变：缺角色 = 不学习），但回退**不再静默**。
    let (parsed, errors) = Config::parse(&renamed);
    assert!(!parsed.learning_on_tab);
    assert_eq!(
        errors,
        vec![ConfigError::Missing {
            role: role::LEARNING_ON_TAB
        }]
    );

    // 负例：**类型不符**（把 `Count` 塞进开关角色）——此前 `bool()` 只返回 `None`，
    // 全链路静默；现在逐角色点名。
    check_type_mismatch_is_reported();

    // 负例：漏装（平台少写一项）同样点名，不静默当作「不限制保留量」。
    let dropped = bag_of(&|role| role != role::MIN_RETAINED_INPUT_LENGTH, &[]);
    assert_eq!(
        config_role_notes(&dropped),
        Vec::<String>::new(),
        "角色集合层面无未识别项"
    );
    assert_eq!(
        diagnostics(&dropped),
        vec!["config: 缺少角色 min_retained_raw_length".to_string()]
    );
    assert_eq!(Config::parse(&dropped).0.min_retained_input_length, 0);
}

#[test]
fn config_reaches_sessions_and_host_options() {
    // 对应平台原先断言的 `session.min_retained` / `host_options` / 触发键。
    let mut scheme = fixture_scheme();
    let mut context = Context::new();
    let session = scheme.new_session(&mut context);
    let config = full_bag(&[
        (role::MIN_RETAINED_INPUT_LENGTH, Value::Count(4)),
        (role::PAGE_SIZE, Value::Count(7)),
        (role::PAGE_CYCLE, Value::Bool(true)),
        (role::PAGE_UP_KEYS, Value::Texts(vec!["comma".to_string()])),
        (
            role::REVERSE_LOOKUP_CHARACTER_KEYS,
            Value::Texts(vec!["quotedbl".to_string()]),
        ),
    ]);
    scheme.apply_config(&config).expect("全角色袋");
    assert_eq!(scheme.sessions[&session.0].min_retained, 4);
    assert_eq!(scheme.host_options.page_size, 7);
    assert!(scheme.host_options.page_cycle);
    sync_trigger_keys(&mut context, &scheme.config);
    assert_eq!(
        context.get_property(K_CHAR_TO_SOUND_SHAPE_KEY),
        Some("quotedbl")
    );
    assert_eq!(
        scheme.host_options.page_up_keys,
        vec![KeyEvent::from_repr("comma").expect("comma")]
    );
}

#[test]
fn empty_page_key_lists_unbind_the_keys() {
    // 角色**显式给出空列表** ⇒ 不绑定翻页键（与 `hux-cfg` 的
    // `Settings::host_options()` 同语义，即配置页清空键列表后真的不再翻页）；
    // 角色**缺失** ⇒ 保留 core 缺省绑定（`HostOptions::default()` 的 `-`/`=`）。
    let mut scheme = fixture_scheme();
    scheme
        .apply_config(&full_bag(&[
            (role::PAGE_UP_KEYS, Value::Texts(Vec::new())),
            (role::PAGE_DOWN_KEYS, Value::Texts(Vec::new())),
        ]))
        .expect("全角色袋");
    assert!(
        scheme.host_options.page_up_keys.is_empty(),
        "显式空列表 ⇒ 上翻页键不绑定"
    );
    assert!(
        scheme.host_options.page_down_keys.is_empty(),
        "显式空列表 ⇒ 下翻页键不绑定"
    );
    // 缺角色（空袋）⇒ 保持缺省绑定，与 core 一致。
    let mut missing = fixture_scheme();
    // 空袋 ⇒ 逐角色诊断，但配置照常落地（与 `apply_config` 的既有语义一致）。
    let errors = missing
        .apply_config(&SchemeConfig::default())
        .expect_err("空袋必须回逐角色诊断");
    assert!(
        errors
            .iter()
            .all(|error| matches!(error, hux_core::scheme::ConfigError::Missing { .. })),
        "空袋应逐角色报缺失：{errors:?}"
    );
    assert_eq!(
        missing.host_options.page_up_keys,
        HostOptions::default().page_up_keys
    );
    assert_eq!(
        missing.host_options.page_down_keys,
        HostOptions::default().page_down_keys
    );
    assert!(
        !HostOptions::default().page_up_keys.is_empty(),
        "core 缺省上翻页绑定应为非空（否则本用例是恒真的）"
    );
}

/// 全角色袋：每个角色都给一个**类型正确**的值（真实装配路径就是这个形态）。
fn bag_with_a_value_for_every_role() -> SchemeConfig {
    bag(&[
        (role::HIGH_FREQ_LIMIT, Value::Count(1500)),
        (role::MIN_RETAINED_INPUT_LENGTH, Value::Count(4)),
        (role::PAGE_SIZE, Value::Count(7)),
        (role::PAGE_CYCLE, Value::Bool(true)),
        (role::PAGE_UP_KEYS, Value::Texts(vec!["comma".to_string()])),
        (
            role::PAGE_DOWN_KEYS,
            Value::Texts(vec!["period".to_string()]),
        ),
        (
            role::REVERSE_LOOKUP_PRONUNCIATION_KEYS,
            Value::Texts(vec!["grave".to_string()]),
        ),
        (
            role::REVERSE_LOOKUP_CHARACTER_KEYS,
            Value::Texts(vec!["quotedbl".to_string()]),
        ),
        (role::LEARNING_ON_TAB, Value::Bool(true)),
        (role::ALLOW_DUPLICATE_SINGLE, Value::Bool(true)),
        (role::FULL_CHARSET, Value::Bool(true)),
        (role::FILTER_NON_HAN, Value::Bool(true)),
    ])
}

/// 全角色袋逐字段落位（每个角色都取到装入值）。
fn check_values_for_every_role_land(config: &Config) {
    assert_eq!(config.high_freq_limit, 1500);
    assert_eq!(config.min_retained_input_length, 4);
    assert_eq!(config.page_size, 7);
    assert!(config.page_cycle);
    assert_eq!(config.page_up_keys, Some(vec!["comma".to_string()]));
    assert_eq!(config.page_down_keys, Some(vec!["period".to_string()]));
    assert_eq!(
        config.reverse_lookup_pronunciation_keys,
        vec!["grave".to_string()]
    );
    assert_eq!(
        config.reverse_lookup_character_keys,
        vec!["quotedbl".to_string()]
    );
    assert!(config.learning_on_tab);
    assert!(config.allow_duplicate_single);
    assert!(config.full_charset);
    assert!(config.filter_non_han);
}

/// 未识别的角色袋：解析器忽略（契约是通用容器，将来新增角色不破坏本方案）。
fn bag_with_an_unknown_role() -> SchemeConfig {
    bag(&[
        ("future_role", Value::Text("x".to_string())),
        (role::PAGE_SIZE, Value::Count(9)),
    ])
}

// 按角色清单装袋（`keep` 过滤出需要的角色；每个角色给**类型正确**的值，
// 这样诊断只会来自角色集合或刻意构造的类型不符）。
fn bag_of(keep: &dyn Fn(&str) -> bool, extra: &[(&'static str, Value)]) -> SchemeConfig {
    let mut config = SchemeConfig::new();
    for (name, value) in [
        (role::HIGH_FREQ_LIMIT, Value::Count(1)),
        (role::MIN_RETAINED_INPUT_LENGTH, Value::Count(1)),
        (role::PAGE_SIZE, Value::Count(1)),
        (role::PAGE_CYCLE, Value::Bool(true)),
        (role::PAGE_UP_KEYS, Value::Texts(vec!["minus".to_string()])),
        (
            role::PAGE_DOWN_KEYS,
            Value::Texts(vec!["equal".to_string()]),
        ),
        (
            role::REVERSE_LOOKUP_PRONUNCIATION_KEYS,
            Value::Texts(vec!["grave".to_string()]),
        ),
        (
            role::REVERSE_LOOKUP_CHARACTER_KEYS,
            Value::Texts(vec!["quotedbl".to_string()]),
        ),
        (role::LEARNING_ON_TAB, Value::Bool(true)),
        (role::ALLOW_DUPLICATE_SINGLE, Value::Bool(true)),
        (role::FULL_CHARSET, Value::Bool(true)),
        (role::FILTER_NON_HAN, Value::Bool(true)),
    ] {
        if keep(name) {
            config.set(name, value);
        }
    }
    for (role, value) in extra {
        config.set(role, value.clone());
    }
    config
}

// 诊断 = 角色集合（未识别）+ 逐角色（缺失 / 类型不符），文案统一带 `config:` 前缀。
fn diagnostics(bag: &SchemeConfig) -> Vec<String> {
    let (_, errors) = Config::parse(bag);
    config_diagnostics(bag, &errors)
}

/// 「类型不符」负例：把 `Count` 塞进开关角色，逐角色点名（不再全链路静默）。
fn check_type_mismatch_is_reported() {
    let wrong_type = bag_of(&|_| true, &[(role::LEARNING_ON_TAB, Value::Count(1))]);
    assert_eq!(config_role_notes(&wrong_type), Vec::<String>::new());
    assert_eq!(
        diagnostics(&wrong_type),
        vec!["config: 角色 tab_learning 类型不符（期望 开关，实际 计数）".to_string()]
    );
    assert_eq!(
        Config::parse(&wrong_type).1,
        vec![ConfigError::TypeMismatch {
            role: role::LEARNING_ON_TAB,
            expected: "开关",
            found: "计数",
        }]
    );
    assert!(
        !Config::parse(&wrong_type).0.learning_on_tab,
        "类型不符按缺省回退"
    );
}
