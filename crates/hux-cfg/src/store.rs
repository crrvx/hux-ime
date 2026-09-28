// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! 选项持久化（参照 `M.options`）：主文件 `tiger_sentence.options.yaml`，
//! 缺失键回退 `<user dir>/user.yaml` 的 `var/option/<name>`（只读）；
//! 保存失败写入属性 `tiger_sentence_options_error`。
//!
//! 回退**只对已声明的缺省生效**：`sync` 按 `store_defaults` 的角色表遍历，
//! 因此 legacy 文档里出现但未纳入该表的键（例：宿主自持的 `ascii_punct`）会被读入
//! `values` 却**不会**作用到会话上——宿主标准项由宿主自身维护，方案/内核不接管。
//! 若将来要让某个键也走持久化，须先把角色加入 `store_defaults`（否则等同死读）。
//!
//! 结构：本模块保留模块文档、常量与 [`OptionsStore`] 定义；实现按职责分到子模块：
//! `load`（主文件读取与 YAML 文档解析助手）、`legacy`（legacy 回退）、`save`（写回）、
//! `access`（状态访问与同步）；单测放在 `tests`。

mod access;
mod legacy;
mod load;
mod save;

use crate::Options;
use std::path::PathBuf;
use yaml_rust2::Yaml;

/// 主存储文件名（用户数据目录下）。
pub const OPTIONS_FILE: &str = "tiger_sentence.options.yaml";
/// legacy 回退文件名（rime 用户配置，只读）；仅本模块的回退路径使用，不对外暴露。
const LEGACY_FILE: &str = "user.yaml";
/// 保存失败属性名（参照 `M.options`）。
pub const OPTIONS_ERROR_PROPERTY: &str = "tiger_sentence_options_error";
/// 保存失败属性的值（参照 `M.options`）：除 [`OptionsStore::observe`] 外，
/// 配置页写回路径（[`OptionsStore::set_values`]）也据此维护属性与状态诊断。
pub const OPTIONS_ERROR_MESSAGE: &str = "Unable to save tiger_sentence.options.yaml";
const OPTIONS_KEY: &str = "options";
const LEGACY_ROOT: &str = "var";
const LEGACY_OPTION: &str = "option";

/// 选项存储：完整 YAML 文档（保留未知键）+ core [`Options`] 状态。
pub struct OptionsStore {
    path: PathBuf,
    document: Yaml,
    options: Options,
}

#[cfg(test)]
mod tests;
