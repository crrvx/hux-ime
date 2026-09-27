// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! 融合排序用例：候选助手与两列归并的参照用例。

use super::super::fusion::apply_fusion_ordering;
use super::*;

/// 融合排序用例的最小候选（其余字段与排序无关）。
fn fusion_candidate(text: &str, source_mask: u8, direct_rank: f64) -> Evaluated {
    Evaluated {
        text: text.to_string(),
        score: 0.0,
        confidence_score: 0.0,
        early_commit_confidence_score: 0.0,
        code_score: 0.0,
        max_rank: 1,
        supplement_score: 0.0,
        learning_score: 0.0,
        edge_count: 1,
        source_mask,
        direct_rank,
        path: 0,
        segmented: String::new(),
        previous_raw_length: 0,
        previous_text: None,
    }
}

fn fusion_texts(candidates: &[Evaluated]) -> Vec<&str> {
    candidates.iter().map(|item| item.text.as_str()).collect()
}

/// 融合排序索引：一条 `fusion_event` 落到 `mode` 的融合分区里。
fn fusion_index(
    mode: &str,
    raw: &[u8],
    direct: &str,
    composed: &str,
    direct_wins: bool,
) -> LearningIndex {
    let event = hux_core::learning::fusion_event(
        mode,
        raw,
        direct,
        composed,
        direct_wins,
        raw.len(),
        1000.0,
    )
    .expect("融合事件");
    LearningIndex::build(
        &[hux_core::learning::Event {
            time: event.time,
            mode: event.mode,
            code: event.code,
            text: event.text,
            context: event.context,
        }],
        1000.0,
    )
}

/// 照抄上游 `tools/test_sentence_learning.lua` 的融合用例：
/// 无事件保持原交错序；一条 `C > A` 的 Direct 偏好只把 Direct 前缀提到 A 之前。
#[test]
fn fusion_ordering_matches_reference_cases() {
    let mode = "sentence-v2|test";
    let cases = || {
        vec![
            fusion_candidate("A", SOURCE_COMPOSED, f64::INFINITY),
            fusion_candidate("B", SOURCE_DIRECT, 1.0),
            fusion_candidate("C", SOURCE_DIRECT, 2.0),
        ]
    };
    // 无学习库：两侧前缀分都是 0 ⇒ 回退原始下标顺序。
    let mut items = cases();
    apply_fusion_ordering(None, mode, b"ii", &mut items);
    assert_eq!(
        fusion_texts(&items),
        ["A", "B", "C"],
        "无学习库时融合必须回退原始下标顺序"
    );
    // 空模式（未接入学习）同样退化为原序。
    let mut items = cases();
    apply_fusion_ordering(None, "", b"ii", &mut items);
    assert_eq!(
        fusion_texts(&items),
        ["A", "B", "C"],
        "空模式下同样退化为原序，不得报错或重排"
    );
    // 一条 `Direct C > Composed A`：Direct 列的 B、C 一起前移，A 退到最后。
    let mut index = fusion_index(mode, b"ii", "C", "A", true);
    let mut items = cases();
    apply_fusion_ordering(Some(&mut index), mode, b"ii", &mut items);
    assert_eq!(
        fusion_texts(&items),
        ["B", "C", "A"],
        "Direct 前缀整体前移：B、C 一起提到 A 之前"
    );
}

/// 融合排序的两条独立规则：全 Direct 按直接序 rank 排，Composed 胜时 Composed 列先出。
#[test]
fn fusion_ordering_preserves_direct_order_and_reverse_preference() {
    let mode = "sentence-v2|test";
    // 全为 Direct：与学习库无关地按 `direct_rank` 重排（「直接序保持」）。
    let mut items = vec![
        fusion_candidate("C", SOURCE_DIRECT, 2.0),
        fusion_candidate("A", SOURCE_DIRECT, f64::INFINITY),
        fusion_candidate("B", SOURCE_DIRECT, 1.0),
    ];
    apply_fusion_ordering(None, mode, b"ii", &mut items);
    assert_eq!(
        fusion_texts(&items),
        ["B", "C", "A"],
        "全 Direct 时按直接序 rank 重排为 B、C、A"
    );
    // 反向偏好（Composed 胜）：Composed 列先出。
    let mut index = fusion_index(mode, b"ii", "B", "A", false);
    let mut items = vec![
        fusion_candidate("B", SOURCE_DIRECT, 1.0),
        fusion_candidate("A", SOURCE_COMPOSED, f64::INFINITY),
    ];
    apply_fusion_ordering(Some(&mut index), mode, b"ii", &mut items);
    assert_eq!(
        fusion_texts(&items),
        ["A", "B"],
        "Composed 胜时 Composed 列先出"
    );
}
