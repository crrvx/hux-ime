// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! Android 落点的**数据目录根**：由共用层 `platform/fcitx5` 在根下接 addon 子目录。
//!
//! 宿主（fcitx5-android 的 `native-lib.cpp`）在启动 fcitx5 之前先设好两个变量：
//! `XDG_DATA_HOME` = 应用可写数据目录（选项 / 学习库 / 模型），`XDG_DATA_DIRS` = 插件
//! assets 的安装位置。故本落点**只认这两个变量**：没有 `$HOME` 回退，也没有 `/usr/...` 缺省
//! ——两者缺失时不是「换个地方找」，而是宿主没按契约注入。
//!
//! 与桌面同构的是**子目录**（`fcitx5/hux`）与「用户目录在前」的顺序，那部分在共用层。

use std::path::PathBuf;

/// 用户可写数据根：只认宿主注入的 `$XDG_DATA_HOME`（Android 没有 `$HOME` 约定）。
pub fn user_data_root(xdg_data_home: Option<PathBuf>) -> Option<PathBuf> {
    xdg_data_home
}

/// 系统数据根：只认宿主注入的 `$XDG_DATA_DIRS`，**无缺省**（插件数据在 APK assets 里，
/// 路径由宿主决定）。
pub fn system_data_roots(xdg_data_dirs: Option<&[PathBuf]>) -> Vec<PathBuf> {
    xdg_data_dirs.unwrap_or_default().to_vec()
}

/// 按宿主注入采集用户可写数据根：`$XDG_DATA_HOME`（没注入就没有）。
pub fn user_data_root_from_env() -> Option<PathBuf> {
    user_data_root(env_path("XDG_DATA_HOME"))
}

/// 按宿主注入采集系统数据根：`$XDG_DATA_DIRS`（冒号分隔；没注入就是空）。
pub fn system_data_roots_from_env() -> Vec<PathBuf> {
    let roots = std::env::var("XDG_DATA_DIRS")
        .ok()
        .map(|value| split_paths(&value));
    system_data_roots(roots.as_deref())
}

fn env_path(name: &str) -> Option<PathBuf> {
    std::env::var_os(name).map(PathBuf::from)
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
    fn user_data_root_is_the_injected_variable_only() {
        assert_eq!(
            user_data_root(Some(PathBuf::from("/data/fcitx5/files/data"))),
            Some(PathBuf::from("/data/fcitx5/files/data"))
        );
        assert_eq!(
            user_data_root(None),
            None,
            "没有 $XDG_DATA_HOME 就没有用户目录（不猜 $HOME）"
        );
    }

    #[test]
    fn system_data_roots_have_no_defaults() {
        assert_eq!(
            system_data_roots(Some(&[PathBuf::from("/data/app/usr/share")])),
            vec![PathBuf::from("/data/app/usr/share")]
        );
        assert!(
            system_data_roots(None).is_empty(),
            "宿主没注入 $XDG_DATA_DIRS ⇒ 没有系统目录（不猜 /usr/share）"
        );
        assert!(system_data_roots(Some(&[])).is_empty());
    }

    #[test]
    fn split_paths_ignores_empty_parts() {
        assert_eq!(
            split_paths("/data/a::/data/b:"),
            vec![PathBuf::from("/data/a"), PathBuf::from("/data/b")]
        );
        assert!(split_paths("").is_empty());
    }
}
