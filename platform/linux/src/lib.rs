// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! Linux 落点的**数据目录根**（XDG 规则）：由共用层 `platform/fcitx5` 在根下接 addon 子目录。
//!
//! 本 crate 是桌面端的平台事实：用哪些根目录、读哪些环境变量、缺省是什么。规则函数收参数、
//! 不碰环境（便于纯测试）；[`user_data_root_from_env`] / [`system_data_roots_from_env`] 是给
//! 共用层的采集包装——共用层按目标平台选一份落点 crate，两者同名同型（见 `platform/fcitx5`
//! 的 `Cargo.toml` 与 `src/paths.rs`）。

use std::path::PathBuf;

/// 用户可写数据根（选项 / 学习库 / 模型）：`$XDG_DATA_HOME`，缺省 `$HOME/.local/share`。
pub fn user_data_root(xdg_data_home: Option<PathBuf>, home: Option<PathBuf>) -> Option<PathBuf> {
    xdg_data_home.or_else(|| home.map(|home| home.join(".local/share")))
}

/// 系统数据根（按 `$XDG_DATA_DIRS` 顺序）。
///
/// `None` = 变量未设置 ⇒ 用 XDG 缺省；`Some(&[])` = 设置但为空 ⇒ 空列表（按 XDG 规则即为空，
/// **不**回退缺省）。
pub fn system_data_roots(xdg_data_dirs: Option<&[PathBuf]>) -> Vec<PathBuf> {
    match xdg_data_dirs {
        Some(roots) => roots.to_vec(),
        None => vec![
            PathBuf::from("/usr/local/share"),
            PathBuf::from("/usr/share"),
        ],
    }
}

/// 按桌面环境采集用户可写数据根：`$XDG_DATA_HOME` → `$HOME/.local/share`。
pub fn user_data_root_from_env() -> Option<PathBuf> {
    user_data_root(env_path("XDG_DATA_HOME"), env_path("HOME"))
}

/// 按桌面环境采集系统数据根：`$XDG_DATA_DIRS`（冒号分隔），未设置时用 XDG 缺省。
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
    fn user_data_root_prefers_xdg_data_home_then_home() {
        assert_eq!(
            user_data_root(Some(PathBuf::from("/xdg")), Some(PathBuf::from("/home/u"))),
            Some(PathBuf::from("/xdg"))
        );
        assert_eq!(
            user_data_root(None, Some(PathBuf::from("/home/u"))),
            Some(PathBuf::from("/home/u/.local/share"))
        );
        assert_eq!(user_data_root(None, None), None);
    }

    #[test]
    fn system_data_roots_default_only_when_unset() {
        assert_eq!(
            system_data_roots(None),
            vec![
                PathBuf::from("/usr/local/share"),
                PathBuf::from("/usr/share")
            ]
        );
        assert_eq!(
            system_data_roots(Some(&[PathBuf::from("/s1"), PathBuf::from("/s2")])),
            vec![PathBuf::from("/s1"), PathBuf::from("/s2")]
        );
        assert!(
            system_data_roots(Some(&[])).is_empty(),
            "设置了但为空 ⇒ 空列表，不回退缺省"
        );
    }

    #[test]
    fn split_paths_ignores_empty_parts() {
        assert_eq!(
            split_paths("/a::/b:"),
            vec![PathBuf::from("/a"), PathBuf::from("/b")]
        );
        assert!(split_paths("").is_empty());
    }
}
