// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! 可达性用例：`has_complete_candidate` 的完整输入、锁尾扫描、失配锁与排除文本。

use super::*;

/// 完整性判据决定能否直接上屏：无锁时按整段输入的边覆盖判断。
#[test]
fn has_complete_candidate_detects_complete_input() {
    let lexicon = fixture_lexicon();
    // 无锁：abab 完整（ab → 交/疒），带必需前缀亦完整
    assert!(
        has_complete_candidate(&lexicon, "abab", "", None, false, true, None),
        "两段边覆盖全输入即算完整"
    );
    assert!(
        has_complete_candidate(&lexicon, "abab", "交", None, false, true, None),
        "必需前缀须与候选文本一致才算完整"
    );
}

/// 已锁部分不再重扫：扫描必须从锁末端开始，起点算错会重复消费输入。
#[test]
fn has_complete_candidate_scans_from_lock_end() {
    let lexicon = fixture_lexicon();
    // 锁 "ab"→交：扫描自锁末端开始
    let lock = DecodeLock {
        raw: "ab",
        text: "交",
        boundaries: "2,3;",
    };
    assert!(
        has_complete_candidate(&lexicon, "abab", "", None, false, true, Some(&lock)),
        "有锁时从锁末端继续扫描，空必需前缀也应完整"
    );
    assert!(
        has_complete_candidate(&lexicon, "abab", "交", None, false, true, Some(&lock)),
        "锁文本与必需前缀一致时判完整"
    );
    assert!(
        has_complete_candidate(&lexicon, "ab", "交", None, false, true, Some(&lock)),
        "锁已覆盖全输入时不再需要剩余边"
    );
}

/// 锁与输入或已确认文本不符时不得判完整，否则会上屏错误内容。
#[test]
fn has_complete_candidate_rejects_mismatched_lock() {
    let lexicon = fixture_lexicon();
    let lock = DecodeLock {
        raw: "ab",
        text: "交",
        boundaries: "2,3;",
    };
    // 锁前缀与输入不符 / 与已确认文本不符 → false
    let foreign = DecodeLock {
        raw: "cd",
        text: "交",
        boundaries: "2,3;",
    };
    assert!(
        !has_complete_candidate(&lexicon, "abab", "", None, false, true, Some(&foreign)),
        "锁前缀与输入不符必须判不完整"
    );
    assert!(
        !has_complete_candidate(&lexicon, "abab", "疒", None, false, true, Some(&lock)),
        "已确认文本与锁文本不符必须判不完整"
    );
}

/// 排除文本用于避免重复上屏同一内容：只压制完全相同者，更长的完成仍算数。
#[test]
fn has_complete_candidate_honors_excluded_text() {
    let lexicon = fixture_lexicon();
    let lock = DecodeLock {
        raw: "ab",
        text: "交",
        boundaries: "2,3;",
    };
    // excluded 与锁文本一致：「交」不算新完成，「交交」可以
    assert!(
        !has_complete_candidate(&lexicon, "ab", "交", Some("交"), false, true, Some(&lock)),
        "排除文本与锁文本相同：不算新完成"
    );
    assert!(
        has_complete_candidate(&lexicon, "abab", "交", Some("交"), false, true, Some(&lock)),
        "排除文本只压制完全相同者，更长的完成仍算数"
    );
}
