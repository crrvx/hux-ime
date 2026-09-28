// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! hux-ime（虎虚）fcitx5 addon 的 Rust 侧：C ABI、数据加载与会话装配。
//!
//! 分工：`shell/hux.cpp` 只做 fcitx5 接口适配（按键 → 本层；提交/preedit/候选 ← 本层回调），
//! 逻辑在 Rust（本层 → `hux-core`）。组合重建照 2c 重放桩同构规则：
//! 提交（翻译失效）或输入变化时重建，否则保留段状态（含菜单高亮）。
//!
//! 会话：每个输入上下文一个（`InputContextProperty`，组合/候选互相隔离）；
//! `deactivate/reset` 清空（参照行为）；数据目录见 [`paths::data_dirs`]。

mod abi;
mod engine;
mod learning_store;
mod paths;
mod session;
mod ui;

/// 学习库时间戳（平台层读系统时钟；内核不读时钟）。
pub(crate) use engine::wall_clock;

#[cfg(test)]
mod tests;
