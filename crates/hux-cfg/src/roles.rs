// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! 角色词汇与「角色 → 选项键」解析：**角色归配置层，键归方案**。
//!
//! 内核契约（`hux_core::scheme`）只留通用容器——[`OptionDecl`]（角色 → 键）与
//! [`SchemeConfig`]（角色 → 值）；本层拥有角色的**名字与默认值**，
//! 方案自报「角色 → 键」。换方案时内核与配置层结构不动（`docs/refactor.md` §5）。
//!
//! 方向约束：方案**不依赖**本 crate（`hux-scheme/* → hux-core`），故方案侧以同名字面量声明角色，
//! 一致性由平台装配处的测试守护（清单逐项比对 + 装配路径诊断），见平台用例
//! `scheme_config_roles_match_the_scheme`。
//!
//! **命名口径**：常量名（与设置字段名 / ABI 成员名）描述**引擎概念**，字符串值是**线上键**，
//! 与上游方案 schema / rime 选项同名（改值即破坏与上游的互通与老用户配置）——
//! 例如 `ROLE_MIN_RETAINED_INPUT_LENGTH = "min_retained_raw_length"`、
//! `ROLE_REVERSE_LOOKUP_PRONUNCIATION_KEYS = "sound_to_char_shape_keys"`。
//!
//! [`SchemeConfig`]: hux_core::scheme::SchemeConfig

use hashbrown::HashMap;
use hux_core::scheme::OptionDecl;
use std::fmt;

/// 提前上屏总开关（运行时选项；键由方案声明）。
pub const ROLE_EARLY_COMMIT: &str = "early_commit";
/// 提前上屏至预编辑（运行时选项；键由方案声明）。
pub const ROLE_EARLY_COMMIT_TO_PREEDIT: &str = "early_commit_to_preedit";
/// 单字重码参与组句（运行时选项；键由方案声明）。
pub const ROLE_ALLOW_DUPLICATE_SINGLE: &str = "allow_duplicate_single";
/// 数字直选（运行时选项；键由方案声明）。
pub const ROLE_DIGIT_SELECT: &str = "digit_select";
/// 全角标点（**宿主标准选项**：键即 rime 标准名，不由方案声明）。
pub const ROLE_FULL_SHAPE: &str = "full_shape";
/// ASCII 标点（**宿主标准选项**：键即 rime 标准名，不由方案声明）。
pub const ROLE_ASCII_PUNCT: &str = "ascii_punct";

/// 高频字过滤上限（方案配置袋角色）。
pub const ROLE_HIGH_FREQ_LIMIT: &str = "high_freq_limit";
/// 最短保留**输入**长度（方案配置袋角色）：提前上屏 / 空码上屏时至少留在组合里的输入字符数。
pub const ROLE_MIN_RETAINED_INPUT_LENGTH: &str = "min_retained_raw_length";
/// 每页候选个数（方案配置袋角色）。
pub const ROLE_PAGE_SIZE: &str = "page_size";
/// 翻页循环（方案配置袋角色）。
pub const ROLE_PAGE_CYCLE: &str = "page_cycle";
/// 上翻页键（方案配置袋角色）。
pub const ROLE_PAGE_UP_KEYS: &str = "page_up_keys";
/// 下翻页键（方案配置袋角色）。
pub const ROLE_PAGE_DOWN_KEYS: &str = "page_down_keys";
/// 反查（按**读音**入口）触发键（方案配置袋角色）：输入读音即列出对应字词。
pub const ROLE_REVERSE_LOOKUP_PRONUNCIATION_KEYS: &str = "sound_to_char_shape_keys";
/// 反查（按**字符**入口）触发键（方案配置袋角色）：取光标处字符列出其读音与编码。
pub const ROLE_REVERSE_LOOKUP_CHARACTER_KEYS: &str = "char_to_sound_shape_keys";
/// Tab 确认即写入学习库的开关（方案配置袋角色）。
pub const ROLE_LEARNING_ON_TAB: &str = "tab_learning";

/// 方案必须声明的运行时选项角色（缺任一即装配失败/报错）。
pub const SCHEME_OPTION_ROLES: &[&str] = &[
    ROLE_EARLY_COMMIT,
    ROLE_EARLY_COMMIT_TO_PREEDIT,
    ROLE_ALLOW_DUPLICATE_SINGLE,
    ROLE_DIGIT_SELECT,
];

/// 宿主 / 内核标准选项角色：**键 = 角色名**（rime 标准名，不由方案声明）。
pub const HOST_OPTION_ROLES: &[&str] = &[ROLE_FULL_SHAPE, ROLE_ASCII_PUNCT];

/// 运行时开关的角色序（= 状态菜单项顺序 = C ABI `HUX_OPTION_*` 的角色序）。
pub const RUNTIME_OPTION_ROLES: &[&str] = &[
    ROLE_EARLY_COMMIT,
    ROLE_EARLY_COMMIT_TO_PREEDIT,
    ROLE_ALLOW_DUPLICATE_SINGLE,
    ROLE_FULL_SHAPE,
    ROLE_DIGIT_SELECT,
];

/// 引擎设置映射到方案配置袋的角色全集（装配完整性由此守护：缺一即测试失败）。
pub const SCHEME_CONFIG_ROLES: &[&str] = &[
    ROLE_HIGH_FREQ_LIMIT,
    ROLE_MIN_RETAINED_INPUT_LENGTH,
    ROLE_PAGE_SIZE,
    ROLE_PAGE_CYCLE,
    ROLE_PAGE_UP_KEYS,
    ROLE_PAGE_DOWN_KEYS,
    ROLE_REVERSE_LOOKUP_PRONUNCIATION_KEYS,
    ROLE_REVERSE_LOOKUP_CHARACTER_KEYS,
    ROLE_LEARNING_ON_TAB,
];

/// 声明解析失败：方案未声明必需角色 / 同角色重复声明 / 声明有空项。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DeclError {
    /// 方案未声明的必需角色（清单）。
    Missing(Vec<&'static str>),
    /// 同角色重复声明（含与宿主标准选项冲突）。
    Duplicate(&'static str),
    /// 角色名或键为空。
    Empty,
}

impl fmt::Display for DeclError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Missing(roles) => write!(f, "方案未声明角色：{}", roles.join(", ")),
            Self::Duplicate(role) => write!(f, "角色重复声明：{role}"),
            Self::Empty => write!(f, "选项声明含空角色名或空键"),
        }
    }
}

/// 角色的**已解析**选项键表：宿主标准键 + 方案声明。
///
/// 生产路径由平台在装配处用方案的 [`Scheme::option_declarations`] 解析；
/// 解析失败（[`DeclError`]）时平台**报错而不静默降级**：那些角色不参与选项接线。
///
/// [`Scheme::option_declarations`]: hux_core::scheme::Scheme::option_declarations
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct OptionKeys {
    keys: HashMap<&'static str, &'static str>,
}

impl OptionKeys {
    /// 由方案的声明解析：先铺宿主标准键，再逐个收方案声明；
    /// **缺任一 [`SCHEME_OPTION_ROLES`] 即 `Err`**（换方案漏声明会在装配处暴露，不会静默失效）。
    pub fn resolve(declarations: &[OptionDecl]) -> Result<Self, DeclError> {
        let mut keys = HashMap::new();
        for role in HOST_OPTION_ROLES {
            keys.insert(*role, *role);
        }
        for decl in declarations {
            if decl.role.is_empty() || decl.key.is_empty() {
                return Err(DeclError::Empty);
            }
            if keys.insert(decl.role, decl.key).is_some() {
                return Err(DeclError::Duplicate(decl.role));
            }
        }
        let missing: Vec<&'static str> = SCHEME_OPTION_ROLES
            .iter()
            .copied()
            .filter(|role| !keys.contains_key(role))
            .collect();
        if missing.is_empty() {
            Ok(Self { keys })
        } else {
            Err(DeclError::Missing(missing))
        }
    }

    /// 角色对应的选项键（未声明 / 未知角色为 `None`）。
    pub fn key(&self, role: &str) -> Option<&'static str> {
        self.keys.get(role).copied()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn declarations() -> Vec<OptionDecl> {
        vec![
            OptionDecl {
                role: ROLE_EARLY_COMMIT,
                key: "scheme_early_commit",
            },
            OptionDecl {
                role: ROLE_EARLY_COMMIT_TO_PREEDIT,
                key: "scheme_early_commit_to_preedit",
            },
            OptionDecl {
                role: ROLE_ALLOW_DUPLICATE_SINGLE,
                key: "scheme_allow_duplicate_single",
            },
            OptionDecl {
                role: ROLE_DIGIT_SELECT,
                key: "scheme_digit_select",
            },
        ]
    }

    /// 四张清单之间的分组 / 顺序不变式（`docs/refactor.md` §5）。
    ///
    /// 这些关系此前只靠人工维护：新增一个方案开关却忘了入 `RUNTIME_OPTION_ROLES`
    /// 不会让任何断言失败（审计 F12）。
    #[test]
    fn role_tables_stay_disjoint_and_ordered() {
        // 每张清单内部无重复。
        for table in [
            SCHEME_OPTION_ROLES,
            HOST_OPTION_ROLES,
            RUNTIME_OPTION_ROLES,
            SCHEME_CONFIG_ROLES,
        ] {
            let mut seen = Vec::new();
            for role in table {
                assert!(!seen.contains(role), "角色 {role} 在同一清单中重复");
                seen.push(*role);
            }
        }
        // 宿主标准项由配置层自持，方案不得声明（`OptionKeys::resolve` 会判冲突）。
        for role in HOST_OPTION_ROLES {
            assert!(
                !SCHEME_OPTION_ROLES.contains(role),
                "宿主标准项 {role} 不应出现在方案声明角色里"
            );
        }
        // 运行时开关序（= 状态菜单序 = ABI 角色序）保持方案开关的相对先后。
        let positions: Vec<usize> = SCHEME_OPTION_ROLES
            .iter()
            .map(|role| {
                RUNTIME_OPTION_ROLES
                    .iter()
                    .position(|item| item == role)
                    .unwrap_or_else(|| panic!("运行时角色缺少方案开关 {role}"))
            })
            .collect();
        assert!(
            positions.windows(2).all(|pair| pair[0] < pair[1]),
            "方案开关在运行时角色序中被重排：{SCHEME_OPTION_ROLES:?}"
        );
    }

    #[test]
    fn resolve_needs_every_scheme_role() {
        // 正例：4 个角色齐备，宿主标准键就位。
        let keys = OptionKeys::resolve(&declarations()).expect("完整声明");
        assert_eq!(keys.key(ROLE_EARLY_COMMIT), Some("scheme_early_commit"));
        assert_eq!(keys.key(ROLE_FULL_SHAPE), Some("full_shape"));
        assert_eq!(keys.key(ROLE_ASCII_PUNCT), Some("ascii_punct"));
        assert_eq!(keys.key("not_a_role"), None);

        // 负例：缺一个角色即报错，并点名缺失角色（换方案漏声明不得静默通过）。
        let mut incomplete = declarations();
        incomplete.retain(|decl| decl.role != ROLE_DIGIT_SELECT);
        assert_eq!(
            OptionKeys::resolve(&incomplete),
            Err(DeclError::Missing(vec![ROLE_DIGIT_SELECT]))
        );
        assert_eq!(
            OptionKeys::resolve(&[]),
            Err(DeclError::Missing(SCHEME_OPTION_ROLES.to_vec()))
        );
    }

    #[test]
    fn resolve_rejects_duplicate_and_empty_declarations() {
        // 同角色两条声明：后者不得静默覆盖前者。
        let mut duplicated = declarations();
        duplicated.push(OptionDecl {
            role: ROLE_EARLY_COMMIT,
            key: "scheme_early_commit_again",
        });
        assert_eq!(
            OptionKeys::resolve(&duplicated),
            Err(DeclError::Duplicate(ROLE_EARLY_COMMIT))
        );

        // 宿主标准选项由配置层自持：方案不得改名（改名即冲突）。
        let mut hijacked = declarations();
        hijacked.push(OptionDecl {
            role: ROLE_FULL_SHAPE,
            key: "scheme_full_shape",
        });
        assert_eq!(
            OptionKeys::resolve(&hijacked),
            Err(DeclError::Duplicate(ROLE_FULL_SHAPE))
        );

        // 空角色名 / 空键：装配缺陷，直接报错。
        let mut empty = declarations();
        empty.push(OptionDecl { role: "", key: "x" });
        assert_eq!(OptionKeys::resolve(&empty), Err(DeclError::Empty));

        // 诊断文案（进平台状态串）。
        assert_eq!(
            DeclError::Missing(vec![ROLE_DIGIT_SELECT]).to_string(),
            "方案未声明角色：digit_select"
        );
        assert_eq!(
            DeclError::Duplicate(ROLE_DIGIT_SELECT).to_string(),
            "角色重复声明：digit_select"
        );
    }
}
