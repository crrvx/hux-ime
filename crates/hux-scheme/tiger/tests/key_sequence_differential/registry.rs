// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! 登记表自校验：登记名必须真实存在且唯一，实际跳过的用例集合必须恰好等于登记集合。

use crate::deviation::{DEVIATIONS, Deviation};
use crate::golden::Case;

/// 登记项在本金样内的用例（不存在即 panic：登记名写错会静默失去守护）。
pub fn registered_case<'a>(cases: &'a [Case], deviation: &Deviation, golden: &str) -> &'a Case {
    assert_eq!(deviation.golden, golden, "登记项属于其它金样");
    let matches: Vec<&Case> = cases
        .iter()
        .filter(|case| case.name == deviation.case)
        .collect();
    assert_eq!(
        matches.len(),
        1,
        "{golden}: 登记用例 `{}` 必须唯一存在",
        deviation.case
    );
    matches[0]
}

/// 剔除登记在册的用例，返回（待重放, 登记项）。
///
/// 自校验：本金样中**实际跳过**的用例集合必须恰好等于登记表中属于本金样者 ——
/// 任何未登记却被跳过的用例都会让断言失败（不得静默跳过其它用例）。
pub fn split_cases<'a>(
    cases: &'a [Case],
    golden: &str,
) -> (Vec<&'a Case>, Vec<(&'a Deviation, &'a Case)>) {
    let mut kept = Vec::new();
    let mut deviated: Vec<(&Deviation, &Case)> = Vec::new();
    let mut skipped: Vec<&str> = Vec::new();
    for case in cases {
        match DEVIATIONS
            .iter()
            .find(|deviation| deviation.golden == golden && deviation.case == case.name)
        {
            Some(deviation) => {
                skipped.push(case.name.as_str());
                deviated.push((deviation, case));
            }
            None => kept.push(case),
        }
    }
    let mut expected: Vec<&str> = DEVIATIONS
        .iter()
        .filter(|deviation| deviation.golden == golden)
        .filter(|deviation| cases.iter().any(|case| case.name == deviation.case))
        .map(|deviation| deviation.case)
        .collect();
    expected.sort_unstable();
    skipped.sort_unstable();
    assert_eq!(
        skipped, expected,
        "{golden}: 实际跳过的用例与 DEVIATIONS 在本金样内的登记不一致\
         （不得跳过未登记的用例，也不得漏跳已登记的用例）"
    );
    for (deviation, case) in &deviated {
        assert_eq!(
            deviation.steps.len(),
            case.steps.len(),
            "{golden}: 登记用例 `{}` 的期望值步数（{}）与金样（{}）不一致",
            deviation.case,
            deviation.steps.len(),
            case.steps.len()
        );
    }
    (kept, deviated)
}
