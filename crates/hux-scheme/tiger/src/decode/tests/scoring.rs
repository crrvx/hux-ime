// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! 打分与配置用例：路径摘要、排序先验、补充模型奖励、无模型分值与词法模型往返。

use super::*;

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
