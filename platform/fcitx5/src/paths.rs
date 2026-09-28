// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! 平台数据目录与模型定位（fcitx5 桌面 / Android 共用的那一半）。
//!
//! 本层只做**共用**的三件事：给根目录接上 addon 子目录 `fcitx5/hux`、按「用户目录在前」
//! 去重排序、认 `HUX_DATA_DIRS` 开发覆盖。**根目录规则与读哪些环境变量属于落点**：
//! `platform/linux` 的 XDG 规则与 `platform/android` 的宿主注入规则各一份，两边提供同名的
//! `user_data_root_from_env` / `system_data_roots_from_env`，本层只按目标平台选 `use`。
//!
//! `HUX_DATA_DIRS`（冒号分隔）是开发覆盖：命中即整体取代顺序。内核不读环境变量。

use std::path::PathBuf;

use hux_core::scheme::{Asset, AssetKind, find_asset};

/// 落点 crate（Cargo 不允许同一个依赖名在不同 target 下指向不同路径，故这里按平台选 `use`）。
#[cfg(target_os = "android")]
use hux_platform_android as platform;
#[cfg(target_os = "linux")]
use hux_platform_linux as platform;

#[cfg(not(any(target_os = "android", target_os = "linux")))]
compile_error!(
    "hux-platform-fcitx5 目前只落 linux 与 android：新平台请加一份落点 crate（platform/<os>），\
     提供 user_data_root_from_env / system_data_roots_from_env，并在 platform/fcitx5/Cargo.toml \
     与 src/paths.rs 的 `use` 里接上"
);

/// addon 数据子目录（两端同构：`<根>/fcitx5/hux`）。
const ADDON_SUBDIR: &str = "fcitx5/hux";

/// 只读数据目录（按优先级）；开发可用 `HUX_DATA_DIRS` 覆盖。
pub(crate) fn data_dirs() -> Vec<PathBuf> {
    let override_dirs = std::env::var("HUX_DATA_DIRS")
        .ok()
        .map(|value| split_paths(&value));
    if let Some(dirs) = override_dirs.filter(|dirs| !dirs.is_empty()) {
        return dirs;
    }
    compose(user_data_dir(), system_data_dirs())
}

/// 用户可写数据目录（选项 / 学习库 / 模型）；无用户目录时返回 `None`。
pub(crate) fn user_data_dir() -> Option<PathBuf> {
    platform::user_data_root_from_env().map(|root| root.join(ADDON_SUBDIR))
}

/// 在各数据目录中查找**方案声明的模型资产**（`HUX_MODEL` 覆盖在调用处处理）。
pub(crate) fn default_model_path(dirs: &[PathBuf], assets: &[Asset]) -> Option<PathBuf> {
    assets
        .iter()
        .find(|asset| asset.kind == AssetKind::Model)
        .and_then(|asset| find_asset(dirs, asset.file))
}

/// 模型**该放的位置**（**不要求文件存在**）：首个数据目录（= 用户数据目录，见 [`data_dirs`]）
/// 下接方案声明的模型资产路径。
///
/// 与 [`default_model_path`] 的分工：后者只在文件确实存在时给出路径（供装载）；
/// 本函数供宿主的「打开模型目录」入口——没有模型时正是要告诉用户**放到哪儿**，
/// 故路径可以不存在（其父目录即目标目录）。
pub(crate) fn intended_model_path(dirs: &[PathBuf], assets: &[Asset]) -> Option<PathBuf> {
    let asset = assets.iter().find(|asset| asset.kind == AssetKind::Model)?;
    dirs.first().map(|dir| dir.join(asset.file))
}

/// 系统数据目录：落点负责采集 `$XDG_DATA_DIRS` 与各自的缺省，这里只接 addon 子目录。
fn system_data_dirs() -> Vec<PathBuf> {
    platform::system_data_roots_from_env()
        .iter()
        .map(|root| root.join(ADDON_SUBDIR))
        .collect()
}

/// 用户目录在前，系统目录按序跟随；同一目录只留一次。
fn compose(user: Option<PathBuf>, system: Vec<PathBuf>) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(user) = user {
        dirs.push(user);
    }
    for dir in system {
        if !dirs.contains(&dir) {
            dirs.push(dir);
        }
    }
    dirs
}

/// 冒号分隔的路径列表（空段丢弃）。
fn split_paths(value: &str) -> Vec<PathBuf> {
    value
        .split(':')
        .filter(|part| !part.is_empty())
        .map(PathBuf::from)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_paths_ignores_empty_parts() {
        assert_eq!(
            split_paths("/a::/b:"),
            vec![PathBuf::from("/a"), PathBuf::from("/b")]
        );
        assert!(split_paths("").is_empty());
    }

    #[test]
    fn compose_puts_user_dir_first_and_dedups() {
        assert_eq!(
            compose(
                Some(PathBuf::from("/user/fcitx5/hux")),
                vec![
                    PathBuf::from("/user/fcitx5/hux"),
                    PathBuf::from("/s1/fcitx5/hux"),
                    PathBuf::from("/s1/fcitx5/hux"),
                    PathBuf::from("/s2/fcitx5/hux"),
                ],
            ),
            vec![
                PathBuf::from("/user/fcitx5/hux"),
                PathBuf::from("/s1/fcitx5/hux"),
                PathBuf::from("/s2/fcitx5/hux"),
            ]
        );
        assert_eq!(
            compose(None, vec![PathBuf::from("/s1/fcitx5/hux")]),
            vec![PathBuf::from("/s1/fcitx5/hux")],
            "没有用户目录时系统目录照旧"
        );
    }

    /// 「该放的位置」**不要求文件存在**（对比 [`default_model_path`] 只在文件存在时给出路径）：
    /// 没有模型时它给的是用户数据目录下的方案资产路径，父目录即「模型该放的地方」。
    #[test]
    fn intended_model_path_needs_no_existing_file() {
        use hux_scheme_tiger::scheme::ASSETS;
        let dir = hux_test_support::temp_dir("intended-model-path");
        assert_eq!(
            default_model_path(std::slice::from_ref(&dir), ASSETS),
            None,
            "空目录里没有模型资产"
        );
        assert_eq!(
            intended_model_path(std::slice::from_ref(&dir), ASSETS),
            Some(dir.join("models/sentence-ngram-mobile.bin")),
            "该放的位置与文件是否存在无关"
        );
        assert_eq!(
            intended_model_path(&[], ASSETS),
            None,
            "没有数据目录 ⇒ None"
        );
        std::fs::remove_dir_all(&dir).ok();
    }
}
