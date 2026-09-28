// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! 托盘（状态菜单）结构的源码级守卫：条目平铺 + 两个三态**单选子菜单**。
//!
//! 覆盖 ①～⑥ 六条语义（子菜单生成与当前态来源、选中落地、配置页刷新、平铺注册、动态文案），
//! 断言顺序 = 原用例的小节序。

use super::source::Source;

/// 托盘（状态菜单）结构守卫（源码级）：全部条目平铺为状态区同级条目 + 两个三态**单选子菜单**。
///
/// 覆盖用户确认的结构与五条语义：
///   ① 两个子菜单各三项、文案 = 配置页显示名（逐字一致）、单选（恒一勾）；
///   ② 子菜单当前态取自 **schema**（不是引擎选项——「标点映射」尤其：引擎没有 `ascii_punct` 角色）；
///   ③ 选中即「写 schema → 落盘 → 推送」，推送口径与配置页**同一个** `applyConfig()`；
///   ④ 配置页保存 / 重新加载 / 重新部署都会刷新子菜单勾选态；
///   ⑤ 托盘不再有「虎虚」菜单（平铺）、「虎虚」只是图标 + 标题、每个条目都注册进状态区。
#[test]
fn tray_flattens_entries_and_builds_tri_state_submenus() {
    let source = Source::read();
    tray_mode_menu_builder(&source);
    tray_mode_menu_sources(&source);
    tray_mode_choice_lands_and_pushes(&source);
    tray_config_page_refreshes(&source);
    let setup = tray_flattened_registration(&source);
    tray_dynamic_labels(&source, &setup);
}

/// ① + ②)：`addModeMenu` 的生成口径与单选项实现。
fn tray_mode_menu_builder(source: &Source) {
    // ① + ②) 两个子菜单：父项标签、菜单来源（枚举注解 ⇒ 三项、文案逐字同配置页）、单选。
    let builder = source.function("void addModeMenu(");
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
    let mode_action = source.function("class HuxModeAction");
    assert!(
        mode_action.contains("setCheckable(true);")
            && mode_action.contains("bool isChecked(fcitx::InputContext * /*unused*/) const override { return selected_(); }"),
        "单选项必须是可勾选项、勾选态现算：{mode_action}"
    );
    assert!(
        !mode_action.contains("hux_engine_option_value"),
        "③/② 单选项的勾选态不得读引擎选项（三态的事实来源是 schema）：{mode_action}"
    );
}

/// ②)：两个子菜单的挂载点、当前态来源与父项标签。
fn tray_mode_menu_sources(source: &Source) {
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
        let call = source.function(&format!("addModeMenu<{enum_type}, {annotation}>("));
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
}

/// ③)：选中即「写 schema → 落盘 → 推送 → 刷新」。
fn tray_mode_choice_lands_and_pushes(source: &Source) {
    // ③) 选中即「写 schema → 落盘 → 推送 → 刷新」，推送口径与配置页同一个 `applyConfig()`。
    for (choose, field) in [
        ("void chooseEarlyCommitMode(", "earlyCommitMode"),
        ("void choosePunctMode(", "punctMode"),
    ] {
        let body = source.function(choose);
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
    let commit = source.function("void commitModeChoice(");
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
            source.function(entry).contains(call),
            "③ 配置页保存/重载与子菜单必须同一推送口径（{call}）：{entry}"
        );
    }
}

/// ④)：配置页保存 / 重新加载 / 重新部署都刷新状态区条目。
fn tray_config_page_refreshes(source: &Source) {
    // ④) 配置页保存 / 重新加载 / 重新部署都刷新状态区条目（子菜单勾选态取自 schema）。
    for (entry, call) in [
        ("void setConfig(", "refreshStatusAreas();"),
        ("void reloadConfig() override", "refreshStatusAreas();"),
        ("void redeploy(", "refreshStatusAreas(inputContext);"),
    ] {
        assert!(
            source.function(entry).contains(call),
            "④ {entry} 必须刷新状态区条目：{}",
            source.function(entry)
        );
    }
    let refresh_all = source.function("void refreshStatusAreas(");
    assert!(
        refresh_all.contains("refreshActions(inputContext);")
            && refresh_all.contains("instance_->inputContextManager().foreach("),
        "④ 没有单一输入上下文（配置页路径）时应对每个 IC 各刷一遍：{refresh_all}"
    );
    let refresh_one = source.function("void refreshActions(");
    assert!(
        refresh_one.contains("for (fcitx::Action *action : statusActions_) {")
            && refresh_one.contains("action->update(inputContext);"),
        "④ 刷新必须逐个通知状态区条目重取勾选态：{refresh_one}"
    );
}

/// ⑤)：条目平铺注册（顺序 = 显示顺序）；返回 `setupStatusMenu()` 供动态文案小节复用。
fn tray_flattened_registration(source: &Source) -> String {
    let flat = source.flat();
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
    let update = source.function("void updateStatusArea(");
    assert!(
        update.contains("for (fcitx::Action *action : statusActions_) {")
            && update.contains("statusArea.addAction(fcitx::StatusGroup::InputMethod, action);"),
        "⑤ 每个条目都要作为状态区同级条目挂上（顺序 = `statusActions_`）：{update}"
    );
    let register = source.function("void registerStatusAction(");
    assert!(
        register.contains("instance_->userInterfaceManager().registerAction(name, &action);")
            && register.contains("statusActions_.push_back(&action);"),
        "⑤ 注册进 UserInterfaceManager 的同时记入显示顺序：{register}"
    );
    // 注册顺序 = 显示顺序：虎虚（图标锚点 + 模型状态）→ 四个布尔开关 → 宿主开关 → 两个子菜单 → 重新部署。
    let setup = source.function("void setupStatusMenu()");
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
    setup
}

/// ⑥)：动态文案（模型信息并入首项、子菜单父项显示「名称：当前值」）。
fn tray_dynamic_labels(source: &Source, setup: &str) {
    // ⑥) 动态文案：模型信息并入首项「虎虚」，两个子菜单父项显示「名称：当前值」。
    assert!(
        setup.contains("updateDynamicLabels();"),
        "⑤ 注册完成后要先算一次动态文案：{setup}"
    );
    assert!(
        !setup.contains("hux-model") && !setup.contains("modelAction_"),
        "⑥ 独立「模型」条目已并入首项，不应再有：{setup}"
    );
    let dynamic_labels = source.function("void updateDynamicLabels()");
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
    let refresh_actions = source.function("void refreshActions(fcitx::InputContext *inputContext)");
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
    let tri_role = source.function("static constexpr bool isTriStateRole(");
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
