// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! 宿主壳与 ABI 头文件的**源码文本契约**。
//!
//! C++ 侧（`platform/fcitx5/shell/hux.cpp`）与 `platform/fcitx5/src/abi.rs` 分属两次编译
//! （cmake / cargo）：两侧对不上时两边都编译得过，只有读源码逐项比对才拦得住——配置项
//! 默认值漂移、枚举改名 / 改值、ABI 入口漏声明都属于这一类。
//!
//! 分层：这些用例只依赖仓库内的文本（经 `repo_path` 定位）与 `hux-cfg` 的公开类型，
//! 不触碰 crate 内部项，故放集成测试；引擎行为（含 ABI 入口的判空 / 越界）留在
//! `src/tests.rs` 单测，那里够得着 `Engine`。

use hux_cfg::Settings;
use hux_test_support::repo_path;

/// 取 C++ 配置 schema（`shell/hux.cpp`）里 `.path{"<name>"}` 之后的 `.defaultValue` 字面量。
///
/// C++ 侧的默认值不参与 cargo 测试（`hux.cpp` 由 cmake 单独编译），改错了两侧都编译得过；
/// 这里以「解析源码」把它变成可断言的字面量（剥掉行注释；`KeyList` 的默认值跨多行，
/// 按花括号配平补齐）。
fn schema_default(name: &str) -> String {
    let source =
        std::fs::read_to_string(repo_path("platform/fcitx5/shell/hux.cpp")).expect("read hux.cpp");
    let lines: Vec<String> = source
        .lines()
        .map(|line| {
            line.trim()
                .split("//")
                .next()
                .unwrap_or("")
                .trim_end()
                .to_string()
        })
        .collect();
    for (index, line) in lines.iter().enumerate() {
        let Some(rest) = line.strip_prefix(".path{") else {
            continue;
        };
        if rest.trim_end_matches("},").trim_matches('"') != name {
            continue;
        }
        for (offset, next) in lines[index + 1..].iter().enumerate() {
            if next.starts_with(".path{") {
                break;
            }
            let Some(rest) = next.strip_prefix(".defaultValue = ") else {
                continue;
            };
            let mut value = rest.trim_end_matches(',').to_string();
            let mut open = value.matches('{').count() as i32 - value.matches('}').count() as i32;
            let mut cursor = index + offset + 2;
            while open > 0 && cursor < lines.len() {
                let more = lines[cursor].trim_end_matches(',');
                value.push_str(more);
                open += more.matches('{').count() as i32 - more.matches('}').count() as i32;
                cursor += 1;
            }
            return value;
        }
        panic!("schema 项 {name} 没有 defaultValue");
    }
    panic!("schema 缺少项 {name}");
}

/// 「候选窗口显示预编辑」是宿主显示项（不进引擎 `Settings`，故
/// `schema_defaults_match_settings_defaults` 明确跳过它）：默认值在这里单独钉住——
/// 改回「关」不会让任何编译或其它测试失败，而用户侧就是「预编辑又没了」。
#[test]
fn host_schema_panel_preedit_defaults_to_on() {
    assert_eq!(schema_default("PanelPreedit"), "true");
}

/// 默认反查触发键：音反查 `` ` ``（`grave`）、字反查 `~`（`asciitilde`），且都**无修饰**。
///
/// `~` 在物理键盘上是 Shift+`` ` ``，但前端上报的是该 level 的 keysym（`asciitilde`+Shift），
/// 而 fcitx5 `Key::normalize()` 会去掉这类「本身就产字符」键的 Shift（旧默认 `Alt+:` 同理：
/// `:` = Shift+`;` 归一化成 `colon`+Alt）⇒ 引擎收到的是 `asciitilde` + 无修饰。
/// 断言按语义给（含 keysym 且无修饰），不钉源码的书写形式。
#[test]
fn host_schema_reverse_lookup_defaults_are_grave_and_asciitilde() {
    let pronunciation = schema_default("SoundToCharShapeKey");
    assert!(
        pronunciation.contains("FcitxKey_grave") && pronunciation.contains("KeyState::NoState"),
        "音反查默认键应为无修饰的 `（grave）：{pronunciation}"
    );
    assert!(
        !pronunciation.contains("KeyState::Alt"),
        "音反查默认键不应带修饰：{pronunciation}"
    );
    let character = schema_default("CharToSoundShapeKey");
    assert!(
        character.contains("FcitxKey_asciitilde") && character.contains("KeyState::NoState"),
        "字反查默认键应为无修饰的 ~（asciitilde）：{character}"
    );
    assert!(
        !character.contains("KeyState::Alt"),
        "字反查默认键不应带修饰：{character}"
    );
}

/// C++ 配置 schema（`shell/hux.cpp`）的默认值必须与 `hux-cfg::Settings::default()` 一致。
///
/// 两边各写一份默认值且此前无任何校验：C++ 构造时即 `applyConfig` 覆盖引擎侧默认，
/// 故 Rust 侧漂移不会被发现。此处以「解析 C++ 源 ↔ 逐项比对」把它变成 CI 不变量。
/// 两个三态项（「提前上屏」「标点」）各承载两个 `Settings` 布尔字段：期望值取**两个布尔的
/// 默认值按引擎折算规则**拼出的枚举名（见 `tri_state_options_fold_to_engine_booleans`），
/// 故两侧任一处漂移都会在此失败。
#[test]
fn schema_defaults_match_settings_defaults() {
    let source =
        std::fs::read_to_string(repo_path("platform/fcitx5/shell/hux.cpp")).expect("read hux.cpp");

    // 解析 `.path{"Name"}` 与其后的 `.defaultValue = <value>,`（KeyList 可能跨多行）。
    let mut defaults: Vec<(String, String)> = Vec::new();
    let mut pending: Option<String> = None;
    let mut lines = source.lines().map(str::trim).peekable();
    while let Some(line) = lines.next() {
        if let Some(rest) = line.strip_prefix(".path{") {
            pending = Some(rest.trim_end_matches("},").trim_matches('"').to_string());
            continue;
        }
        let Some(rest) = line.strip_prefix(".defaultValue = ") else {
            continue;
        };
        let Some(name) = pending.take() else { continue };
        let mut value = rest.trim_end_matches(',').to_string();
        // KeyList 跨行：补齐花括号直到配平。
        let mut open = value.matches('{').count() as i32 - value.matches('}').count() as i32;
        while open > 0 {
            let next = lines.next().expect("unterminated defaultValue");
            value.push_str(next.trim_end_matches(','));
            open += next.matches('{').count() as i32 - next.matches('}').count() as i32;
        }
        defaults.push((name, value));
    }

    // C++ 用 keysym 常量声明默认键：`fcitx::Key(FcitxKey_grave, fcitx::KeyState::NoState)`
    // → rime 键名 `grave`（`FcitxKey_<name>` 即 X11 键名，与 librime 键名表同名）；
    // 带 `KeyState::Alt` 的项加 `Alt+` 前缀（旧默认形态）。
    let keys = |raw: &str| -> Vec<String> {
        raw.split("fcitx::Key(FcitxKey_")
            .skip(1)
            .filter_map(|part| {
                let name: String = part
                    .chars()
                    .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                    .collect();
                if name.is_empty() {
                    return None;
                }
                Some(if part.contains("KeyState::Alt") {
                    format!("Alt+{name}")
                } else {
                    name
                })
            })
            .collect()
    };
    let enum_tail = |raw: &str| raw.rsplit("::").next().unwrap_or(raw).to_string();

    let settings = Settings::default();
    // 三态项的期望枚举名：由两个布尔按引擎折算规则拼出（配置页 ⇒ `applyConfig()` 的同一映射）。
    let early_commit_mode = match (settings.early_commit, settings.early_commit_to_preedit) {
        (false, _) => "Off",
        (true, true) => "ToPreedit",
        (true, false) => "ToOutput",
    };
    let punct_mode = match (settings.ascii_punct, settings.full_shape) {
        (true, _) => "Ascii",
        (false, true) => "FullShapeAll",
        (false, false) => "FullShapeCommon",
    };
    let mut checked = 0usize;
    for (name, raw) in &defaults {
        let expected: String = match name.as_str() {
            "EarlyCommitMode" => early_commit_mode.to_string(),
            "PunctMode" => punct_mode.to_string(),
            "AllowDuplicateSingle" => settings.allow_duplicate_single.to_string(),
            "TabLearning" => settings.learning_on_tab.to_string(),
            "DigitSelect" => settings.digit_select.to_string(),
            "PageCycle" => settings.page_cycle.to_string(),
            "HighFreqLimit" => settings.high_freq_limit.to_string(),
            "PageSize" => settings.page_size.to_string(),
            "MinRetainedRawLength" => settings.min_retained_input_length.to_string(),
            "CandidateLayout" => format!("{:?}", settings.candidate_layout),
            "PreeditMode" => format!("{:?}", settings.preedit_mode),
            "PageUpKey" => settings.page_up_keys.join(","),
            "PageDownKey" => settings.page_down_keys.join(","),
            "SoundToCharShapeKey" => settings.reverse_lookup_pronunciation_keys.join(","),
            "CharToSoundShapeKey" => settings.reverse_lookup_character_keys.join(","),
            _ => continue, // PanelPreedit 等宿主显示项不经引擎
        };
        let actual = if raw.contains("fcitx::KeyList") {
            keys(raw).join(",")
        } else if expected.chars().all(|c| c.is_ascii_digit())
            || matches!(expected.as_str(), "true" | "false")
        {
            raw.clone()
        } else {
            enum_tail(raw)
        };
        assert_eq!(actual, expected, "schema 默认值与 Settings 不一致：{name}");
        checked += 1;
    }
    assert_eq!(
        checked, 15,
        "应逐项核对 15 个引擎设置（两个三态项各承载两个布尔字段：19 个字段 = 17 项）"
    );
}

/// 反向守护：上面那条测试只保证「schema 里出现的项与 `Settings` 一致」，
/// 是**单向**的——新增一个 `Settings` 字段而不写进 `shell/hux.cpp` 的 schema 不会失败。
/// 本测试补上另一向：`Settings` 的每个字段都必须在 schema 中声明，反之 schema 里除
/// `HOST_ONLY_PATHS`（只服务宿主显示、不经引擎的项）外不得出现引擎不认识的路径。
///
/// 表内每项都用 `offset_of!` 引用真实字段名 ⇒ **改名字段即编译失败**；`FIELDS.len()` 被钉住
/// ⇒ 新增字段必须同步本表与配置页（否则此测试先红）。这正是本表要堵的漂移入口。
/// 两个三态项各承载两个字段（「提前上屏」↔ `early_commit` / `early_commit_to_preedit`、
/// 「标点」↔ `ascii_punct` / `full_shape`），故路径列允许重复、比对前对期望集合去重。
#[test]
fn every_settings_field_is_declared_in_the_schema() {
    // （字段名，承载它的 schema 路径名，偏移）。顺序 = `Settings` 声明序。
    const FIELDS: &[(&str, &str, usize)] = &[
        (
            "early_commit",
            "EarlyCommitMode",
            std::mem::offset_of!(Settings, early_commit),
        ),
        (
            "early_commit_to_preedit",
            "EarlyCommitMode",
            std::mem::offset_of!(Settings, early_commit_to_preedit),
        ),
        (
            "allow_duplicate_single",
            "AllowDuplicateSingle",
            std::mem::offset_of!(Settings, allow_duplicate_single),
        ),
        (
            "full_shape",
            "PunctMode",
            std::mem::offset_of!(Settings, full_shape),
        ),
        (
            "ascii_punct",
            "PunctMode",
            std::mem::offset_of!(Settings, ascii_punct),
        ),
        (
            "learning_on_tab",
            "TabLearning",
            std::mem::offset_of!(Settings, learning_on_tab),
        ),
        (
            "digit_select",
            "DigitSelect",
            std::mem::offset_of!(Settings, digit_select),
        ),
        (
            "full_charset",
            "FullCharset",
            std::mem::offset_of!(Settings, full_charset),
        ),
        (
            "filter_non_han",
            "FilterNonHan",
            std::mem::offset_of!(Settings, filter_non_han),
        ),
        (
            "page_cycle",
            "PageCycle",
            std::mem::offset_of!(Settings, page_cycle),
        ),
        (
            "high_freq_limit",
            "HighFreqLimit",
            std::mem::offset_of!(Settings, high_freq_limit),
        ),
        (
            "page_size",
            "PageSize",
            std::mem::offset_of!(Settings, page_size),
        ),
        (
            "min_retained_input_length",
            "MinRetainedRawLength",
            std::mem::offset_of!(Settings, min_retained_input_length),
        ),
        (
            "candidate_layout",
            "CandidateLayout",
            std::mem::offset_of!(Settings, candidate_layout),
        ),
        (
            "preedit_mode",
            "PreeditMode",
            std::mem::offset_of!(Settings, preedit_mode),
        ),
        (
            "page_up_keys",
            "PageUpKey",
            std::mem::offset_of!(Settings, page_up_keys),
        ),
        (
            "page_down_keys",
            "PageDownKey",
            std::mem::offset_of!(Settings, page_down_keys),
        ),
        (
            "reverse_lookup_pronunciation_keys",
            "SoundToCharShapeKey",
            std::mem::offset_of!(Settings, reverse_lookup_pronunciation_keys),
        ),
        (
            "reverse_lookup_character_keys",
            "CharToSoundShapeKey",
            std::mem::offset_of!(Settings, reverse_lookup_character_keys),
        ),
    ];

    // 只服务宿主显示、不经引擎的 schema 项（与上一条测试的 `_ => continue` 一致）。
    const HOST_ONLY_PATHS: &[&str] = &["PanelPreedit"];

    assert_eq!(
        FIELDS.len(),
        19,
        "Settings 字段数变化：新增/删除字段必须同步本表与 shell/hux.cpp 的 schema（或将新增项登记为宿主显示项）"
    );
    // 字段顺序由 `repr(Rust)` 决定（编译器会重排），故**不假设**「声明序 == 偏移序」；
    // 只要求偏移互异且落在结构体内——重复登记或张冠李戴都会被抓住。
    let mut offsets: Vec<usize> = FIELDS.iter().map(|(_, _, offset)| *offset).collect();
    let size = std::mem::size_of::<Settings>();
    assert!(
        offsets.iter().all(|offset| *offset < size),
        "FIELDS 中有偏移越界项（size_of::<Settings>() = {size}）"
    );
    offsets.sort_unstable();
    offsets.dedup();
    assert_eq!(offsets.len(), FIELDS.len(), "FIELDS 中两个字段指向同一偏移");

    let source =
        std::fs::read_to_string(repo_path("platform/fcitx5/shell/hux.cpp")).expect("read hux.cpp");
    let mut declared: Vec<&str> = source
        .split(".path{\"")
        .skip(1)
        .filter_map(|part| part.split('"').next())
        .collect();
    declared.sort_unstable();
    declared.dedup();

    for (field, path, _) in FIELDS {
        assert!(
            declared.contains(path),
            "Settings::{field} 未在 shell/hux.cpp 的 schema 中声明（新增字段须同步配置页，\
             或在 HOST_ONLY_PATHS 登记为宿主显示项）"
        );
    }
    let mut engine_paths: Vec<&str> = declared
        .iter()
        .copied()
        .filter(|path| !HOST_ONLY_PATHS.contains(path))
        .collect();
    engine_paths.sort_unstable();
    let mut expected: Vec<&str> = FIELDS.iter().map(|(_, path, _)| *path).collect();
    expected.sort_unstable();
    // 一个三态项承载两个字段 ⇒ 期望集合先去重；`declared` 本身已去重。
    expected.dedup();
    assert_eq!(
        engine_paths, expected,
        "schema 路径集合与 Settings 字段表不一致（双向守护：两侧都必须有对方）"
    );
}

/// 三态项（「提前上屏」「标点映射」）↔ 引擎四个布尔开关的映射守卫（源码级）。
///
/// 三态**只存在于配置页与状态菜单的两个单选子菜单之间**：引擎侧仍是 `hux_options` 的四个
/// `int32_t`（`HUX_OPTION_*` 角色与 ABI 一律不动）。折算规则写在 C++ 里、cargo 测试执行不到，
/// 故按源码文本把三条链路钉住：schema（路径 / 行标签 / 三项显示名 / 默认态 / 声明位置）、
/// `applyConfig()`（枚举 → 四个布尔，配置页与子菜单**共用的唯一推送口径**）、三个折算式
/// （角色 → 枚举，只剩「启动对齐」与布尔项镜像在用）。
/// 回滚本次改动（恢复四个布尔项）本测试即失败；另经变异核对：改折算方向、改角色指向的字段、
/// 调换表项角色序、调换对齐的两个角色都会红。
#[test]
fn tri_state_options_fold_to_engine_booleans() {
    let source =
        std::fs::read_to_string(repo_path("platform/fcitx5/shell/hux.cpp")).expect("read hux.cpp");
    // 折成单行：断行与缩进不影响断言（涉及的字符串字面量里没有空白，故折叠不改变它们）。
    let flat = source.split_whitespace().collect::<Vec<_>>().join(" ");
    // 取某个函数「签名 + 函数体」（按花括号配平，避免窗口切进下一个函数）。
    let function = |signature: &str| -> String {
        let start = flat
            .find(signature)
            .unwrap_or_else(|| panic!("hux.cpp 缺少 {signature}"));
        let open = start
            + flat[start..]
                .find('{')
                .unwrap_or_else(|| panic!("{signature} 没有函数体"));
        let mut depth = 0i32;
        for (offset, ch) in flat[open..].char_indices() {
            match ch {
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        return flat[start..open + offset + 1].to_string();
                    }
                }
                _ => {}
            }
        }
        panic!("{signature} 的花括号不配平");
    };
    // 取某段源码（从签名到其后的第一个 `};`，用于表 / 文案表这种聚合块）。
    let block = |signature: &str| -> &str {
        let start = flat
            .find(signature)
            .unwrap_or_else(|| panic!("hux.cpp 缺少 {signature}"));
        let end = start
            + flat[start..]
                .find("};")
                .unwrap_or_else(|| panic!("{signature} 未闭合"));
        &flat[start..end]
    };

    // 1) 两个三态项：行标签 + 三项显示名（两侧注解必须逐字相同，宏自带 static_assert）。
    //    显示名同时是托盘子菜单的文案（经注解 `toString` 取），故这些串改一处即两处生效。
    for declaration in [
        ".description{\"提前上屏\"}",
        ".description{\"标点映射\"}",
        "FCITX_CONFIG_ENUM_NAME(HuxEarlyCommitMode, \"关闭\", \"至输出\", \"至预编辑串\");",
        "FCITX_CONFIG_ENUM_I18N_ANNOTATION(HuxEarlyCommitMode, \"关闭\", \"至输出\", \"至预编辑串\");",
        "FCITX_CONFIG_ENUM_NAME(HuxPunctMode, \"关闭（半角）\", \"全角（常用）\", \"全角（all）\");",
        "FCITX_CONFIG_ENUM_I18N_ANNOTATION(HuxPunctMode, \"关闭（半角）\", \"全角（常用）\", \"全角（all）\");",
    ] {
        assert!(
            flat.contains(declaration),
            "三态项的 schema 声明缺失：{declaration}"
        );
    }
    // 声明位置：两个三态项在「行为」分区**末尾**（布尔项 → 值选项 → 其余枚举 → 三态项）。
    let preedit = flat
        .find(".path{\"PreeditMode\"}")
        .expect("schema 缺少 PreeditMode");
    for path in [".path{\"EarlyCommitMode\"}", ".path{\"PunctMode\"}"] {
        let at = flat
            .find(path)
            .unwrap_or_else(|| panic!("schema 缺少 {path}"));
        assert!(
            preedit < at,
            "{path} 必须排在 PreeditMode 之后（三态项在分区末尾）：{at} <= {preedit}"
        );
    }

    // 2) 默认态 = 现状：提前上屏「至输出」（`early_commit` 开、不进预编辑）、标点「全角（常用）」
    //    （两个标点开关皆关）。
    assert_eq!(
        schema_default("EarlyCommitMode"),
        "HuxEarlyCommitMode::ToOutput"
    );
    assert_eq!(schema_default("PunctMode"), "HuxPunctMode::FullShapeCommon");

    // 3) 配置页推送：枚举 → 四个布尔（`hux_options` 的字段名不变）。
    for folded in [
        "options.early_commit = earlyCommitMode == HuxEarlyCommitMode::Off ? 0 : 1;",
        "options.early_commit_to_preedit = earlyCommitMode == HuxEarlyCommitMode::ToPreedit ? 1 : 0;",
        "options.ascii_punct = punctMode == HuxPunctMode::Ascii ? 1 : 0;",
        "options.full_shape = punctMode == HuxPunctMode::FullShapeAll ? 1 : 0;",
    ] {
        assert!(
            flat.contains(folded),
            "applyConfig() 的三态折算与契约不符：{folded}"
        );
    }

    // 4) 角色级折算式（枚举 ↔ 引擎开关值）：只剩两条调用路径——启动对齐按角色补值、
    //    布尔项托盘开关镜像；三态托盘子菜单不走它（直接写 schema，见下一个测试）。
    let early_commit = function("constexpr HuxEarlyCommitMode toggleEarlyCommit(");
    assert!(
        early_commit.contains("if (!on) { return HuxEarlyCommitMode::Off; }"),
        "关「提前上屏」必须落「关闭」：{early_commit}"
    );
    assert!(
        early_commit.contains(
            "return mode == HuxEarlyCommitMode::Off ? HuxEarlyCommitMode::ToOutput : mode;"
        ),
        "开「提前上屏」只在「关闭」时落到「至输出」（不夺「至预编辑串」）：{early_commit}"
    );
    assert!(
        !early_commit.contains("ToPreedit"),
        "「提前上屏」的翻转不得改动「至预编辑串」：{early_commit}"
    );

    let to_preedit = function("constexpr HuxEarlyCommitMode toggleEarlyCommitToPreedit(");
    assert!(
        to_preedit.contains("if (on) { return HuxEarlyCommitMode::ToPreedit; }"),
        "开「提前上屏至预编辑」必须落「至预编辑串」：{to_preedit}"
    );
    assert!(
        to_preedit.contains(
            "return mode == HuxEarlyCommitMode::ToPreedit ? HuxEarlyCommitMode::ToOutput : mode;"
        ),
        "关「提前上屏至预编辑」只把「至预编辑串」降为「至输出」、其余态保持\
         （无条件降级会在启动对齐里复活刚定下的「关闭」）：{to_preedit}"
    );

    let punct = function("constexpr HuxPunctMode togglePunct(");
    assert!(
        punct.contains("return on ? HuxPunctMode::FullShapeAll : HuxPunctMode::FullShapeCommon;"),
        "标点折算式：开 ⇒ 「全角（all）」、关 ⇒ 「全角（常用）」：{punct}"
    );
    assert!(
        !punct.contains("Ascii"),
        "标点折算式不得产出 Ascii（「关闭（半角）」由子菜单直接写 schema，不经角色级折算）：{punct}"
    );

    // 5) 角色 → 字段 + 写回：两个「提前上屏」角色共用同一个三态项、「标点映射」指向标点三态项
    //    （其余两个角色照旧直写）；顺序即 `HUX_OPTION_*` 角色序（启动对齐按它逐个折算）。
    let table = block("kSharedBehaviorOptions[] = {");
    let mut cursor = 0usize;
    let mut entries: Vec<(&str, &str)> = Vec::new();
    for (role, field, write) in [
        (
            "HUX_OPTION_EARLY_COMMIT",
            "earlyCommitMode",
            "toggleEarlyCommit(",
        ),
        (
            "HUX_OPTION_EARLY_COMMIT_TO_PREEDIT",
            "earlyCommitMode",
            "toggleEarlyCommitToPreedit(",
        ),
        (
            "HUX_OPTION_ALLOW_DUPLICATE_SINGLE",
            "allowDuplicateSingle",
            "config.allowDuplicateSingle.setValue(on)",
        ),
        ("HUX_OPTION_FULL_SHAPE", "punctMode", "togglePunct(on)"),
        (
            "HUX_OPTION_DIGIT_SELECT",
            "digitSelect",
            "config.digitSelect.setValue(on)",
        ),
    ] {
        let marker = format!("{{{role},");
        let at = table[cursor..]
            .find(&marker)
            .map(|index| index + cursor)
            .unwrap_or_else(|| panic!("kSharedBehaviorOptions 缺少角色或角色序错乱：{role}"));
        cursor = at + marker.len();
        // 本项到下一项（表项均以 `{HUX_OPTION_` 开头）之间的文本片段。
        let next = table[cursor..]
            .find("{HUX_OPTION_")
            .map(|index| index + cursor)
            .unwrap_or(table.len());
        let entry = &table[at..next];
        assert!(
            entry.contains(&format!("return config.{field}.path();")),
            "角色 {role} 的配置路径未指向 schema 的 {field}：{entry}"
        );
        assert!(
            entry.contains(write),
            "角色 {role} 的写回未经过 {write}：{entry}"
        );
        entries.push((role, entry));
    }
    let entry = |role: &str| -> &str {
        entries
            .iter()
            .find(|(name, _)| *name == role)
            .unwrap_or_else(|| panic!("kSharedBehaviorOptions 缺少角色 {role}"))
            .1
    };

    // 6) 文案表按 ABI 角色下标取，长度仍是 `HUX_OPTION_COUNT`：三个已被三态子菜单取代的角色
    //    文案**保留**（抽掉任一项都会让后续下标整体错位），此处一并钉住长度与内容。
    let labels = block("kLabels[] = {");
    for label in [
        "\"提前上屏\"",
        "\"提前上屏至预编辑\"",
        "\"单字重码组句\"",
        "\"全角标点\"",
        "\"数字直选\"",
        "\"启用全字集\"",
        "\"过滤非汉字\"",
    ] {
        assert!(labels.contains(label), "状态菜单文案表缺少 {label}");
    }
    assert!(
        labels.matches('"').count() == 7 * 2,
        "文案表应恰有 7 项（HUX_OPTION_COUNT）：{labels}"
    );

    // 7) 旧布尔项不得残留：四个 schema 项已被两个三态项取代（回滚本次改动即在此失败）。
    for stale in [
        ".path{\"EarlyCommit\"}",
        ".path{\"EarlyCommitToPreedit\"}",
        ".path{\"FullShape\"}",
        ".path{\"AsciiPunct\"}",
        "earlyCommitToPreedit",
        "fullShape",
        "asciiPunct",
    ] {
        assert!(!flat.contains(stale), "仍残留旧布尔项：{stale}");
    }

    // 8) 启动对齐：**总闸角色最后折算**（行为保真）。`(0,1)`（旧配置页两个独立勾选框可造出：
    //    提前上屏关 + 至预编辑开）的有效行为是「不提前上屏」，按角色序一趟折完会把它重新打开。
    //    真值在 C++ 侧以 `static_assert` 于**编译期**钉住（`cmake --build` 即校验），这里再断言
    //    那四条仍在（删掉即红），且「子角色先、总闸最后」的次序与运行时两趟一致。
    assert!(
        entry("HUX_OPTION_EARLY_COMMIT").contains("/*gate=*/true"),
        "「提前上屏」必须标为这一对的总闸（启动对齐最后折算）"
    );
    assert!(
        !entry("HUX_OPTION_EARLY_COMMIT_TO_PREEDIT").contains("/*gate=*/true"),
        "「提前上屏至预编辑」不是总闸（它必须先于总闸折算）"
    );
    let adopt = function("constexpr HuxEarlyCommitMode adoptEarlyCommitMode(");
    let sub_first = adopt
        .find("toggleEarlyCommitToPreedit(HuxEarlyCommitMode::ToOutput, toPreedit)")
        .unwrap_or_else(|| panic!("对齐真值复算未先折子角色：{adopt}"));
    let gate_last = adopt
        .find("return toggleEarlyCommit(mode, earlyCommit);")
        .unwrap_or_else(|| panic!("对齐真值复算未最后折总闸：{adopt}"));
    assert!(
        sub_first < gate_last,
        "对齐真值复算必须先折「至预编辑串」再折总闸：{adopt}"
    );
    for truth in [
        "static_assert(adoptEarlyCommitMode(false, false) == HuxEarlyCommitMode::Off,",
        "static_assert(adoptEarlyCommitMode(false, true) == HuxEarlyCommitMode::Off,",
        "static_assert(adoptEarlyCommitMode(true, false) == HuxEarlyCommitMode::ToOutput,",
        "static_assert(adoptEarlyCommitMode(true, true) == HuxEarlyCommitMode::ToPreedit,",
    ] {
        assert!(flat.contains(truth), "启动对齐的四组合真值缺一条：{truth}");
    }
    let adopt_loop = function("void adoptStoredRuntimeOptions()");
    assert!(
        adopt_loop.contains("for (const bool gatePass : {false, true}) {")
            && adopt_loop.contains("shared.gate != gatePass"),
        "启动对齐必须是两趟（非总闸先、总闸最后）：{adopt_loop}"
    );

    // 9) 镜像路径只剩布尔项：schema 写回 → 落盘 → 刷新（推送由 `HuxToggleAction` 自己做）。
    let mirror = function("void mirrorRuntimeRole(");
    let saved = mirror
        .find("fcitx::safeSaveAsIni(config_, kConfigPath)")
        .unwrap_or_else(|| panic!("布尔项镜像未落盘：{mirror}"));
    let refresh = mirror
        .find("refreshStatusAreas(inputContext);")
        .unwrap_or_else(|| panic!("布尔项镜像未刷新勾选态：{mirror}"));
    assert!(
        saved < refresh,
        "顺序应为：写 schema → 落盘 → 刷新：{mirror}"
    );
}

/// 托盘（状态菜单）结构守卫（源码级）：全部条目平铺为状态区同级条目 + 两个三态**单选子菜单**。
///
/// 覆盖用户确认的结构与五条语义（见提交 `371baf88`、`0e2cbe29`）：
///   ① 两个子菜单各三项、文案 = 配置页显示名（逐字一致）、单选（恒一勾）；
///   ② 子菜单当前态取自 **schema**（不是引擎选项——「标点映射」尤其：引擎没有 `ascii_punct` 角色）；
///   ③ 选中即「写 schema → 落盘 → 推送」，推送口径与配置页**同一个** `applyConfig()`；
///   ④ 配置页保存 / 重新加载 / 重新部署都会刷新子菜单勾选态；
///   ⑤ 托盘不再有「虎虚」菜单（平铺）、「虎虚」只是图标 + 标题、每个条目都注册进状态区。
#[test]
fn tray_flattens_entries_and_builds_tri_state_submenus() {
    let source =
        std::fs::read_to_string(repo_path("platform/fcitx5/shell/hux.cpp")).expect("read hux.cpp");
    let flat = source.split_whitespace().collect::<Vec<_>>().join(" ");
    let function = |signature: &str| -> String {
        let start = flat
            .find(signature)
            .unwrap_or_else(|| panic!("hux.cpp 缺少 {signature}"));
        let open = start
            + flat[start..]
                .find('{')
                .unwrap_or_else(|| panic!("{signature} 没有函数体"));
        let mut depth = 0i32;
        for (offset, ch) in flat[open..].char_indices() {
            match ch {
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        return flat[start..open + offset + 1].to_string();
                    }
                }
                _ => {}
            }
        }
        panic!("{signature} 的花括号不配平");
    };

    // ① + ②) 两个子菜单：父项标签、菜单来源（枚举注解 ⇒ 三项、文案逐字同配置页）、单选。
    let builder = function("void addModeMenu(");
    assert!(
        builder.contains("for (size_t index = 0; index < Annotation::enumLength; ++index) {")
            && builder.contains("const Enum mode = static_cast<Enum>(index);"),
        "子菜单项必须按枚举声明序**逐项**生成（三项 = 枚举项数）：{builder}"
    );
    assert!(
        builder.contains("Annotation::toString(mode)"),
        "子菜单文案必须取枚举注解显示名（与配置页下拉逐字一致）：{builder}"
    );
    assert!(
        builder.contains("[current, mode] { return current() == mode; }"),
        "每项的勾选态 = 「当前态 == 本项」，值互异且覆盖整枚举 ⇒ 恒有且仅有一项打勾：{builder}"
    );
    assert!(
        builder.contains("parentAction.setMenu(&menu);")
            && builder.contains("menu.addAction(item.get());")
            && builder.contains("items.push_back(std::move(item));"),
        "子菜单必须挂到父项并逐项入菜单：{builder}"
    );
    let mode_action = function("class HuxModeAction");
    assert!(
        mode_action.contains("setCheckable(true);")
            && mode_action.contains("bool isChecked(fcitx::InputContext * /*unused*/) const override { return selected_(); }"),
        "单选项必须是可勾选项、勾选态现算：{mode_action}"
    );
    assert!(
        !mode_action.contains("hux_engine_option_value"),
        "③/② 单选项的勾选态不得读引擎选项（三态的事实来源是 schema）：{mode_action}"
    );
    for (enum_type, annotation, parent_label, prefix, field) in [
        (
            "HuxEarlyCommitMode",
            "HuxEarlyCommitModeI18NAnnotation",
            "\"提前上屏\"",
            "\"hux-early-commit\"",
            "earlyCommitMode",
        ),
        (
            "HuxPunctMode",
            "HuxPunctModeI18NAnnotation",
            "\"标点映射\"",
            "\"hux-punct\"",
            "punctMode",
        ),
    ] {
        let call = function(&format!("addModeMenu<{enum_type}, {annotation}>("));
        assert!(
            call.contains(&format!("config_.behavior->{field}.value()")),
            "② {parent_label} 子菜单的当前态必须取自 schema 字段 {field}：{call}"
        );
        assert!(
            !call.contains("hux_engine_option_value"),
            "② {parent_label} 子菜单不得以引擎选项为当前态（标点映射尤其）：{call}"
        );
        assert!(
            call.contains(parent_label) && call.contains(prefix),
            "① 子菜单父项标签/注册名不符（应为 {parent_label} / {prefix}）：{call}"
        );
    }

    // ③) 选中即「写 schema → 落盘 → 推送 → 刷新」，推送口径与配置页同一个 `applyConfig()`。
    for (choose, field) in [
        ("void chooseEarlyCommitMode(", "earlyCommitMode"),
        ("void choosePunctMode(", "punctMode"),
    ] {
        let body = function(choose);
        assert!(
            body.contains(&format!(
                "config_.behavior.mutableValue()->{field}.setValue(mode);"
            )),
            "③ 选中必须写 schema 字段 {field}：{body}"
        );
        assert!(
            body.contains("commitModeChoice(inputContext);"),
            "③ 选中必须走统一的落地入口：{body}"
        );
    }
    let commit = function("void commitModeChoice(");
    let saved = commit
        .find("fcitx::safeSaveAsIni(config_, kConfigPath)")
        .unwrap_or_else(|| panic!("③ 子菜单选中未落盘：{commit}"));
    let pushed = commit
        .find("applyConfig();")
        .unwrap_or_else(|| panic!("③ 子菜单选中未推送给引擎：{commit}"));
    let refreshed = commit
        .find("refreshStatusAreas(inputContext);")
        .unwrap_or_else(|| panic!("③ 子菜单选中未刷新勾选态：{commit}"));
    assert!(
        saved < pushed && pushed < refreshed,
        "③ 顺序必须是：落盘 → 推送 → 刷新：{commit}"
    );
    for (entry, call) in [
        ("void setConfig(", "applyConfig();"),
        ("void reloadConfig() override", "applyConfig();"),
    ] {
        assert!(
            function(entry).contains(call),
            "③ 配置页保存/重载与子菜单必须同一推送口径（{call}）：{entry}"
        );
    }

    // ④) 配置页保存 / 重新加载 / 重新部署都刷新状态区条目（子菜单勾选态取自 schema）。
    for (entry, call) in [
        ("void setConfig(", "refreshStatusAreas();"),
        ("void reloadConfig() override", "refreshStatusAreas();"),
        ("void redeploy(", "refreshStatusAreas(inputContext);"),
    ] {
        assert!(
            function(entry).contains(call),
            "④ {entry} 必须刷新状态区条目：{}",
            function(entry)
        );
    }
    let refresh_all = function("void refreshStatusAreas(");
    assert!(
        refresh_all.contains("refreshActions(inputContext);")
            && refresh_all.contains("instance_->inputContextManager().foreach("),
        "④ 没有单一输入上下文（配置页路径）时应对每个 IC 各刷一遍：{refresh_all}"
    );
    let refresh_one = function("void refreshActions(");
    assert!(
        refresh_one.contains("for (fcitx::Action *action : statusActions_) {")
            && refresh_one.contains("action->update(inputContext);"),
        "④ 刷新必须逐个通知状态区条目重取勾选态：{refresh_one}"
    );

    // ⑤) 平铺：没有「虎虚」菜单（`menu_`/`menuAction_.setMenu` 全无），`setMenu` 只用于两个子菜单。
    assert!(
        !flat.contains("fcitx::Menu menu_;") && !flat.contains("menuAction_.setMenu"),
        "「虎虚」不再挂菜单（全部条目平铺为同级条目）"
    );
    assert_eq!(
        flat.matches("setMenu(").count(),
        1,
        "setMenu 只应出现在子菜单构造器里（父项挂自己那份菜单）"
    );
    let update = function("void updateStatusArea(");
    assert!(
        update.contains("for (fcitx::Action *action : statusActions_) {")
            && update.contains("statusArea.addAction(fcitx::StatusGroup::InputMethod, action);"),
        "⑤ 每个条目都要作为状态区同级条目挂上（顺序 = `statusActions_`）：{update}"
    );
    let register = function("void registerStatusAction(");
    assert!(
        register.contains("instance_->userInterfaceManager().registerAction(name, &action);")
            && register.contains("statusActions_.push_back(&action);"),
        "⑤ 注册进 UserInterfaceManager 的同时记入显示顺序：{register}"
    );
    // 注册顺序 = 显示顺序：虎虚（图标锚点 + 模型状态）→ 四个布尔开关 → 宿主开关 → 两个子菜单 → 重新部署。
    let setup = function("void setupStatusMenu()");
    let mut cursor = 0usize;
    for marker in [
        "registerStatusAction(\"hux-menu\", *menuAction_);",
        "registerStatusAction(std::string(\"hux-\") + option, *action);",
        "registerStatusAction(\"hux-panel-preedit\", *panelPreeditAction_);",
        "addModeMenu<HuxEarlyCommitMode",
        "addModeMenu<HuxPunctMode",
        "registerStatusAction(\"hux-redeploy\", *redeployAction_);",
    ] {
        assert_eq!(
            setup.matches(marker).count(),
            1,
            "状态区条目应恰好注册一次（{marker}）：{setup}"
        );
        let at = setup[cursor..]
            .find(marker)
            .map(|index| index + cursor)
            .unwrap_or_else(|| panic!("状态区条目注册顺序不符（缺 {marker}）：{setup}"));
        cursor = at + marker.len();
    }
    // ⑥) 动态文案：模型信息并入首项「虎虚」，两个子菜单父项显示「名称：当前值」。
    assert!(
        setup.contains("updateDynamicLabels();"),
        "⑤ 注册完成后要先算一次动态文案：{setup}"
    );
    assert!(
        !setup.contains("hux-model") && !setup.contains("modelAction_"),
        "⑥ 独立「模型」条目已并入首项，不应再有：{setup}"
    );
    let dynamic_labels = function("void updateDynamicLabels()");
    assert!(
        setup.contains("return std::string(\"虎虚：\") + modelText();"),
        "⑥ 「虎虚」首项应带模型状态（首项是可点的 `HuxHostAction`，文案由 label 函数现算）：{setup}"
    );
    assert!(
        dynamic_labels.contains("\"提前上屏：\" +") && dynamic_labels.contains("\"标点映射：\" +"),
        "⑥ 两个子菜单父项应显示「名称：当前值」：{dynamic_labels}"
    );
    assert!(
        dynamic_labels.contains("HuxEarlyCommitModeI18NAnnotation::toString(")
            && dynamic_labels.contains("HuxPunctModeI18NAnnotation::toString("),
        "⑥ 当前值必须取枚举注解的显示名（与配置页下拉项同源）：{dynamic_labels}"
    );
    let refresh_actions = function("void refreshActions(fcitx::InputContext *inputContext)");
    assert!(
        refresh_actions.contains("updateDynamicLabels();"),
        "⑥ 每次刷新都要重算动态文案（模型与三态都会变）：{refresh_actions}"
    );
    // 三态角色跳过布尔开关（它们由子菜单承载）；文案表长度仍是 ABI 角色数。
    assert!(
        setup.contains("if (isTriStateRole(role)) {")
            && setup.contains("continue;")
            && setup.contains("static_assert(std::size(kLabels) == HUX_OPTION_COUNT,"),
        "⑤ 三态角色不得再作为布尔开关进托盘：{setup}"
    );
    let tri_role = function("static constexpr bool isTriStateRole(");
    for role in [
        "HUX_OPTION_EARLY_COMMIT",
        "HUX_OPTION_EARLY_COMMIT_TO_PREEDIT",
        "HUX_OPTION_FULL_SHAPE",
    ] {
        assert!(
            tri_role.contains(&format!("role == {role}")),
            "⑤ 三态角色清单缺 {role}：{tri_role}"
        );
    }
}

/// 「虎虚」首项（模型入口）守卫（源码级）：
///   ① 文案 = 引擎给的那段原文（**不自带**「模型：」，否则与首项前缀叠成「虎虚：模型：…」）；
///   ② 首项**可点**：打开「所加载模型所在目录」，且仍带 `hux` 图标、仍登记在 `statusActions_`；
///   ③ 打开目录 = `hux_engine_model_path` → `parent_path` → `create_directories` → 双 fork
///      `execlp`（先 `xdg-open`、再 `gio open`），**不得**用 `std::system`（路径会经 shell 解释）；
///   ④ ABI 入口在头文件里声明、在 `abi.rs` 里导出（CI 另有 `nm -D` ↔ 头文件的动态比对）。
#[test]
fn model_entry_opens_the_model_directory() {
    let source =
        std::fs::read_to_string(repo_path("platform/fcitx5/shell/hux.cpp")).expect("read hux.cpp");
    let flat = source.split_whitespace().collect::<Vec<_>>().join(" ");
    let function = |signature: &str| -> String {
        let start = flat
            .find(signature)
            .unwrap_or_else(|| panic!("hux.cpp 缺少 {signature}"));
        let open = start
            + flat[start..]
                .find('{')
                .unwrap_or_else(|| panic!("{signature} 没有函数体"));
        let mut depth = 0i32;
        for (offset, ch) in flat[open..].char_indices() {
            match ch {
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        return flat[start..open + offset + 1].to_string();
                    }
                }
                _ => {}
            }
        }
        panic!("{signature} 的花括号不配平");
    };

    // ① 文案：引擎原文 + 短兜底，不带任何前缀；「虎虚：」只在调用方加一次。
    let text = function("std::string modelText() const");
    assert!(
        text.contains(r#"return info != nullptr ? std::string(info) : std::string("不可用");"#),
        "modelText() 必须只回引擎原文（nullptr 时短兜底）：{text}"
    );
    assert!(
        !text.contains("模型："),
        "modelText() 不得自带前缀（会与首项前缀叠成「虎虚：模型：…」）：{text}"
    );
    assert_eq!(
        flat.matches("+ modelText()").count(),
        1,
        "模型短名只应由首项文案消费（前缀各加一次）"
    );
    assert!(
        flat.contains(r#"return std::string("虎虚：") + modelText();"#),
        "首项文案 = 「虎虚：」+ 引擎原文（前缀只加一次）"
    );

    // ② 首项可点：HuxHostAction（带 activate）+ hux 图标 + 登记进状态区。
    assert!(
        flat.contains("menuAction_ = std::make_unique<HuxHostAction>(")
            && flat.contains("openModelDirectory(inputContext);")
            && flat.contains(r#"registerStatusAction("hux-menu", *menuAction_);"#),
        "首项必须是可点动作（点击打开模型目录）且仍登记在 statusActions_ 里"
    );
    let host_action = function("class HuxHostAction");
    assert!(
        host_action.contains(
            "std::string icon(fcitx::InputContext * /*unused*/) const override { return icon_; }"
        ),
        "首项必须保留 hux 图标（`icon_` 由构造参数给出）：{host_action}"
    );
    assert!(flat.contains(r#""hux");"#), "首项构造必须传入 hux 图标名");

    // ③ 打开目录：路径 → 父目录 → 建目录 → 拉起文件管理器。
    let open = function("void openModelDirectory(");
    for fragment in [
        "const char *path = hux_engine_model_path(engine_);",
        "std::filesystem::path(path).parent_path()",
        "std::filesystem::create_directories(directory, error)",
        "launchFileManager(directory.string())",
    ] {
        assert!(
            open.contains(fragment),
            "打开模型目录缺少 {fragment}：{open}"
        );
    }
    let launch = function("static bool launchFileManager(");
    for fragment in [
        "const pid_t child = fork();",
        "const pid_t grandchild = fork();",
        r#"execlp("xdg-open", "xdg-open", directory.c_str(),"#,
        r#"execlp("gio", "gio", "open", directory.c_str(),"#,
        "waitpid(child, &status, 0)",
    ] {
        assert!(
            launch.contains(fragment),
            "拉起文件管理器缺少 {fragment}：{launch}"
        );
    }
    // 注释里会**提到** `std::system`（说明为何不用），故只按「调用形态」判定（带参数括号）。
    for forbidden in [
        "std::system(",
        "system(",
        "popen(",
        "execl(",
        "execlp(\"/bin/sh\"",
    ] {
        assert!(
            !flat.contains(forbidden),
            "不得用 {forbidden} 拉进程（路径经 shell 解释 = 注入面）"
        );
    }

    // ④ ABI：头文件声明（含指针有效期语义）+ Rust 导出。
    let header = std::fs::read_to_string(repo_path("crates/hux-ffi/include/hux_abi.h"))
        .expect("read hux_abi.h");
    // 逐行比对（含缩进）：注释掉的声明不算数（`contains` 会匹配注释里的同一行）。
    assert!(
        header
            .lines()
            .any(|line| line.trim()
                == "const char *hux_engine_model_path(const hux_engine *engine);"),
        "hux_abi.h 缺少（未被注释的）hux_engine_model_path 声明"
    );
    assert!(
        header
            .contains("未找到模型 ⇒ 默认查找路径（其父目录即「模型该放的地方」，文件可以不存在）")
            && header.contains("指针有效期同 hux_engine_model_info"),
        "hux_engine_model_path 的语义（未找到时给「该放的位置」+ 指针有效期）必须写进头文件"
    );
    let abi =
        std::fs::read_to_string(repo_path("platform/fcitx5/src/abi.rs")).expect("read abi.rs");
    let abi_flat = abi.split_whitespace().collect::<Vec<_>>().join(" ");
    assert!(
        abi_flat.contains(
            "#[unsafe(no_mangle)] pub unsafe extern \"C\" fn hux_engine_model_path(engine: *const Engine) -> *const c_char"
        ),
        "abi.rs 必须导出 hux_engine_model_path（否则 nm -D 与头文件不一致）"
    );
    assert!(
        abi_flat.contains(".map_or(std::ptr::null(), |path| path.as_ptr())"),
        "空路径必须回 NULL（宿主据此直接返回，不去开目录）"
    );
}

/// C++ 壳的配置生命周期（源码级守卫，同 `schema_defaults_match_settings_defaults` 的风格）：
/// `setConfig` 必须 `safeSaveAsIni` 落盘、`reloadConfig` 必须重读文件。
///
/// 依据：fcitx5 的 D-Bus `Controller1::SetConfig` 只调 `addonInstance->setConfig(config)`、
/// **不代写配置文件**（`fcitx5/src/modules/dbus/dbusmodule.cpp`），落盘归 addon；而基类
/// `reloadConfig()` 是空实现（`fcitx/addoninstance.h`）。不落盘时配置页的改动只活在内存 +
/// `options.yaml` 里，任何**文件里显式写过**的键都会在下次启动被
/// `adoptStoredRuntimeOptions()` 当权威、把配置页的改动静默压回（「勾选后没有效果」），
/// 不进 `options.yaml` 的项（ASCII 直通 / 快捷键 / 页大小 / 候选排列 / 预编辑内容 /
/// 翻页循环 / 最短保留码数 / 高频上限 / Tab 学习）则直接丢失。
#[test]
fn host_config_page_saves_and_reloads_the_addon_config() {
    let source =
        std::fs::read_to_string(repo_path("platform/fcitx5/shell/hux.cpp")).expect("read hux.cpp");
    let body = |signature: &str| -> String {
        let start = source
            .find(signature)
            .unwrap_or_else(|| panic!("hux.cpp 缺少 {signature}"));
        // 按字符取窗口：源码含中文注释，字节切片会落在字符边界内。
        source[start..].chars().take(240).collect()
    };
    let set_config = body("void setConfig(const fcitx::RawConfig &raw) override");
    assert!(
        set_config.contains("safeSaveAsIni(config_, kConfigPath)"),
        "setConfig 必须落盘 conf/hux.conf（否则下次启动被旧值压回）：{set_config}"
    );
    let reload_config = body("void reloadConfig() override");
    assert!(
        reload_config.contains("readAsIni(config_, kConfigPath)"),
        "reloadConfig 必须重读 conf/hux.conf：{reload_config}"
    );
}
