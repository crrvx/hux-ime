// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! 键序列金样（2c）重放：真 librime 探针记录 vs Rust 会话逐步比对。
//!
//! 比对字段：`consumed`、输入、光标、提交、候选（按页：页码/数量/文本/注释/高亮）。
//! `preedit`（预编辑串）属宿主层职责，金样保留但不比对；
//! `page_no`（`fields[9]`）纳入比对（此前只以页切片间接约束）。
//! 处理器链：方案 `processor` 未消费（Forward）的键交 core `host` 模块（librime
//! `key_binder`/`selector`/`navigator`/`express_editor` 等价物）后比对 `consumed`；
//! 组合重建用 core `CompositionBuilder`（参照 `ConcreteEngine::Compose`），
//! 之后执行 update 通知器等价物（`interaction::update_notifier`）。
//!
//! 金样与数据：`goldens/key_sequence.tsv.gz`、`goldens/key_sequence/`（合成小码表）；
//! 音反查：`goldens/sound_to_char_shape.tsv.gz`、`goldens/sound_to_char_shape/`（小 PY_c + 音反查索引夹具）。
//! 再生成：`tools/generators/gen_key_sequence_golden.sh`、`tools/generators/gen_sound_to_char_shape_golden.sh`
//! （依赖系统 librime + librime-lua）。
//!
//! # 偏离守护（可证伪）
//!
//! 金样记录的是**上游行为**，本仓有若干处**已登记**的差异（见 [`DEVIATIONS`]）。
//! 登记项不是「跳过名单」而是**期望值表**，逐条满足：
//!
//! 1. **逐位断言**：该用例每一步都必须等于登记表里的**本仓期望行**（`consumed`/输入/
//!    光标/提交/高亮/候选数/候选/注释，与金样同字段）；
//! 2. **确有差异**：`期望行 != 金样行` 的步集合必须与 `实测 != 金样行` 的步集合**完全一致**，
//!    且非空 —— 实现若回退成上游行为，或某天上游 pin/金样追平使差异消失，都会**失败**
//!    （而不是静默通过）；登记表里写一个「实际并不偏离」的用例同样会失败；
//! 3. **不得静默跳过**：实际跳过的用例集合必须恰好等于登记集合（[`split_cases`]），
//!    登记名必须真实存在、唯一、步数吻合（`registry_is_falsifiable`）。
//!
//! 负面路径（把实现改回上游行为 / 塞入一个不偏离的用例）必须让本测试变红。

// 集成测试目标里 `mod x;` 相对 `tests/` 解析，故用显式路径指向本文件同名目录。
#[path = "key_sequence_differential/deviation.rs"]
mod deviation;
#[path = "key_sequence_differential/dump.rs"]
mod dump;
#[path = "key_sequence_differential/golden.rs"]
mod golden;
#[path = "key_sequence_differential/observe.rs"]
mod observe;
#[path = "key_sequence_differential/registry.rs"]
mod registry;
#[path = "key_sequence_differential/replay.rs"]
mod replay;
#[path = "key_sequence_differential/views.rs"]
mod views;

use std::path::PathBuf;

use deviation::DEVIATIONS;
use golden::Case;
use registry::registered_case;
use views::{RowView, expected_row, rows_differ};

#[test]
fn key_sequence_matches_reference() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..");
    let cases = golden::load_cases(&root.join("goldens/key_sequence.tsv.gz"));
    golden::check_coverage(&cases);
    // 上游金样原样保留：其中「上游缺陷」用例见 DEVIATIONS（登记 + 本仓期望值，不重生成）。
    let (kept, deviated) = registry::split_cases(&cases, "key_sequence.tsv.gz");
    let data_dir = root.join("goldens/key_sequence");
    let mut failures = Vec::new();
    let mut steps = 0usize;
    for case in &kept {
        steps += case.steps.len();
        replay::compare_with_golden(
            case,
            &data_dir,
            hux_core::host::DEFAULT_PAGE_SIZE,
            None,
            // 夹具 `tab_learning: false` ⇒ 参照学习库不就绪（同探针口径）。
            false,
            &mut failures,
        );
    }
    for (deviation, case) in &deviated {
        replay::compare_with_registered_expectations(
            deviation,
            case,
            &data_dir,
            hux_core::host::DEFAULT_PAGE_SIZE,
            None,
            false,
            &mut failures,
        );
    }
    // 重放面下限（同上）：非登记用例与步数不得变少（登记项由 `DEVIATIONS` 单独钉住）。
    // 68 例 / 285 步 − 6 个登记项（`punct_menu_equal` 3 + `punct_menu_minus` 3 +
    // `nav_page_home_minus` 4 + `digit_menu_select` 3 + `apostrophe_*_page` 各 5）= 62 例 / 262 步。
    assert!(
        kept.len() >= 62 && steps >= 262,
        "key_sequence 重放覆盖不足：{} 例 / {} 步（下限 62 例 / 262 步）",
        kept.len(),
        steps
    );
    assert!(
        failures.is_empty(),
        "键序列不一致 {} 处（{} 例 / {} 步，另 {} 例登记偏离）：\n{}",
        failures.len(),
        kept.len(),
        steps,
        deviated.len(),
        failures.join("\n")
    );
}

/// Tab 锁路径的**真机探针**金样。
///
/// 主金样 `key_sequence.tsv.gz` 的夹具写 `tiger_sentence/tab_learning: false` ⇒ 参照的
/// `learned.store` 为 nil，处理器里 `if not state.tab_pending and learned.store and
/// learned.store.db then … learned.baseline = first end` 这条分支**永不进入**；
/// 本夹具（`goldens/key_sequence_tab/tiger_sentence.custom.yaml`）写 `true` ⇒ 学习库就绪，
/// 分支进入。重放侧 `store_ready = true` 与之对应（两者必须一起改）。
///
/// 断言：①逐位比对真机记录；②**基线捕获确实发生**（`captures == 8`，即 8 个用例各一次
/// 首次 Tab）——把 `store_ready` 写回 false（或处理器不再捕获基线）即失败；
/// ③夹具条件本身入库（`custom.yaml`）且被本用例复核，防止「金样说 true、重放说 false」
/// 这类**两边一致地错**的漂移。
///
/// 实测事实（记录以免误读覆盖面）：本夹具的这些用例在 `tab_learning: true/false` 下的
/// **比对字段逐位相同**（Tab 的可见行为不因学习库就绪而变；真机探针两侧各跑一次实测），
/// 故本金样的增量是「参照分支真的走进去了 + 重放侧真的接上了 `learning_selection`」，
/// 而不是新的可见行为。
#[test]
fn key_sequence_tab_matches_reference() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..");
    let data_dir = root.join("goldens/key_sequence_tab");
    // 夹具条件与重放口径必须一起改（`tools/generators/gen_key_sequence_tab_golden.sh` 同断言）。
    let fixture = std::fs::read_to_string(data_dir.join("tiger_sentence.custom.yaml"))
        .expect("读取 Tab 夹具 custom.yaml");
    assert!(
        fixture.contains("tiger_sentence/tab_learning: true")
            && !fixture.contains("tab_learning: false"),
        "Tab 金样夹具必须开启 tab_learning（否则本金样失去意义）"
    );
    let cases = golden::load_cases(&root.join("goldens/key_sequence_tab.tsv.gz"));
    let golden_steps: usize = cases.iter().map(|case| case.steps.len()).sum();
    assert!(
        cases.len() >= 8,
        "key_sequence_tab 金样用例数不足：{} < 8（金样被截断？）",
        cases.len()
    );
    assert!(
        golden_steps >= 44,
        "key_sequence_tab 金样步数不足：{golden_steps} < 44（金样被截断？）"
    );
    let (kept, deviated) = registry::split_cases(&cases, "key_sequence_tab.tsv.gz");
    assert!(
        deviated.is_empty(),
        "Tab 金样是上游行为原样记录，不应有偏离登记"
    );
    let mut failures = Vec::new();
    let mut captures = 0usize;
    for case in &kept {
        captures += replay::compare_with_golden(
            case,
            &data_dir,
            hux_core::host::DEFAULT_PAGE_SIZE,
            None,
            true,
            &mut failures,
        );
    }
    assert!(
        failures.is_empty(),
        "Tab 键序列不一致 {} 处（{} 例）：\n{}",
        failures.len(),
        kept.len(),
        failures.join("\n")
    );
    assert_eq!(
        captures, 8,
        "夹具 tab_learning: true ⇒ 每个用例首次 Tab 都必须捕获学习基线（共 8 例）；\
         captures=0 说明重放侧没接上 store_ready，等价于回到「Tab 路径只有 processor 级用例」"
    );
}

/// 音反查金样：夹具页大小 5（与引擎一致，翻页用例覆盖后续页）；音反查前缀 `` ` ``。
#[test]
fn sound_to_char_shape_matches_reference() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..");
    let cases = golden::load_cases(&root.join("goldens/sound_to_char_shape.tsv.gz"));
    // 覆盖下限：同上。
    let golden_steps: usize = cases.iter().map(|case| case.steps.len()).sum();
    assert!(
        cases.len() >= 31,
        "sound_to_char_shape 金样用例数不足：{} < 31（金样被截断？）",
        cases.len()
    );
    assert!(
        golden_steps >= 164,
        "sound_to_char_shape 金样步数不足：{golden_steps} < 164（金样被截断？）"
    );
    // 同上：`nav-page-*` 记录的是上游「`=`/`-` 被标点分支遮蔽」的行为，见 DEVIATIONS。
    let (kept, deviated) = registry::split_cases(&cases, "sound_to_char_shape.tsv.gz");
    let data_dir = root.join("goldens/sound_to_char_shape");
    let mut failures = Vec::new();
    let mut steps = 0usize;
    for case in &kept {
        steps += case.steps.len();
        replay::compare_with_golden(
            case,
            &data_dir,
            hux_core::host::DEFAULT_PAGE_SIZE,
            Some("grave"),
            // 音反查夹具同样 `tab_learning: false`（与主方案夹具同源）。
            false,
            &mut failures,
        );
    }
    for (deviation, case) in &deviated {
        replay::compare_with_registered_expectations(
            deviation,
            case,
            &data_dir,
            hux_core::host::DEFAULT_PAGE_SIZE,
            Some("grave"),
            false,
            &mut failures,
        );
    }
    // 重放面下限（同上）：25 例 / 132 步（登记偏离 6 例 32 步另计）。
    assert!(
        kept.len() >= 25 && steps >= 132,
        "sound_to_char_shape 重放覆盖不足：{} 例 / {} 步（下限 25 例 / 132 步）",
        kept.len(),
        steps
    );
    assert!(
        failures.is_empty(),
        "音反查不一致 {} 处（{} 例 / {} 步，另 {} 例登记偏离）：\n{}",
        failures.len(),
        kept.len(),
        steps,
        deviated.len(),
        failures.join("\n")
    );
}

/// 登记表自校验（无需重放）：每个登记名必须在**指定金样**中真实存在且唯一；
/// 期望值步数与金样步数一致；登记项**确有差异**（否则报「偏离已消失/未登记」）；
/// 同一用例不得同时出现在两份金样。
#[test]
fn registry_is_falsifiable() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..");
    let goldens = ["key_sequence.tsv.gz", "sound_to_char_shape.tsv.gz"];
    let loaded: Vec<Vec<Case>> = goldens
        .iter()
        .map(|golden| golden::load_cases(&root.join("goldens").join(golden)))
        .collect();
    let mut seen: Vec<(&str, &str)> = Vec::new();
    for deviation in DEVIATIONS {
        assert!(
            goldens.contains(&deviation.golden),
            "未知金样：{}",
            deviation.golden
        );
        assert!(
            !seen.iter().any(|(_, case)| *case == deviation.case),
            "登录用例名 `{}` 重名，登记表无法唯一定位",
            deviation.case
        );
        seen.push((deviation.golden, deviation.case));
        let cases = &loaded[goldens
            .iter()
            .position(|golden| *golden == deviation.golden)
            .expect("金样下标")];
        let case = registered_case(cases, deviation, deviation.golden);
        assert_eq!(
            deviation.steps.len(),
            case.steps.len(),
            "{}: 登记期望值步数与金样不一致",
            deviation.case
        );
        let differing = deviation
            .steps
            .iter()
            .zip(&case.steps)
            .filter(|(raw, step)| rows_differ(&expected_row(raw), &RowView::of_golden(step)))
            .count();
        assert!(
            differing > 0,
            "{}: 登记表期望值与上游金样完全相同（并不偏离）—— \
             把「名字存在但不偏离」的用例写进登记表即可静默取消其全部步的比对，故必须失败",
            deviation.case
        );
    }
}

#[test]
#[ignore]
fn dump_registered_expectations() {
    crate::dump::run();
}
