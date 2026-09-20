// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! 平台数据目录与模型定位（fcitx5 桌面 / Android 共用）。
//!
//! 解析顺序：`HUX_DATA_DIRS`（冒号分隔，开发覆盖）→ 用户数据目录
//! （`$XDG_DATA_HOME` 或 `$HOME/.local/share`）→ `$XDG_DATA_DIRS`
//! （默认 `/usr/local/share:/usr/share`），各目录下接 `fcitx5/hux`。
//! 用户可写数据（选项 / 学习库 / 模型）用 [`user_data_dir`]；内核不读环境变量。

use std::path::PathBuf;

use hux_core::lexicon::{MODEL_PATH, candidate_paths};

/// 只读数据目录（按优先级）；开发可用 `HUX_DATA_DIRS` 覆盖。
pub(crate) fn data_dirs() -> Vec<PathBuf> {
    let override_dirs = std::env::var("HUX_DATA_DIRS")
        .ok()
        .map(|value| split_paths(&value));
    if let Some(dirs) = override_dirs.filter(|dirs| !dirs.is_empty()) {
        return dirs;
    }
    data_dirs_from(
        std::env::var_os("XDG_DATA_HOME").map(PathBuf::from),
        std::env::var_os("HOME").map(PathBuf::from),
        std::env::var_os("XDG_DATA_DIRS").map(PathBuf::from),
    )
}

/// 用户可写数据目录（选项 / 学习库 / 模型）；无用户目录时返回 `None`。
pub(crate) fn user_data_dir() -> Option<PathBuf> {
    user_data_dir_from(
        std::env::var_os("XDG_DATA_HOME").map(PathBuf::from),
        std::env::var_os("HOME").map(PathBuf::from),
    )
}

/// 在各数据目录中查找模型文件（`HUX_MODEL` 覆盖在调用处处理）。
pub(crate) fn default_model_path(dirs: &[PathBuf]) -> Option<PathBuf> {
    candidate_paths(dirs, MODEL_PATH)
        .into_iter()
        .find(|path| path.is_file())
}

// ---------------------------------------------------------------- 纯函数（可测）

fn split_paths(value: &str) -> Vec<PathBuf> {
    value
        .split(':')
        .filter(|part| !part.is_empty())
        .map(PathBuf::from)
        .collect()
}

fn user_data_home(xdg_data_home: Option<PathBuf>, home: Option<PathBuf>) -> Option<PathBuf> {
    xdg_data_home.or_else(|| home.map(|home| home.join(".local/share")))
}

fn user_data_dir_from(xdg_data_home: Option<PathBuf>, home: Option<PathBuf>) -> Option<PathBuf> {
    user_data_home(xdg_data_home, home).map(|dir| dir.join("fcitx5/hux"))
}

fn data_dirs_from(
    xdg_data_home: Option<PathBuf>,
    home: Option<PathBuf>,
    xdg_data_dirs: Option<PathBuf>,
) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(user) = user_data_dir_from(xdg_data_home, home) {
        dirs.push(user);
    }
    let shared = match xdg_data_dirs {
        Some(value) => split_paths(&value.to_string_lossy()),
        None => vec![
            PathBuf::from("/usr/local/share"),
            PathBuf::from("/usr/share"),
        ],
    };
    for dir in shared {
        let path = dir.join("fcitx5/hux");
        if !dirs.contains(&path) {
            dirs.push(path);
        }
    }
    dirs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn data_dirs_follow_xdg_rules() {
        let dirs = data_dirs_from(
            Some(PathBuf::from("/xdg")),
            Some(PathBuf::from("/home/u")),
            Some(PathBuf::from("/s1:/s2")),
        );
        assert_eq!(
            dirs,
            vec![
                PathBuf::from("/xdg/fcitx5/hux"),
                PathBuf::from("/s1/fcitx5/hux"),
                PathBuf::from("/s2/fcitx5/hux"),
            ]
        );
        let dirs = data_dirs_from(None, Some(PathBuf::from("/home/u")), None);
        assert_eq!(dirs[0], PathBuf::from("/home/u/.local/share/fcitx5/hux"));
        assert_eq!(dirs[1], PathBuf::from("/usr/local/share/fcitx5/hux"));
        assert_eq!(dirs[2], PathBuf::from("/usr/share/fcitx5/hux"));
    }

    #[test]
    fn user_data_dir_prefers_xdg_data_home() {
        assert_eq!(
            user_data_dir_from(Some(PathBuf::from("/xdg")), None),
            Some(PathBuf::from("/xdg/fcitx5/hux"))
        );
        assert_eq!(
            user_data_dir_from(None, Some(PathBuf::from("/home/u"))),
            Some(PathBuf::from("/home/u/.local/share/fcitx5/hux"))
        );
        assert_eq!(user_data_dir_from(None, None), None);
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
