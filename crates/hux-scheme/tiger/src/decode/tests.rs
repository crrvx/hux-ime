// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

use super::beam::{has_letter, normalize, parse_boundaries, parse_selector};
use super::fusion::apply_fusion_ordering;
use super::*;

fn fixture_lexicon() -> Lexicon {
    let dir = hux_test_support::repo_path("goldens/lexicon");
    Lexicon::load(std::slice::from_ref(&dir), 1500)
}

fn fixture_decoder() -> Decoder {
    let dir = hux_test_support::repo_path("goldens/lexicon");
    let lexicon = Lexicon::load(std::slice::from_ref(&dir), 1500);
    let supplement = Supplement::load_default(Some(&dir));
    Decoder::new(lexicon, supplement, None)
}

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

/// 归一化同时做去空白、去控制符、统一小写三件事，后续所有比较都依赖它。
#[test]
fn normalize_strips_whitespace_and_control() {
    assert_eq!(
        normalize("A B\tC\r\n"),
        b"abc",
        "空白与控制符都要剔除，字母统一小写"
    );
    assert_eq!(
        normalize("a\u{0b}b"),
        b"ab",
        "垂直制表符同属控制符，必须剔除"
    );
}

/// 字母判据决定输入走哪条解码支路，纯数字必须落到另一支。
#[test]
fn has_letter_detects_ascii_letters() {
    assert!(has_letter(b"a1"), "含字母即判真");
    assert!(!has_letter(b"123"), "纯数字不算字母");
}

/// 选择器解析对齐参照实现的匹配口径：分号取原值、撇号固定 3、0 表示第 10 项、溢出饱和。
#[test]
fn parse_selector_parses_and_saturates() {
    assert_eq!(
        parse_selector(b"ab;", 2),
        (2, 3),
        "分号选择器序号取分号前的值，游标停在其后"
    );
    assert_eq!(parse_selector(b"ab'", 2), (3, 3), "撇号选择器序号固定为 3");
    assert_eq!(parse_selector(b"ab0", 2), (10, 3), "数字 0 代表第 10 项");
    assert_eq!(parse_selector(b"ab12", 2), (12, 4), "多位数字按十进制解析");
    assert_eq!(
        parse_selector(b"ab00", 2),
        (0, 4),
        "00 解析为第 0 项，不得当成 10"
    );
    assert_eq!(
        parse_selector(b"ab99999999999999999999", 2),
        (u64::MAX, 22),
        "溢出必须饱和到 u64::MAX，不得 panic 或回绕"
    );
    assert_eq!(
        parse_selector(b"ab", 2),
        (0, 2),
        "无选择器时序号 0 且游标不动"
    );
}

/// 锁定前缀代表已上屏内容：重建出的每个候选都必须以它开头。
#[test]
fn locked_decode_rebuilds_confirmed_prefix() {
    let dir = hux_test_support::repo_path("goldens/lexicon");
    let lexicon = Lexicon::load(std::slice::from_ref(&dir), 1500);
    let supplement = Supplement::load_default(Some(&dir));
    let mut decoder = Decoder::new(lexicon, supplement, None);
    let unlocked = decoder.decode_with("ab", false, "").expect("decode");
    let top = unlocked.items.first().expect("candidates").clone();
    // 路径链 root..top；取第一个非根节点作为局部锁边界。
    let mut chain = Vec::new();
    let mut current = Some(top.path);
    while let Some(index) = current {
        chain.push(index);
        current = decoder.arena[index].previous;
    }
    chain.reverse();
    assert!(chain.len() >= 2, "期望多节点路径");
    let node = chain[1];
    let (raw_length, text_length) = (
        decoder.arena[node].raw_length,
        decoder.arena[node].text_length,
    );
    let locked_text = decoder.arena[node].text.clone();
    let locked_raw = "ab"[..raw_length].to_string();
    let boundaries = format!("{raw_length},{text_length};");
    let lock = DecodeLock {
        raw: &locked_raw,
        text: &locked_text,
        boundaries: &boundaries,
    };
    let locked = decoder
        .decode_with_lock("ab", false, "", Some(lock))
        .expect("locked decode");
    assert!(!locked.items.is_empty(), "锁住前缀后仍必须给出候选");
    for item in &locked.items {
        assert!(item.text.starts_with(&locked_text), "{}", item.text);
    }
}

/// 边界覆盖整段输入的全量锁必须原样复现顶层候选文本。
#[test]
fn locked_decode_honors_full_input_lock() {
    let dir = hux_test_support::repo_path("goldens/lexicon");
    let lexicon = Lexicon::load(std::slice::from_ref(&dir), 1500);
    let supplement = Supplement::load_default(Some(&dir));
    let mut decoder = Decoder::new(lexicon, supplement, None);
    let unlocked = decoder.decode_with("ab", false, "").expect("decode");
    let top = unlocked.items.first().expect("candidates").clone();
    let mut chain = Vec::new();
    let mut current = Some(top.path);
    while let Some(index) = current {
        chain.push(index);
        current = decoder.arena[index].previous;
    }
    chain.reverse();
    // 全量锁：以顶层候选路径的全部边界重建，前缀即整段输入。
    let boundaries: String = chain[1..]
        .iter()
        .map(|&index| {
            format!(
                "{},{};",
                decoder.arena[index].raw_length, decoder.arena[index].text_length
            )
        })
        .collect();
    let lock = DecodeLock {
        raw: "ab",
        text: &top.text,
        boundaries: &boundaries,
    };
    let locked = decoder
        .decode_with_lock("ab", false, "", Some(lock))
        .expect("locked decode");
    assert!(!locked.items.is_empty(), "全量锁下候选不得为空");
    assert!(
        locked.items.iter().all(|item| item.text == top.text),
        "{:?}",
        locked
            .items
            .iter()
            .map(|item| &item.text)
            .collect::<Vec<_>>()
    );
}

/// 锁只钉住前缀，剩余输入仍要正常解码并允许扩展出多字候选。
#[test]
fn locked_decode_expands_after_partial_lock() {
    let dir = hux_test_support::repo_path("goldens/lexicon");
    let lexicon = Lexicon::load(std::slice::from_ref(&dir), 1500);
    let supplement = Supplement::load_default(Some(&dir));
    let mut decoder = Decoder::new(lexicon, supplement, None);
    // 码表事实：ab → 交（rank 1）、疒（rank 2）；整段输入 >1 字节时单字节尾边被跳过，
    // 故 "abab" 唯一两段路径为 ab+ab。锁住首边后应继续解出 交交/交疒。
    let lock = DecodeLock {
        raw: "ab",
        text: "交",
        boundaries: "2,3;",
    };
    let locked = decoder
        .decode_with_lock("abab", false, "", Some(lock))
        .expect("locked decode");
    assert!(!locked.items.is_empty(), "局部锁后仍要解出剩余输入");
    assert!(
        locked.items.iter().all(|item| item.text.starts_with("交")),
        "所有候选都必须沿用锁定的前缀文本"
    );
    assert!(
        locked
            .items
            .iter()
            .any(|item| item.text.chars().count() > 1),
        "扩展应产生多字候选：{:?}",
        locked
            .items
            .iter()
            .map(|item| &item.text)
            .collect::<Vec<_>>()
    );
}

/// 原始串不符、空串、零边界、文本长度不符都必须整条拒绝，不得静默降级成无锁解码。
#[test]
fn locked_decode_rejects_mismatches() {
    let dir = hux_test_support::repo_path("goldens/lexicon");
    let lexicon = Lexicon::load(std::slice::from_ref(&dir), 1500);
    let supplement = Supplement::load_default(Some(&dir));
    let mut decoder = Decoder::new(lexicon, supplement, None);
    let unlocked = decoder.decode_with("ab", false, "").expect("decode");
    let top = unlocked.items.first().expect("candidates").clone();
    let node = decoder.arena[top.path].previous.expect("non-root path");
    let (raw_length, text_length) = (
        decoder.arena[node].raw_length,
        decoder.arena[node].text_length,
    );
    let locked_text = decoder.arena[node].text.clone();
    let locked_raw = "ab"[..raw_length].to_string();
    let boundaries = format!("{raw_length},{text_length};");
    let raw_mismatch = DecodeLock {
        raw: "xy",
        text: &locked_text,
        boundaries: &boundaries,
    };
    assert!(
        decoder
            .decode_with_lock("ab", false, "", Some(raw_mismatch))
            .unwrap()
            .items
            .is_empty(),
        "锁前缀与输入不符：整条拒绝给空候选"
    );
    let empty_raw = DecodeLock {
        raw: "",
        text: &locked_text,
        boundaries: &boundaries,
    };
    assert!(
        decoder
            .decode_with_lock("ab", false, "", Some(empty_raw))
            .unwrap()
            .items
            .is_empty(),
        "空锁前缀视为不匹配，不得当成无锁"
    );
    let short = "0,0;".to_string();
    let short_lock = DecodeLock {
        raw: &locked_raw,
        text: &locked_text,
        boundaries: &short,
    };
    assert!(
        decoder
            .decode_with_lock("ab", false, "", Some(short_lock))
            .unwrap()
            .items
            .is_empty(),
        "零边界与锁文本不符，必须拒绝"
    );
    // 边界文本长度与锁文本不一致（"a" != "ab"）
    let text_boundary = format!("{raw_length},1;");
    let text_lock = DecodeLock {
        raw: &locked_raw,
        text: "ab",
        boundaries: &text_boundary,
    };
    assert!(
        decoder
            .decode_with_lock("ab", false, "", Some(text_lock))
            .unwrap()
            .items
            .is_empty(),
        "边界文本长度与锁文本不一致也必须拒绝"
    );
}

/// 路径摘要供宿主展示分段：节点由外向内且原始长度严格递增。
#[test]
fn path_summary_orders_nodes_outermost_first() {
    let dir = hux_test_support::repo_path("goldens/lexicon");
    let lexicon = Lexicon::load(std::slice::from_ref(&dir), 1500);
    let supplement = Supplement::load_default(Some(&dir));
    let mut decoder = Decoder::new(lexicon, supplement, None);
    let output = decoder.decode_with("abab", false, "").expect("decode");
    let item = output.items.first().expect("candidates");
    let (raw_length, diff) = decoder.path_summary(item);
    assert_eq!(raw_length, 4, "路径摘要必须覆盖整段输入");
    assert_eq!(diff.text, item.text, "摘要文本必须等于候选文本");
    assert!(!diff.path.is_empty(), "完整候选必须带非空路径");
    assert!(
        diff.path
            .windows(2)
            .all(|window| window[0].raw_length < window[1].raw_length),
        "路径节点由外向内，原始长度必须严格递增"
    );
}

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

/// 排序先验的缺省值是调参基线，任何改动都必须显式并在这里留下痕迹。
#[test]
fn ranking_prior_parameters_defaults() {
    let parameters = RankingPriorParameters::default();
    assert_eq!(
        parameters.canonical_code_reward, 2.0,
        "规范码奖励缺省为 2.0"
    );
    assert_eq!(
        parameters.lexical_prior_weight, 0.1,
        "词法先验权重缺省为 0.1"
    );
    assert_eq!(
        parameters.lexical_candidate_limit, 5,
        "词法候选上限缺省为 5"
    );
    assert_eq!(
        parameters.canonical_isolation_min_code_length, 4,
        "规范隔离最小码长缺省为 4"
    );
    // 5ce1ca2 新增的早提交先验（参照 `ranking_prior.*` 默认值）。
    assert_eq!(
        parameters.supplement_early_commit_scale, 0.05,
        "补充早提交斜率缺省为 0.05"
    );
    assert_eq!(
        parameters.supplement_early_commit_cap, 0.75,
        "补充早提交上限缺省为 0.75"
    );
    assert_eq!(
        parameters.personalized_early_commit_cap, 0.80,
        "个性化早提交上限缺省为 0.80"
    );
    assert_eq!(
        parameters.empty_code_strong_share, 0.99999,
        "空码强阈值缺省为 0.99999"
    );
    // 空码强阈值比普通强阈值（0.999）更严。
    assert!(
        parameters.empty_code_strong_share > 0.999,
        "空码强阈值必须严于普通强阈值 0.999"
    );
    // decoder 默认即内建参数。
    let decoder = fixture_decoder();
    assert_eq!(
        decoder.ranking_prior_parameters(),
        parameters,
        "新建解码器的先验参数必须等于内建缺省"
    );
}

/// 参数可注入是消融实验的前提：setter 后回读必须一致。
#[test]
fn ranking_prior_parameters_setter_roundtrip() {
    let mut decoder = fixture_decoder();
    decoder.set_ranking_prior_parameters(RankingPriorParameters {
        canonical_code_reward: 1.0,
        ..RankingPriorParameters::default()
    });
    assert_eq!(
        decoder.ranking_prior_parameters().canonical_code_reward,
        1.0,
        "setter 写入后必须原样回读，不得被缺省覆盖"
    );
}

/// 早提交贡献遵循参照公式：负分不出力、正分线性放大、到上限封住。
#[test]
fn supplement_early_commit_contribution_matches_reference() {
    // 参照 `min(supplement_early_commit_cap, max(0, score) * scale)`。
    let parameters = RankingPriorParameters::default();
    assert_eq!(
        parameters.supplement_early_commit_contribution(0.0),
        0.0,
        "零分不得贡献早提交加成"
    );
    assert_eq!(
        parameters.supplement_early_commit_contribution(-3.0),
        0.0,
        "负分夹到 0，不得反向惩罚候选"
    );
    assert_eq!(
        parameters.supplement_early_commit_contribution(4.0),
        0.2,
        "正分按斜率线性放大"
    );
    assert_eq!(
        parameters.supplement_early_commit_contribution(15.0),
        0.75,
        "超过斜率对应值即触顶"
    );
    assert_eq!(
        parameters.supplement_early_commit_contribution(1000.0),
        0.75,
        "极大分同样受上限约束"
    );
    // 消融：放大斜率后仍受上限约束。
    let scaled = RankingPriorParameters {
        supplement_early_commit_scale: 0.5,
        ..parameters
    };
    assert_eq!(
        scaled.supplement_early_commit_contribution(2.0),
        0.75,
        "放大斜率后仍以上限封顶"
    );
}

/// 码形证据只来自词法模型，没有模型时码分必须恒 0，不得有别的来源。
#[test]
fn decode_without_model_has_zero_code_score() {
    let mut decoder = fixture_decoder();
    // 无模型时码形证据不累计，故恒为 0。
    let output = decoder.decode_with("ab", false, "").expect("decode");
    assert!(!output.items.is_empty(), "无词法模型时仍必须给出码表候选");
    assert!(
        output.items.iter().all(|item| item.code_score == 0.0),
        "无模型即无码形证据，码分必须恒为 0"
    );
}

/// 词法模型由配置层后装，默认必须为空以免测试与运行态互相污染。
#[test]
fn lexical_model_setter_roundtrip() {
    let mut decoder = fixture_decoder();
    assert!(
        decoder.lexical_model().is_none(),
        "默认解码器不得自带词法模型"
    );
    let model = crate::lexical::load(&hux_test_support::repo_path(
        "data/tiger_sentence.lexical.bin",
    ))
    .expect("load lexical model");
    decoder.set_lexical_model(Some(model));
    assert!(
        decoder.lexical_model().is_some(),
        "装入词法模型后必须能读回"
    );
}

/// 文本级退格会产生与码表边不对应的锁，此时按中立码证据重放而不是整段拒绝。
#[test]
fn locked_decode_replays_opaque_prefix_neutrally() {
    let dir = hux_test_support::repo_path("goldens/lexicon");
    let lexicon = Lexicon::load(std::slice::from_ref(&dir), 1500);
    let supplement = Supplement::load_default(Some(&dir));
    let mut decoder = Decoder::new(lexicon, supplement, None);
    // 锁文本与任何码表边都不对应（文本级退格产生的"不透明"锁）：
    // 参照 12d2ecc 起以中立码证据重放，而不是整段拒绝。
    let lock = DecodeLock {
        raw: "ab",
        text: "某某",
        boundaries: "2,6;",
    };
    let locked = decoder
        .decode_with_lock("abab", false, "", Some(lock))
        .expect("locked decode");
    assert!(!locked.items.is_empty(), "不透明锁前缀也要给出候选");
    assert!(
        locked
            .items
            .iter()
            .all(|item| item.text.starts_with("某某")),
        "{:?}",
        locked
            .items
            .iter()
            .map(|item| &item.text)
            .collect::<Vec<_>>()
    );
}

/// 边界解析照抄参照实现的匹配语义：失败起点右移重试，同段取最后一组。
#[test]
fn parse_boundaries_matches_gmatch() {
    assert_eq!(
        parse_boundaries("2,3;"),
        vec![(2, 3)],
        "单段边界必须解析为一对数字"
    );
    assert_eq!(
        parse_boundaries("2,3;4,6;"),
        vec![(2, 3), (4, 6)],
        "多段边界按出现顺序解析"
    );
    assert_eq!(
        parse_boundaries(""),
        Vec::<(usize, usize)>::new(),
        "空串给空表，不得报错"
    );
    assert_eq!(
        parse_boundaries("abc"),
        Vec::<(usize, usize)>::new(),
        "无数字字段的段整体丢弃"
    );
    assert_eq!(
        parse_boundaries("2,;"),
        Vec::<(usize, usize)>::new(),
        "缺第二个数字的段丢弃"
    );
    assert_eq!(
        parse_boundaries("2,3"),
        Vec::<(usize, usize)>::new(),
        "缺分号终止符的段丢弃"
    );
    assert_eq!(
        parse_boundaries("x2,3;"),
        vec![(2, 3)],
        "前缀噪声靠右移重试找回该段"
    );
    // gmatch 语义：失败起点右移重试
    assert_eq!(
        parse_boundaries("12,34,56;"),
        vec![(34, 56)],
        "同段内多组数字时取最后一组"
    );
    assert_eq!(
        parse_boundaries("1,2,3;"),
        vec![(2, 3)],
        "三段数字同样取最后一组"
    );
}
