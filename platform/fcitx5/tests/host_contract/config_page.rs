// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! C++ 壳的配置生命周期（源码级守卫）：`setConfig` 落盘、`reloadConfig` 重读文件。

use hux_test_support::repo_path;

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
