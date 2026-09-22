// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! 平台数据目录与模型定位（fcitx5 桌面 / Android 共用）。
//!
//! 解析顺序：`HUX_DATA_DIRS`（冒号分隔，开发覆盖）→ 用户数据目录
//! （`$XDG_DATA_HOME` 或 `$HOME/.local/share`）→ `$XDG_DATA_DIRS`
//! （默认 `/usr/local/share:/usr/share`），各目录下接 `fcitx5/hux`。
//! 用户可写数据（选项 / 学习库 / 模型）用 [`user_data_dir`]；内核不读环境变量。

use std::path::PathBuf;

use hux_core::scheme::{Asset, AssetKind};

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

/// 在各数据目录中查找**方案声明的模型候选**（`HUX_MODEL` 覆盖在调用处处理）。
///
/// **目录优先**：先在用户目录里按声明顺序试候选，全部落空才看下一个目录——用户自备的模型
/// 不会被系统目录里的另一种格式顶掉。目录内先五阶（`sentence-fivegram-mobile.bin`）再三阶；
/// **裸文件名（不带 `models/`）只在第一个目录尝试**（与上游「裸名只认用户目录」一致）。
pub(crate) fn default_model_path(dirs: &[PathBuf], assets: &[Asset]) -> Option<PathBuf> {
    let models: Vec<&Asset> = assets
        .iter()
        .filter(|asset| asset.kind == AssetKind::Model)
        .collect();
    for (index, dir) in dirs.iter().enumerate() {
        for asset in &models {
            if index > 0 && !asset.file.contains('/') {
                continue;
            }
            let candidate = dir.join(asset.file);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
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
    fn model_lookup_prefers_user_directory_then_fivegram() {
        let root = hux_test_support::temp_dir("model-lookup");
        let user = root.join("user");
        let shared = root.join("shared");
        for dir in [&user, &shared] {
            std::fs::create_dir_all(dir.join("models")).expect("create models dir");
        }
        let assets = [
            Asset {
                kind: AssetKind::Model,
                file: "models/sentence-fivegram-mobile.bin",
            },
            Asset {
                kind: AssetKind::Model,
                file: "sentence-fivegram-mobile.bin",
            },
            Asset {
                kind: AssetKind::Model,
                file: "models/sentence-ngram-mobile.bin",
            },
            Asset {
                kind: AssetKind::Model,
                file: "sentence-ngram-mobile.bin",
            },
        ];
        let dirs = [user.clone(), shared.clone()];
        // 只有共享目录有五阶 ⇒ 用它
        std::fs::write(shared.join("models/sentence-fivegram-mobile.bin"), b"x").expect("write");
        assert_eq!(
            default_model_path(&dirs, &assets),
            Some(shared.join("models/sentence-fivegram-mobile.bin"))
        );
        // 用户目录的三阶优先于共享目录的五阶（用户自备者不被顶掉）
        std::fs::write(user.join("models/sentence-ngram-mobile.bin"), b"x").expect("write");
        assert_eq!(
            default_model_path(&dirs, &assets),
            Some(user.join("models/sentence-ngram-mobile.bin"))
        );
        // 同目录内五阶优先
        std::fs::write(user.join("models/sentence-fivegram-mobile.bin"), b"x").expect("write");
        assert_eq!(
            default_model_path(&dirs, &assets),
            Some(user.join("models/sentence-fivegram-mobile.bin"))
        );
        // 非首个目录的裸名不参与
        std::fs::write(shared.join("sentence-ngram-mobile.bin"), b"x").expect("write");
        std::fs::remove_file(user.join("models/sentence-fivegram-mobile.bin")).ok();
        std::fs::remove_file(user.join("models/sentence-ngram-mobile.bin")).ok();
        assert_eq!(
            default_model_path(&dirs, &assets),
            Some(shared.join("models/sentence-fivegram-mobile.bin"))
        );
        // 首个目录的裸名可用
        std::fs::write(user.join("sentence-fivegram-mobile.bin"), b"x").expect("write");
        assert_eq!(
            default_model_path(&dirs, &assets),
            Some(user.join("sentence-fivegram-mobile.bin"))
        );
        std::fs::remove_dir_all(&root).ok();
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
