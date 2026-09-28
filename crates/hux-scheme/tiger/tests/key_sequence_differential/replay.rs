// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! 用例重放与比对：真 librime 探针记录的逐步重放，以及与金样/登记期望值的比对。

use hux_core::host::HostOptions;
use hux_core::punct::PunctTable;
use hux_core::session::Context;
use hux_scheme_tiger::decode::Decoder;
use hux_scheme_tiger::interaction::{
    CompositionBuilder, K_SOUND_TO_CHAR_SHAPE_KEY, LiveLearning, OPTION_DIGIT_SELECT,
    OPTION_EARLY_COMMIT, OPTION_EARLY_COMMIT_TO_PREEDIT, SentenceState,
};
use hux_scheme_tiger::lexicon::{Lexicon, Supplement};
use std::path::Path;

use crate::deviation::Deviation;
use crate::golden::{Case, Step};
use crate::observe::observe_step;
use crate::views::{Observed, RowView, compare, describe, expected_row, rows_differ};

// ---------------------------------------------------------------- 重放

/// 重放一个用例；返回（逐步观测, [`store_ready`] 基线捕获次数）。
///
/// `store_ready` 对应参照的 `learned.store and learned.store.db`（金样夹具的
/// `tiger_sentence/tab_learning`）：`goldens/key_sequence.tsv.gz` 的夹具是 `false`，
/// `goldens/key_sequence_tab.tsv.gz` 的夹具是 `true`——**两者必须一起改**。
pub fn replay(
    case: &Case,
    data_dir: &Path,
    page_size: usize,
    lookup_key: Option<&str>,
    store_ready: bool,
) -> (Vec<Observed>, usize) {
    let Fixture {
        mut decoder,
        mut context,
        mut state,
        mut live,
        mut builder,
        punct,
        host_options,
    } = fixture(case, data_dir, page_size, lookup_key, store_ready);
    let mut baseline_captures = 0usize;
    let mut dot_armed = false;
    let mut observed = Vec::with_capacity(case.steps.len());
    for (index, step) in case.steps.iter().enumerate() {
        let (observation, baseline_capture) = observe_step(
            &case.name,
            step,
            &mut decoder,
            &mut context,
            &mut state,
            &mut live,
            &mut builder,
            punct.as_ref(),
            &host_options,
            page_size,
            &mut dot_armed,
            index,
            store_ready,
        );
        if baseline_capture {
            baseline_captures += 1;
        }
        observed.push(observation);
    }
    (observed, baseline_captures)
}

/// 重放所需的引擎与宿主状态。
struct Fixture {
    decoder: Decoder,
    context: Context,
    state: SentenceState,
    live: LiveLearning,
    builder: CompositionBuilder,
    punct: Option<PunctTable>,
    host_options: HostOptions,
}

/// 按夹具建好引擎与宿主状态。
fn fixture(
    case: &Case,
    data_dir: &Path,
    page_size: usize,
    lookup_key: Option<&str>,
    store_ready: bool,
) -> Fixture {
    let dirs = [data_dir.to_path_buf()];
    let lexicon = Lexicon::load(&dirs, 0);
    let supplement = Supplement::load_default(Some(data_dir));
    let decoder = Decoder::new(lexicon, supplement, None);
    let mut context = Context::new();
    let state = SentenceState::fresh(1);
    let live = LiveLearning {
        store_ready,
        ..Default::default()
    };
    context.set_option("ascii_mode", false);
    context.set_option("_auto_commit", true);
    context.set_option(OPTION_EARLY_COMMIT, true);
    context.set_option(OPTION_EARLY_COMMIT_TO_PREEDIT, false);
    // **出厂缺省口径**：`tiger_sentence_digit_select` 在
    // `hux-cfg`/addon/`TigerScheme` 三处都是 `true`，差分层按同一口径重放
    // （addon 扩展，金样记录的是上游行为 ⇒ 数字直选用例登记在 `DEVIATIONS`）。
    context.set_option(OPTION_DIGIT_SELECT, true);
    if let Some(key) = lookup_key {
        context.set_property(K_SOUND_TO_CHAR_SHAPE_KEY, key);
    }
    for (name, value) in &case.options {
        context.set_option(name, *value);
    }
    let builder = CompositionBuilder::default();
    let (punct_table, punct_error) = PunctTable::load_first(&[data_dir.join("symbols.yaml")]);
    assert!(
        punct_table.is_some(),
        "缺少标点表 symbols.yaml：{punct_error:?}"
    );
    let punct = punct_table;
    // 处理器与宿主链共用同一份宿主选项（翻页键绑定判据一致；见 `ProcessorEnv::host_options`）。
    let host_options = HostOptions {
        page_size,
        ..HostOptions::default()
    };
    Fixture {
        decoder,
        context,
        state,
        live,
        builder,
        punct,
        host_options,
    }
}

/// 非登记用例：与金样逐位比对。
pub fn compare_with_golden(
    case: &Case,
    data_dir: &Path,
    page_size: usize,
    lookup_key: Option<&str>,
    store_ready: bool,
    failures: &mut Vec<String>,
) -> usize {
    let (observed, baseline_captures) = replay(case, data_dir, page_size, lookup_key, store_ready);
    assert_eq!(
        observed.len(),
        case.steps.len(),
        "{}: 重放步数与金样不一致",
        case.name
    );
    for (index, (step, actual)) in case.steps.iter().zip(&observed).enumerate() {
        let label = format!("{}[{}] {}", case.name, index, actual.repr);
        compare(&label, &RowView::of_golden(step), &actual.row, failures);
    }
    baseline_captures
}

/// 登记用例：逐位断言**本仓期望值**，并要求「与金样的差异集合」实测与登记一致且非空。
pub fn compare_with_registered_expectations(
    deviation: &Deviation,
    case: &Case,
    data_dir: &Path,
    page_size: usize,
    lookup_key: Option<&str>,
    store_ready: bool,
    failures: &mut Vec<String>,
) -> usize {
    let (observed, baseline_captures) = replay(case, data_dir, page_size, lookup_key, store_ready);
    assert_eq!(
        observed.len(),
        case.steps.len(),
        "{}: 重放步数与金样不一致",
        deviation.case
    );
    let mut differing_registered = 0usize;
    let mut differing_observed = 0usize;
    for (index, ((raw, step), actual)) in deviation
        .steps
        .iter()
        .zip(&case.steps)
        .zip(&observed)
        .enumerate()
    {
        let label = format!("{}[{}] {}", deviation.case, index, actual.repr);
        let (registered, observed_differing) =
            compare_registered_step(deviation, &label, raw, step, actual, failures);
        differing_registered += registered;
        differing_observed += observed_differing;
    }
    if differing_registered == 0 {
        failures.push(format!(
            "{}: 登记表里该用例（{:?}）的期望值与上游金样**完全相同**（并不偏离）—— \
             登记一个不偏离的用例会静默取消其全部步的比对；请从 DEVIATIONS 删除",
            deviation.case, deviation.kind
        ));
    }
    if differing_registered != differing_observed {
        failures.push(format!(
            "{}: 与金样有差异的步数 实测 {} != 登记 {}（{:?}）",
            deviation.case, differing_observed, differing_registered, deviation.kind
        ));
    }
    baseline_captures
}

/// 逐步比对登记用例：① 本仓期望值逐位断言；② 偏离形状（登记集合 vs 实测集合）。
///
/// 返回（登记为偏离的步数, 实测偏离金样的步数）。
fn compare_registered_step(
    deviation: &Deviation,
    label: &str,
    raw: &str,
    step: &Step,
    actual: &Observed,
    failures: &mut Vec<String>,
) -> (usize, usize) {
    let mut differing_registered = 0usize;
    let mut differing_observed = 0usize;
    let expected = expected_row(raw);
    let golden = RowView::of_golden(step);
    // ① 本仓期望值：逐位断言。
    compare(label, &expected, &actual.row, failures);
    // ② 偏离形状：登记表与实测「相对金样有差异的步」集合必须一致。
    if rows_differ(&expected, &golden) {
        differing_registered += 1;
    }
    if rows_differ(&actual.row, &golden) {
        differing_observed += 1;
    }
    if rows_differ(&expected, &golden) && !rows_differ(&actual.row, &golden) {
        failures.push(format!(
            "{label}: 登记的偏离（{:?}）已消失 —— 实测与上游金样一致，\
             实现可能已回退成上游行为；期望 {} 实测 {}",
            deviation.kind,
            describe(&expected),
            describe(&actual.row)
        ));
    }
    if !rows_differ(&expected, &golden) && rows_differ(&actual.row, &golden) {
        failures.push(format!(
            "{label}: 实测与上游金样存在**未登记**的差异（登记种类 {:?}）：实测 {} 金样 {}",
            deviation.kind,
            describe(&actual.row),
            describe(&golden)
        ));
    }
    (differing_registered, differing_observed)
}
