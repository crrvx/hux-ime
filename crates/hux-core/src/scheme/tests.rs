// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! 方案配置袋单测：角色键袋读写、类型检查与 `require_*` 诊断。

use super::*;

#[test]
fn scheme_config_is_a_role_keyed_bag() {
    let config = SchemeConfig::new()
        .with("switch", Value::Bool(true))
        .with("count", Value::Count(5));
    assert_eq!(config.roles().collect::<Vec<_>>(), vec!["switch", "count"]);
    assert_eq!(config.bool("switch"), Some(true));
    // 同角色后写覆盖先写（装配处不改结构即可改值），不新增条目。
    let replaced = config.clone().with("switch", Value::Bool(false));
    assert_eq!(replaced.bool("switch"), Some(false));
    assert_eq!(replaced.roles().count(), 2);
    let mut mutated = config.clone();
    mutated.set("count", Value::Count(9));
    assert_eq!(mutated.count("count"), Some(9));
    assert_eq!(mutated.roles().count(), 2);
}

#[test]
fn scheme_config_accessors_are_type_checked() {
    let config = SchemeConfig::new()
        .with("switch", Value::Bool(true))
        .with("count", Value::Count(3))
        .with("text", Value::Text("abc".to_string()))
        .with("texts", Value::Texts(vec!["k".to_string()]));
    assert_eq!(
        config.count("switch"),
        None,
        "类型不符即 None，不做隐式转换"
    );
    assert_eq!(config.bool("count"), None);
    assert_eq!(config.text("texts"), None);
    assert_eq!(config.text("text"), Some("abc"));
    assert_eq!(config.texts("texts"), Some(["k".to_string()].as_slice()));
    // 未装配与未知角色一律 `None`（回退语义由方案决定）。
    assert_eq!(config.text("missing"), None);
    assert_eq!(config.count("missing"), None);
    assert_eq!(config.texts("missing"), None);
    assert!(config.get("missing").is_none());
    // 「类型不符」与「空列表」必须可区分（`texts` 不再退化成空切片）。
    assert_eq!(config.texts("switch"), None);
    let empty = SchemeConfig::new().with("empty", Value::Texts(Vec::new()));
    assert_eq!(empty.texts("empty"), Some([].as_slice()));
    // 袋的 `Default` 是**空袋**（不是「全零字段」）：缺角色的回退由方案解析决定。
    assert!(SchemeConfig::default().roles().next().is_none());
}

/// 诊断式读取：`require_*` 把「缺角色」与「类型不符」区分开。
#[test]
fn scheme_config_require_reports_missing_and_mismatch() {
    let config = SchemeConfig::new()
        .with("switch", Value::Bool(true))
        .with("count", Value::Count(3))
        .with("text", Value::Text("abc".to_string()))
        .with("texts", Value::Texts(vec!["k".to_string()]));
    assert_eq!(config.require_bool("switch"), Ok(true));
    assert_eq!(config.require_count("count"), Ok(3));
    assert_eq!(config.require_text("text"), Ok("abc"));
    assert_eq!(
        config.require_texts("texts"),
        Ok(["k".to_string()].as_slice())
    );
    // 缺角色 vs 类型不符：变体不同，文案不同，角色可取出。
    let missing = config.require_bool("absent").expect_err("缺角色");
    assert_eq!(missing, ConfigError::Missing { role: "absent" });
    assert_eq!(missing.role(), "absent");
    assert_eq!(missing.to_string(), "缺少角色 absent");
    let mismatch = config.require_bool("count").expect_err("类型不符");
    assert_eq!(
        mismatch,
        ConfigError::TypeMismatch {
            role: "count",
            expected: "开关",
            found: "计数",
        }
    );
    assert_eq!(mismatch.role(), "count");
    assert_eq!(
        mismatch.to_string(),
        "角色 count 类型不符（期望 开关，实际 计数）"
    );
    // 类型名的中文口径覆盖四类取值。
    for (value, kind) in [
        (Value::Bool(true), "开关"),
        (Value::Count(1), "计数"),
        (Value::Text("x".to_string()), "文本"),
        (Value::Texts(vec!["x".to_string()]), "文本列表"),
    ] {
        assert_eq!(value.kind(), kind);
    }
}
