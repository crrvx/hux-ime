// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! 虎句方案的资源清单与角色声明：数据资产清单、运行时选项声明，以及本方案读取的
//! 引擎设置 / 运行时角色名（与 `hux_cfg::roles` 同值同序）。

use hux_core::scheme::{Asset, AssetKind, OptionDecl};

use crate::interaction::{
    OPTION_ALLOW_DUPLICATE_SINGLE, OPTION_DIGIT_SELECT, OPTION_EARLY_COMMIT,
    OPTION_EARLY_COMMIT_TO_PREEDIT, OPTION_FILTER_NON_HAN, OPTION_FULL_CHARSET,
};
use crate::lexicon::{
    CODES_FILE, LEXICAL_FILE, MODEL_PATH, RANKS_FILE, SUPPLEMENT_FILE, WHITELIST_FILE,
};

/// 方案标识（与上游数据互通；学习库命名沿用）。
pub const SCHEME_ID: &str = "tiger_sentence";
/// 反查索引 / 标点表的文件名（平台记日志与打包用）；词库与语言模型文件名取自 `lexicon`。
const PINYIN_FILE: &str = "tiger_sentence.pinyin.bin.gz";
pub(super) const SYMBOLS_FILE: &str = "symbols.yaml";

/// 数据资产清单（相对数据目录）。
pub const ASSETS: &[Asset] = &[
    Asset {
        kind: AssetKind::Model,
        file: MODEL_PATH,
    },
    Asset {
        kind: AssetKind::Data,
        file: CODES_FILE,
    },
    Asset {
        kind: AssetKind::Data,
        file: RANKS_FILE,
    },
    Asset {
        kind: AssetKind::Data,
        file: WHITELIST_FILE,
    },
    Asset {
        kind: AssetKind::Data,
        file: SUPPLEMENT_FILE,
    },
    Asset {
        kind: AssetKind::Data,
        file: LEXICAL_FILE,
    },
    Asset {
        kind: AssetKind::Data,
        file: PINYIN_FILE,
    },
    Asset {
        kind: AssetKind::Data,
        file: SYMBOLS_FILE,
    },
];

/// 角色名：与 `hux-cfg` 的角色常量**同值**。
///
/// 方案不依赖 `hux-cfg`（依赖方向 `hux-scheme/* → hux-core`），故以字面量声明；
/// 一致性由平台装配处的测试守护（每个角色都必须被方案声明）。
pub(super) mod role {
    pub const EARLY_COMMIT: &str = "early_commit";
    pub const EARLY_COMMIT_TO_PREEDIT: &str = "early_commit_to_preedit";
    pub const ALLOW_DUPLICATE_SINGLE: &str = "allow_duplicate_single";
    pub const DIGIT_SELECT: &str = "digit_select";
    pub const FULL_CHARSET: &str = "full_charset";
    pub const FILTER_NON_HAN: &str = "filter_non_han";
    pub const HIGH_FREQ_LIMIT: &str = "high_freq_limit";
    pub const MIN_RETAINED_INPUT_LENGTH: &str = "min_retained_raw_length";
    pub const PAGE_SIZE: &str = "page_size";
    pub const PAGE_CYCLE: &str = "page_cycle";
    pub const PAGE_UP_KEYS: &str = "page_up_keys";
    pub const PAGE_DOWN_KEYS: &str = "page_down_keys";
    pub const REVERSE_LOOKUP_PRONUNCIATION_KEYS: &str = "sound_to_char_shape_keys";
    pub const REVERSE_LOOKUP_CHARACTER_KEYS: &str = "char_to_sound_shape_keys";
    pub const LEARNING_ON_TAB: &str = "tab_learning";
}

/// 本方案的运行时选项声明（角色 → 键；键即 `interaction::OPTION_*` 的持久化键）。
pub(super) const OPTION_DECLARATIONS: &[OptionDecl] = &[
    OptionDecl {
        role: role::EARLY_COMMIT,
        key: OPTION_EARLY_COMMIT,
    },
    OptionDecl {
        role: role::EARLY_COMMIT_TO_PREEDIT,
        key: OPTION_EARLY_COMMIT_TO_PREEDIT,
    },
    OptionDecl {
        role: role::ALLOW_DUPLICATE_SINGLE,
        key: OPTION_ALLOW_DUPLICATE_SINGLE,
    },
    OptionDecl {
        role: role::DIGIT_SELECT,
        key: OPTION_DIGIT_SELECT,
    },
    OptionDecl {
        role: role::FULL_CHARSET,
        key: OPTION_FULL_CHARSET,
    },
    OptionDecl {
        role: role::FILTER_NON_HAN,
        key: OPTION_FILTER_NON_HAN,
    },
];

/// 本方案从配置袋读取的**引擎设置角色**（顺序即 [`Config`](super::config::Config) 的字段序）。
///
/// 与 `hux_cfg::roles::SCHEME_CONFIG_ROLES` **同值同序**：方案不依赖 `hux-cfg`
/// （依赖方向 `hux-scheme/* → hux-core`），故以同名字面量声明；一致性由平台用例
/// `scheme_config_roles_match_the_scheme` 逐项比对，任一侧改名即失败。
pub const SCHEME_CONFIG_ROLES: &[&str] = &[
    role::HIGH_FREQ_LIMIT,
    role::MIN_RETAINED_INPUT_LENGTH,
    role::PAGE_SIZE,
    role::PAGE_CYCLE,
    role::PAGE_UP_KEYS,
    role::PAGE_DOWN_KEYS,
    role::REVERSE_LOOKUP_PRONUNCIATION_KEYS,
    role::REVERSE_LOOKUP_CHARACTER_KEYS,
    role::LEARNING_ON_TAB,
];

/// 配置袋中本方案还会读取的**运行时选项角色**（平台在装配处追加；键由本方案声明）。
///
/// 字集开关经配置袋下发（配置层角色 → 键在平台侧解析）：方案在 [`Config::parse`](super::config::Config::parse) 里
/// 据此重建词库，故它们虽在上下文里也有选项值，读袋仍是唯一入口。
pub(super) const RUNTIME_CONFIG_ROLES: &[&str] = &[
    role::ALLOW_DUPLICATE_SINGLE,
    role::FULL_CHARSET,
    role::FILTER_NON_HAN,
];
