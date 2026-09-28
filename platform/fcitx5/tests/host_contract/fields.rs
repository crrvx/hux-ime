// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! `Settings` 字段表 ↔ C++ 配置 schema 的**双向**守护。
//!
//! 每条都用 `offset_of!` 引用真实字段名（改名字段即编译失败），`FIELDS.len()` 钉住字段数；
//! 另一向要求每个字段都在 schema 里声明、schema 里除宿主显示项外不得有引擎不认识的路径。

use hux_cfg::Settings;
use hux_test_support::repo_path;

// （字段名，承载它的 schema 路径名，偏移）。顺序 = `Settings` 声明序。
const FIELDS: &[(&str, &str, usize)] = &[
    (
        "early_commit",
        "EarlyCommitMode",
        std::mem::offset_of!(Settings, early_commit),
    ),
    (
        "early_commit_to_preedit",
        "EarlyCommitMode",
        std::mem::offset_of!(Settings, early_commit_to_preedit),
    ),
    (
        "allow_duplicate_single",
        "AllowDuplicateSingle",
        std::mem::offset_of!(Settings, allow_duplicate_single),
    ),
    (
        "full_shape",
        "PunctMode",
        std::mem::offset_of!(Settings, full_shape),
    ),
    (
        "ascii_punct",
        "PunctMode",
        std::mem::offset_of!(Settings, ascii_punct),
    ),
    (
        "learning_on_tab",
        "TabLearning",
        std::mem::offset_of!(Settings, learning_on_tab),
    ),
    (
        "digit_select",
        "DigitSelect",
        std::mem::offset_of!(Settings, digit_select),
    ),
    (
        "full_charset",
        "FullCharset",
        std::mem::offset_of!(Settings, full_charset),
    ),
    (
        "filter_non_han",
        "FilterNonHan",
        std::mem::offset_of!(Settings, filter_non_han),
    ),
    (
        "page_cycle",
        "PageCycle",
        std::mem::offset_of!(Settings, page_cycle),
    ),
    (
        "high_freq_limit",
        "HighFreqLimit",
        std::mem::offset_of!(Settings, high_freq_limit),
    ),
    (
        "page_size",
        "PageSize",
        std::mem::offset_of!(Settings, page_size),
    ),
    (
        "min_retained_input_length",
        "MinRetainedRawLength",
        std::mem::offset_of!(Settings, min_retained_input_length),
    ),
    (
        "candidate_layout",
        "CandidateLayout",
        std::mem::offset_of!(Settings, candidate_layout),
    ),
    (
        "preedit_mode",
        "PreeditMode",
        std::mem::offset_of!(Settings, preedit_mode),
    ),
    (
        "page_up_keys",
        "PageUpKey",
        std::mem::offset_of!(Settings, page_up_keys),
    ),
    (
        "page_down_keys",
        "PageDownKey",
        std::mem::offset_of!(Settings, page_down_keys),
    ),
    (
        "reverse_lookup_pronunciation_keys",
        "SoundToCharShapeKey",
        std::mem::offset_of!(Settings, reverse_lookup_pronunciation_keys),
    ),
    (
        "reverse_lookup_character_keys",
        "CharToSoundShapeKey",
        std::mem::offset_of!(Settings, reverse_lookup_character_keys),
    ),
];

// 只服务宿主显示、不经引擎的 schema 项（与上一条测试的 `_ => continue` 一致）。
const HOST_ONLY_PATHS: &[&str] = &["PanelPreedit"];

/// 反向守护：上面那条测试只保证「schema 里出现的项与 `Settings` 一致」，
/// 是**单向**的——新增一个 `Settings` 字段而不写进 `shell/hux.cpp` 的 schema 不会失败。
/// 本测试补上另一向：`Settings` 的每个字段都必须在 schema 中声明，反之 schema 里除
/// `HOST_ONLY_PATHS`（只服务宿主显示、不经引擎的项）外不得出现引擎不认识的路径。
///
/// 表内每项都用 `offset_of!` 引用真实字段名 ⇒ **改名字段即编译失败**；`FIELDS.len()` 被钉住
/// ⇒ 新增字段必须同步本表与配置页（否则此测试先红）。这正是本表要堵的漂移入口。
/// 两个三态项各承载两个字段（「提前上屏」↔ `early_commit` / `early_commit_to_preedit`、
/// 「标点」↔ `ascii_punct` / `full_shape`），故路径列允许重复、比对前对期望集合去重。
#[test]
fn every_settings_field_is_declared_in_the_schema() {
    fields_are_declared_once_per_field();
    schema_paths_match_the_field_table();
}

/// 字段表自检：数量钉死、偏移互异且落在结构体内。
fn fields_are_declared_once_per_field() {
    assert_eq!(
        FIELDS.len(),
        19,
        "Settings 字段数变化：新增/删除字段必须同步本表与 shell/hux.cpp 的 schema（或将新增项登记为宿主显示项）"
    );
    // 字段顺序由 `repr(Rust)` 决定（编译器会重排），故**不假设**「声明序 == 偏移序」；
    // 只要求偏移互异且落在结构体内——重复登记或张冠李戴都会被抓住。
    let mut offsets: Vec<usize> = FIELDS.iter().map(|(_, _, offset)| *offset).collect();
    let size = std::mem::size_of::<Settings>();
    assert!(
        offsets.iter().all(|offset| *offset < size),
        "FIELDS 中有偏移越界项（size_of::<Settings>() = {size}）"
    );
    offsets.sort_unstable();
    offsets.dedup();
    assert_eq!(offsets.len(), FIELDS.len(), "FIELDS 中两个字段指向同一偏移");
}

/// schema 侧自检：每个字段都有声明，且路径集合与字段表双向一致。
fn schema_paths_match_the_field_table() {
    let source =
        std::fs::read_to_string(repo_path("platform/fcitx5/shell/hux.cpp")).expect("read hux.cpp");
    let mut declared: Vec<&str> = source
        .split(".path{\"")
        .skip(1)
        .filter_map(|part| part.split('"').next())
        .collect();
    declared.sort_unstable();
    declared.dedup();

    for (field, path, _) in FIELDS {
        assert!(
            declared.contains(path),
            "Settings::{field} 未在 shell/hux.cpp 的 schema 中声明（新增字段须同步配置页，\
             或在 HOST_ONLY_PATHS 登记为宿主显示项）"
        );
    }
    let mut engine_paths: Vec<&str> = declared
        .iter()
        .copied()
        .filter(|path| !HOST_ONLY_PATHS.contains(path))
        .collect();
    engine_paths.sort_unstable();
    let mut expected: Vec<&str> = FIELDS.iter().map(|(_, path, _)| *path).collect();
    expected.sort_unstable();
    // 一个三态项承载两个字段 ⇒ 期望集合先去重；`declared` 本身已去重。
    expected.dedup();
    assert_eq!(
        engine_paths, expected,
        "schema 路径集合与 Settings 字段表不一致（双向守护：两侧都必须有对方）"
    );
}
