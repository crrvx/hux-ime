// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! 配置 schema 默认值 ↔ `hux_cfg::Settings` 的一致性与单点钉桩。
//!
//! C++ 侧默认值不参与 cargo 测试，改错了两侧都编译得过；这里解析 `shell/hux.cpp` 的
//! `.path{}` / `.defaultValue` 把它变成可断言的字符串，再与 `Settings::default()` 逐项比对。

use super::schema_default;
use hux_cfg::Settings;
use hux_test_support::repo_path;

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
    let defaults = parse_all_schema_defaults();
    compare_schema_defaults_with_settings(defaults);
}

/// 解析 `shell/hux.cpp` 里每一项的 schema 默认值（顺序 = 源码声明序）。
fn parse_all_schema_defaults() -> Vec<(String, String)> {
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
    defaults
}

// C++ 用 keysym 常量声明默认键：`fcitx::Key(FcitxKey_grave, fcitx::KeyState::NoState)`
// → rime 键名 `grave`（`FcitxKey_<name>` 即 X11 键名，与 librime 键名表同名）；
// 带 `KeyState::Alt` 的项加 `Alt+` 前缀（旧默认形态）。
fn keys(raw: &str) -> Vec<String> {
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
}

fn enum_tail(raw: &str) -> String {
    raw.rsplit("::").next().unwrap_or(raw).to_string()
}

/// 逐项比对 `defaults` 与 `Settings::default()`（三态项按引擎折算规则拼枚举名）。
fn compare_schema_defaults_with_settings(defaults: Vec<(String, String)>) {
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
