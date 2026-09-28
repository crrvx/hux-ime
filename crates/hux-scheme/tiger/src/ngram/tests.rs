// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! `MobileModel` 装载与缓存计数的单元测试。

use super::binary::{BOS, EOS, SHIFT, pack2, scalar};
use super::*;

#[test]
fn scalar_and_pack2_match_reference_rules() {
    assert_eq!(scalar(""), 0);
    assert_eq!(scalar(BOS), 2);
    assert_eq!(scalar(EOS), 3);
    assert_eq!(scalar("甲"), 0x7532);
    assert_eq!(scalar("不存在"), 0x4e0d); // 首个码位
    assert_eq!(pack2(0x7532, 0x7532), 0x7532 * SHIFT + 0x7532);
    assert_eq!(pack2(1, 5), SHIFT + 5);
    assert_eq!(pack2(0x10FFFF, 0x10FFFF), 0x10FFFF * SHIFT + 0x10FFFF);
}

#[test]
fn rejects_unknown_models() {
    let directory = std::env::temp_dir();
    let path = directory.join(format!("hux-unknown-{}.bin", std::process::id()));
    std::fs::write(&path, b"NOTAMODELBLOB").expect("write temp model");
    assert!(MobileModel::load(&path, None).is_err());
    std::fs::remove_file(&path).ok();
}

/// `load` 与 `configure_cache` 对上限的校验口径一致——非法上限返回错误，
/// 而不是在 `Fifo::new(0)` / `Columns::new(0)` 的 `assert!` 处 panic。
#[test]
fn load_rejects_invalid_cache_limits() {
    let fixture = hux_test_support::repo_path("goldens/ngram_fixture.bin");
    for limits in [
        Limits {
            page_bytes: 0,
            ..Limits::default()
        },
        Limits {
            context_entries: 0,
            ..Limits::default()
        },
        Limits {
            bigram_entries: 0,
            ..Limits::default()
        },
        Limits {
            index_pages: 0,
            ..Limits::default()
        },
    ] {
        let Err(error) = MobileModel::load(&fixture, Some(limits)) else {
            panic!("非法上限必须报错");
        };
        assert!(
            error.to_string().contains("invalid cache limits"),
            "诊断应指出上限非法：{error}"
        );
    }
    // 合法上限照常加载。
    assert!(MobileModel::load(&fixture, Some(Limits::default())).is_ok());
}

/// 畸形头部把 `tri_ctx_count` 写成 `u64::MAX` 时显式报错，而不是在 32 位目标上
/// 静默截断成别的值（其余头部字段保持不变 ⇒ 仍能通过前面的布局/尺寸校验）。
#[test]
fn implausible_trigram_context_count_is_rejected() {
    let fixture = hux_test_support::repo_path("goldens/ngram_fixture.bin");
    let mut corrupted = std::fs::read(&fixture).expect("read fixture model");
    corrupted[72..80].copy_from_slice(&u64::MAX.to_le_bytes());
    let path = std::env::temp_dir().join(format!("hux-tricount-{}.bin", std::process::id()));
    std::fs::write(&path, &corrupted).expect("write patched model");
    let Err(error) = MobileModel::load(&path, None) else {
        panic!("巨大上下文数必须报错");
    };
    assert!(
        error
            .to_string()
            .contains("implausible trigram context count"),
        "诊断应指出上下文数不可信：{error}"
    );
    std::fs::remove_file(&path).ok();
}

#[test]
fn corrupt_index_queries_do_not_panic() {
    // 畸形模型：头部合法、trigram 索引区被填充异常值 → 查询必须返回错误而非 panic。
    let fixture = hux_test_support::repo_path("goldens/ngram_fixture.bin");
    let source = std::fs::read(&fixture).expect("read fixture model");
    let directory = std::env::temp_dir();
    for (index, fill) in [0xffu8, 0x00].into_iter().enumerate() {
        let mut corrupted = source.clone();
        let offset = le_u64(&corrupted, 96) as usize; // 头部 tri_index_off
        corrupted[offset..].fill(fill);
        let path = directory.join(format!("hux-corrupt-{}-{index}.bin", std::process::id()));
        std::fs::write(&path, &corrupted).expect("write corrupted model");
        if let Ok(mut model) = MobileModel::load(&path, None) {
            let _ = model.logp("甲", "乙", "丙");
            let _ = model.has_observed_bigram("甲", "乙");
        }
        std::fs::remove_file(&path).ok();
    }
}
