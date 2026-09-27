// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! 方案装配、配置映射与会话生命周期的单元测试（原名内联于门面文件）。

use super::assets::role;
use super::config::config_role_notes;
use super::*;
use hux_core::learning::LearningIndex;
use hux_core::scheme::Value;

fn fixture_dirs() -> Vec<PathBuf> {
    vec![hux_test_support::repo_path("goldens/lexicon")]
}

/// 测试配置袋（角色名与 `hux-cfg` 的常量同值；迁移前逐字段的等价物）。
fn bag(entries: &[(&'static str, Value)]) -> SchemeConfig {
    let mut config = SchemeConfig::new();
    for (role, value) in entries {
        config.set(role, value.clone());
    }
    config
}

/// 全角色袋（每个角色都给一个**类型正确**的值），`overrides` 覆盖同名角色。
/// 真实装配路径（平台）就是这个形态：此后任何缺口都会回诊断。
fn full_bag(overrides: &[(&'static str, Value)]) -> SchemeConfig {
    let mut config = bag(&[
        (role::HIGH_FREQ_LIMIT, Value::Count(1500)),
        (role::MIN_RETAINED_INPUT_LENGTH, Value::Count(0)),
        (role::PAGE_SIZE, Value::Count(5)),
        (role::PAGE_CYCLE, Value::Bool(false)),
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
    ]);
    for (name, value) in overrides {
        config.set(name, value.clone());
    }
    config
}

fn fixture_scheme() -> TigerScheme {
    let config = bag(&[
        (role::HIGH_FREQ_LIMIT, Value::Count(0)),
        (role::PAGE_SIZE, Value::Count(5)),
        (role::LEARNING_ON_TAB, Value::Bool(true)),
    ]);
    TigerScheme::load(&fixture_dirs(), None, &config).0
}

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
    let full = bag(&[
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
    ]);
    let (config, errors) = Config::parse(&full);
    assert_eq!(errors, Vec::<ConfigError>::new(), "全角色袋无诊断");
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
    let future = bag(&[
        ("future_role", Value::Text("x".to_string())),
        (role::PAGE_SIZE, Value::Count(9)),
    ]);
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
    // 按角色清单装袋（`keep` 过滤出需要的角色；每个角色给**类型正确**的值，
    // 这样诊断只会来自角色集合或刻意构造的类型不符）。
    let bag_of = |keep: &dyn Fn(&str) -> bool, extra: &[(&'static str, Value)]| {
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
    };
    // 诊断 = 角色集合（未识别）+ 逐角色（缺失 / 类型不符），文案统一带 `config:` 前缀。
    let diagnostics = |bag: &SchemeConfig| {
        let (_, errors) = Config::parse(bag);
        config_diagnostics(bag, &errors)
    };
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
fn supplement_dir_searches_all_data_dirs() {
    // 一键安装把数据装在系统级目录（用户目录在前但为空）时，补充短语仍须被找到。
    let user_dir = hux_test_support::temp_dir("supplement-user");
    let system_dir = hux_test_support::temp_dir("supplement-system");
    assert_eq!(
        supplement_dir(&[user_dir.clone(), system_dir.clone()]),
        None
    );
    std::fs::write(system_dir.join(SUPPLEMENT_FILE), "甲 乙 2\n").expect("write");
    assert_eq!(
        supplement_dir(&[user_dir.clone(), system_dir.clone()]),
        Some(system_dir.clone())
    );
    std::fs::write(user_dir.join(SUPPLEMENT_FILE), "甲 乙 2\n").expect("write");
    assert_eq!(
        supplement_dir(&[user_dir.clone(), system_dir]),
        Some(user_dir)
    );
}

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
fn apply_config_rebuilds_the_lexicon_for_a_new_high_freq_limit() {
    // 平台在装配方案**之后**才把配置页设置下发（`hux_engine_new` → 宿主 `applyConfig`），
    // 故上限只在 `load` 时生效等于「设置永不生效」；本用例钉住重新下发即重建。
    //
    // 跨层分工：平台侧 `platform/fcitx5/src/tests.rs` 的
    // `apply_settings_rebuilds_the_lexicon_for_a_new_high_freq_limit` 负责引擎可见结果
    // （候选列表里非主码条目消失/回来）；本用例只断言内核独有的**容量上界**：
    // 上限决定码表里保留的 (码, 字) 槽位总数，收紧必须真的丢槽位、放开必须完整复原。
    let capacity = |scheme: &TigerScheme| -> usize {
        scheme
            .decoder
            .lexicon()
            .codes
            .iter()
            .map(|(_, entries)| entries.len())
            .sum()
    };
    let mut scheme = fixture_scheme(); // 夹具按上限 0（不过滤）装载
    let full = capacity(&scheme);
    assert!(full > 0, "夹具码表非空");
    assert_eq!(scheme.decoder.lexicon().data_status().high_freq_limit, 0);
    scheme
        .apply_config(&full_bag(&[(role::HIGH_FREQ_LIMIT, Value::Count(1500))]))
        .expect("全角色袋");
    let tightened = capacity(&scheme);
    assert!(
        tightened < full,
        "收紧上限必须丢弃非主码槽位：{tightened} 应小于 {full}"
    );
    assert_eq!(scheme.decoder.lexicon().data_status().high_freq_limit, 1500);
    // 放开上限同样重建（不是「只收紧一次」）⇒ 容量回到原值。
    scheme
        .apply_config(&full_bag(&[(role::HIGH_FREQ_LIMIT, Value::Count(0))]))
        .expect("全角色袋");
    assert_eq!(capacity(&scheme), full, "放开上限必须完整复原槽位");
    assert_eq!(scheme.decoder.lexicon().data_status().high_freq_limit, 0);
    assert_eq!(
        scheme.learning_mode(),
        format!(
            "sentence-v2|rules={}|optimal=0|dup=1",
            scheme.learning_rules
        )
    );
}

/// 夹具码表 + **一张追加码表**（一个扩展 B 汉字与一个部首，各给一个新码）：
/// 字集开关的用例要有追加表才能看出效果（[`fixture_dirs`] 里没有）。
fn charset_dirs() -> PathBuf {
    let dir = hux_test_support::temp_dir("scheme-charset");
    let fixture = fixture_dirs().remove(0);
    for name in [
        "tiger_sentence.codes.txt",
        "tiger_sentence.char_ranks.txt",
        "tiger_sentence.full_code_whitelist.txt",
    ] {
        std::fs::copy(fixture.join(name), dir.join(name)).expect("复制夹具码表");
    }
    std::fs::write(
        dir.join("tiger_sentence.codes.huma.txt"),
        "𤕫\tzzzv\n⽧\tzzzw\n",
    )
    .expect("写追加码表");
    dir
}

/// 两个字集开关都改词库内容 ⇒ 重新下发配置即重建（同高频上限的重建路径）。
///
/// 语义：关掉全字集只装主表；过滤只作用于**追加表**（主表里的非汉字照旧）。
#[test]
fn apply_config_rebuilds_the_lexicon_for_the_charset_options() {
    let dir = charset_dirs();
    let scheme_of = |overrides: &[(&'static str, Value)]| -> TigerScheme {
        TigerScheme::load(std::slice::from_ref(&dir), None, &full_bag(overrides)).0
    };
    let texts = |scheme: &TigerScheme, code: &str| -> Vec<String> {
        scheme
            .decoder
            .lexicon()
            .probe(code)
            .unwrap_or_else(|| panic!("码 {code} 不存在"))
            .iter()
            .map(|entry| entry.text.clone())
            .collect()
    };

    // 仅主表的条目数（关掉全字集装载；下面用它作基准口径）。
    let primary_entries = scheme_of(&[(role::FULL_CHARSET, Value::Bool(false))])
        .decoder
        .lexicon()
        .codes_entries;

    // 出厂口径（全字集开 + 过滤开）：追加表的汉字在，部首被过滤。
    let mut scheme = scheme_of(&[]);
    assert_eq!(texts(&scheme, "zzzv"), vec!["𤕫".to_string()]);
    assert!(
        scheme.decoder.lexicon().probe("zzzw").is_none(),
        "追加表里的部首应被过滤"
    );
    assert_eq!(scheme.decoder.lexicon().extra_code_tables().len(), 1);
    let filtered_entries = scheme.decoder.lexicon().codes_entries;
    assert_eq!(
        filtered_entries,
        primary_entries + 1,
        "过滤开：追加表只剩那个汉字"
    );
    assert!(
        scheme.data_info().starts_with(&format!(
            "code_tables=[tiger_sentence.codes.txt,tiger_sentence.codes.huma.txt] \
                 entries={filtered_entries}"
        )),
        "装载摘要应含实际装载的码表：{}",
        scheme.data_info()
    );
    assert!(
        scheme
            .data_info()
            .ends_with("full_charset=1 filter_non_han=1")
    );

    // 关掉全字集：追加表独有码消失、诊断口径为空（重新打开同样重建，不是「只关一次」）。
    scheme
        .apply_config(&full_bag(&[(role::FULL_CHARSET, Value::Bool(false))]))
        .expect("全角色袋");
    assert!(scheme.decoder.lexicon().probe("zzzv").is_none());
    assert!(scheme.decoder.lexicon().extra_code_tables().is_empty());
    assert_eq!(
        scheme.data_info(),
        format!(
            "code_tables=[tiger_sentence.codes.txt] entries={primary_entries} \
                 chars={} full_charset=0 filter_non_han=1",
            scheme.decoder.lexicon().character_codes.len()
        ),
        "关掉全字集后摘要只剩主表"
    );
    scheme
        .apply_config(&full_bag(&[(role::FULL_CHARSET, Value::Bool(true))]))
        .expect("全角色袋");
    assert_eq!(texts(&scheme, "zzzv"), vec!["𤕫".to_string()]);

    // 关掉过滤：追加表的部首入词库（条目正好多一条），主表内容不动。
    scheme
        .apply_config(&full_bag(&[(role::FILTER_NON_HAN, Value::Bool(false))]))
        .expect("全角色袋");
    assert_eq!(texts(&scheme, "zzzw"), vec!["⽧".to_string()]);
    assert_eq!(
        scheme.decoder.lexicon().codes_entries,
        primary_entries + 2,
        "过滤关：追加表两行都入词库"
    );
    assert!(
        scheme
            .data_info()
            .ends_with("full_charset=1 filter_non_han=0")
    );

    std::fs::remove_dir_all(&dir).ok();
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
