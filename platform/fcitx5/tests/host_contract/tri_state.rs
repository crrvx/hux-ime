// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! 三态项（「提前上屏」「标点映射」）↔ 引擎四个布尔开关的源码级映射守卫。
//!
//! 折算规则写在 C++ 里、cargo 测试执行不到，故按 `shell/hux.cpp` 的源码文本把三条链路钉住：
//! schema 声明、`applyConfig()` 推送、角色级折算式与启动对齐。断言顺序 = 原用例的 1)～9) 小节序。

use super::schema_default;
use super::source::Source;

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
    let source = Source::read();
    tri_state_declarations(&source);
    tri_state_defaults();
    tri_state_apply_config(&source);
    tri_state_role_folding(&source);
    let entries = tri_state_role_table(&source);
    tri_state_label_table(&source);
    tri_state_no_stale_booleans(&source);
    tri_state_startup_alignment(&source, &entries);
    tri_state_mirror_path(&source);
}

/// 小节 1)：两个三态项的 schema 声明（行标签 / 三项显示名 / 声明位置）。
fn tri_state_declarations(source: &Source) {
    let flat = source.flat();
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
}

/// 小节 2)：默认态 = 现状。
fn tri_state_defaults() {
    // 2) 默认态 = 现状：提前上屏「至输出」（`early_commit` 开、不进预编辑）、标点「全角（常用）」
    //    （两个标点开关皆关）。
    assert_eq!(
        schema_default("EarlyCommitMode"),
        "HuxEarlyCommitMode::ToOutput"
    );
    assert_eq!(schema_default("PunctMode"), "HuxPunctMode::FullShapeCommon");
}

/// 小节 3)：配置页推送（枚举 → 四个布尔）。
fn tri_state_apply_config(source: &Source) {
    let flat = source.flat();
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
}

/// 小节 4)：角色级折算式（枚举 ↔ 引擎开关值）。
fn tri_state_role_folding(source: &Source) {
    // 4) 角色级折算式（枚举 ↔ 引擎开关值）：只剩两条调用路径——启动对齐按角色补值、
    //    布尔项托盘开关镜像；三态托盘子菜单不走它（直接写 schema，见下一个测试）。
    let early_commit = source.function("constexpr HuxEarlyCommitMode toggleEarlyCommit(");
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

    let to_preedit = source.function("constexpr HuxEarlyCommitMode toggleEarlyCommitToPreedit(");
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

    let punct = source.function("constexpr HuxPunctMode togglePunct(");
    assert!(
        punct.contains("return on ? HuxPunctMode::FullShapeAll : HuxPunctMode::FullShapeCommon;"),
        "标点折算式：开 ⇒ 「全角（all）」、关 ⇒ 「全角（常用）」：{punct}"
    );
    assert!(
        !punct.contains("Ascii"),
        "标点折算式不得产出 Ascii（「关闭（半角）」由子菜单直接写 schema，不经角色级折算）：{punct}"
    );
}

/// 小节 5)：角色 → 字段 + 写回；返回各角色的表项切片供小节 8) 复用。
fn tri_state_role_table(source: &Source) -> Vec<(&str, &str)> {
    // 5) 角色 → 字段 + 写回：两个「提前上屏」角色共用同一个三态项、「标点映射」指向标点三态项
    //    （其余两个角色照旧直写）；顺序即 `HUX_OPTION_*` 角色序（启动对齐按它逐个折算）。
    let table = source.block("kSharedBehaviorOptions[] = {");
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
    entries
}

/// 小节 6)：状态菜单文案表（长度与内容）。
fn tri_state_label_table(source: &Source) {
    // 6) 文案表按 ABI 角色下标取，长度仍是 `HUX_OPTION_COUNT`：三个已被三态子菜单取代的角色
    //    文案**保留**（抽掉任一项都会让后续下标整体错位），此处一并钉住长度与内容。
    let labels = source.block("kLabels[] = {");
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
}

/// 小节 7)：旧布尔项不得残留。
fn tri_state_no_stale_booleans(source: &Source) {
    let flat = source.flat();
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
}

/// 小节 8)：启动对齐（总闸角色最后折算）。
fn tri_state_startup_alignment(source: &Source, entries: &[(&str, &str)]) {
    let entry = |role: &str| -> &str {
        entries
            .iter()
            .find(|(name, _)| *name == role)
            .unwrap_or_else(|| panic!("kSharedBehaviorOptions 缺少角色 {role}"))
            .1
    };
    let flat = source.flat();
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
    let adopt = source.function("constexpr HuxEarlyCommitMode adoptEarlyCommitMode(");
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
    let adopt_loop = source.function("void adoptStoredRuntimeOptions()");
    assert!(
        adopt_loop.contains("for (const bool gatePass : {false, true}) {")
            && adopt_loop.contains("shared.gate != gatePass"),
        "启动对齐必须是两趟（非总闸先、总闸最后）：{adopt_loop}"
    );
}

/// 小节 9)：镜像路径只剩布尔项（写 schema → 落盘 → 刷新）。
fn tri_state_mirror_path(source: &Source) {
    // 9) 镜像路径只剩布尔项：schema 写回 → 落盘 → 刷新（推送由 `HuxToggleAction` 自己做）。
    let mirror = source.function("void mirrorRuntimeRole(");
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
