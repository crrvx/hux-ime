// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! 维护工具：打印登记用例（或 `HUX_DUMP_CASES` 指定用例）的实测紧凑行。

use std::path::{Path, PathBuf};

use crate::deviation::DEVIATIONS;
use crate::golden::load_cases;
use crate::replay::replay;
use crate::views::{RowView, rows_differ};

/// 打印用例的实测行（与 [`Deviation::steps`] 同格式的 10 列，可直接粘进 `DEVIATIONS`）。
///
/// - 缺省打印 `DEVIATIONS` 里全部登记用例（复核/更新期望值时用）；
/// - 另可用 `HUX_DUMP_CASES="key_sequence.tsv.gz/case-a,sound_to_char_shape.tsv.gz/case-b"`
///   打印任意用例（**新增偏离项**时先跑它，再核对「期望 ≠ 金样」的步集合）。
/// - 与 `DEVIATIONS` 同口径重放（出厂缺省 + 金样夹具），故输出即当前实现的行为。
pub fn run() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..");
    let mut targets: Vec<(String, String)> = DEVIATIONS
        .iter()
        .map(|deviation| (deviation.golden.to_string(), deviation.case.to_string()))
        .collect();
    if let Ok(extra) = std::env::var("HUX_DUMP_CASES") {
        targets.extend(
            extra
                .split(',')
                .filter(|item| !item.is_empty())
                .map(|item| {
                    let (golden, case) = item
                        .split_once('/')
                        .expect("HUX_DUMP_CASES 形如 <金样>/<用例>");
                    (golden.to_string(), case.to_string())
                }),
        );
    }
    for (golden, case_name) in targets {
        dump_case(&root, &golden, &case_name);
    }
}

/// 打印单个用例的实测紧凑行。
fn dump_case(root: &Path, golden: &str, case_name: &str) {
    let cases = load_cases(&root.join("goldens").join(golden));
    let case = cases
        .iter()
        .find(|case| case.name == case_name)
        .expect("case");
    let (data_dir, lookup) = if golden == "key_sequence.tsv.gz" {
        (root.join("goldens/key_sequence"), None)
    } else {
        (root.join("goldens/sound_to_char_shape"), Some("grave"))
    };
    let (observed, _) = replay(
        case,
        &data_dir,
        hux_core::host::DEFAULT_PAGE_SIZE,
        lookup,
        false,
    );
    println!("--- {golden} / {case_name}");
    for (step, actual) in case.steps.iter().zip(&observed) {
        let row = &actual.row;
        let join = |values: &[String]| -> String {
            if values.is_empty() {
                "-".to_string()
            } else {
                values.join(",")
            }
        };
        let golden_row = RowView::of_golden(step);
        println!(
            "        \"{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\",{}",
            actual.repr,
            row.consumed as u8,
            row.input,
            row.caret,
            row.commit,
            row.page_no,
            row.highlight,
            row.count,
            join(&row.candidates),
            join(&row.comments),
            if rows_differ(row, &golden_row) {
                "  // 偏离金样"
            } else {
                ""
            }
        );
    }
}
