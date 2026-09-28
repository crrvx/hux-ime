// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! addon 配置模型（Rust 半）：外部设置（fcitx5 配置界面 / 测试）与内建缺省。
//!
//! 合并顺序照参照 schema 语义：**`tiger_sentence.options.yaml`（user 覆盖） > 本设置 > 内建缺省**；
//! 可持久化开关（提前上屏、提前上屏至预编辑、单字重码组句、全角标点、数字直选 5 项）
//! 以本设置为存储层缺省；**配置页推送时**本设置的值写回该文件（
//! [`crate::OptionsStore::set_values`]）——两侧是同一批项，故不再出现「`options.yaml` 的旧值
//! 压制配置页」；`ascii_punct` 等作会话初始选项；
//! `learning_on_tab` 门控学习 mode（`false` → 空串 = 不学习，对照参照 `prepare_learning` 的 `enabled`；
//! 线上键 = 上游方案选项 `tiger_sentence/tab_learning`），
//! `high_freq_limit` 变更即时重建词库（见方案的 `apply_config`）。
//!
//! 本层拥有设置词汇（涉及方案与宿主的字段名即角色名，见 [`crate::roles`]）；**选项键由方案声明**
//! （`hux_core::scheme::OptionDecl`），故各入口接收已解析的 [`OptionKeys`] 而不是硬编码键名。
//!
//! 结构：本模块保留模块文档、公开面（`pub use` 重导出）与角色接线；
//! 设置类型与常量在 `types`，设置模型（结构、缺省与派生）在 `model`；单测放在 `tests`。

mod model;
mod types;

pub use model::Settings;
pub use types::{
    CandidateLayout, DEFAULT_HIGH_FREQ_LIMIT, MAX_MIN_RETAINED_INPUT_LENGTH, PreeditMode,
};

use crate::roles::{
    OptionKeys, ROLE_ALLOW_DUPLICATE_SINGLE, ROLE_ASCII_PUNCT, ROLE_DIGIT_SELECT,
    ROLE_EARLY_COMMIT, ROLE_EARLY_COMMIT_TO_PREEDIT, ROLE_FILTER_NON_HAN, ROLE_FULL_CHARSET,
    ROLE_FULL_SHAPE,
};
use hux_core::collections::Map;

impl Settings {
    /// 会话初始选项（写入 context；`options.yaml` 的同名项随后覆盖）：返回值**有序**，
    /// 顺序即写入顺序。与内建缺省表 [`crate::builtin_option_defaults`]（按名查询的 `Map`）
    /// 同名易混，故各按来源命名。
    /// `keys` 由平台在装配处从方案声明解析（见 [`crate::roles::OptionKeys`]）；
    /// 方案未声明的角色不参与接线。
    pub fn session_option_defaults(&self, keys: &OptionKeys) -> Vec<(&'static str, bool)> {
        let mut defaults = Vec::new();
        // 顺序即写入顺序（保持既有顺序：三个方案开关 → 宿主标准项 → 运行时开关
        // 按 `RUNTIME_OPTION_ROLES` 的先后：数字直选 → 全字集 → 过滤非汉字）。
        for (role, value) in [
            (ROLE_EARLY_COMMIT, self.early_commit),
            (ROLE_EARLY_COMMIT_TO_PREEDIT, self.early_commit_to_preedit),
            (ROLE_ALLOW_DUPLICATE_SINGLE, self.allow_duplicate_single),
        ] {
            if let Some(key) = keys.key(role) {
                defaults.push((key, value));
            }
        }
        defaults.push((ROLE_FULL_SHAPE, self.full_shape));
        defaults.push((ROLE_ASCII_PUNCT, self.ascii_punct));
        for (role, value) in [
            (ROLE_DIGIT_SELECT, self.digit_select),
            (ROLE_FULL_CHARSET, self.full_charset),
            (ROLE_FILTER_NON_HAN, self.filter_non_han),
        ] {
            if let Some(key) = keys.key(role) {
                defaults.push((key, value));
            }
        }
        defaults
    }

    /// 单项设置缺省（[`Settings::session_option_defaults`] 的查询形式）。
    pub fn session_option_default(&self, keys: &OptionKeys, name: &str) -> Option<bool> {
        self.session_option_defaults(keys)
            .into_iter()
            .find(|(key, _)| *key == name)
            .map(|(_, value)| value)
    }

    /// 存储层缺省：可持久化的核心开关（`options.yaml` 缺失键回退到这些值）。
    pub fn store_defaults(&self, keys: &OptionKeys) -> Map<String, bool> {
        let mut defaults: Map<String, bool> = Map::new();
        for (role, value) in [
            (ROLE_EARLY_COMMIT, self.early_commit),
            (ROLE_EARLY_COMMIT_TO_PREEDIT, self.early_commit_to_preedit),
            (ROLE_ALLOW_DUPLICATE_SINGLE, self.allow_duplicate_single),
            (ROLE_DIGIT_SELECT, self.digit_select),
            (ROLE_FULL_CHARSET, self.full_charset),
            (ROLE_FILTER_NON_HAN, self.filter_non_han),
        ] {
            if let Some(key) = keys.key(role) {
                defaults.insert(key.to_string(), value);
            }
        }
        // 宿主标准项的角色名即键名：不依赖方案声明。
        defaults.insert(ROLE_FULL_SHAPE.to_string(), self.full_shape);
        defaults
    }
}

#[cfg(test)]
mod tests;
