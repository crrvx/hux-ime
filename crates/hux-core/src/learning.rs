// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tab 纠错学习，对应参照 `lua/tiger_sentence_learning.lua` 的纯计算部分。
//!
//! 实现：`build`（全量重放 oracle）、`runtime`/`update`（运行时快照）、
//! `score`/`prefix_score`（含物化缓存）、`reward`（路径链）、`diff`、
//! `context`/`static_text`/`frame`/`unframe`/`hash`。
//! 持久化（LevelDB `open`/`confirm`）在平台层实现：见 `platform/fcitx5/src/learning_store.rs`。
//!
//! **人工纠错等级**：事件只累加**离散等级**（每次确认 +1，上限 10），
//! 分数按等级取整（same-context `7+2L`、跨上下文 `4+2L`）；**不按时间衰减**，
//! 时间戳只作持久化元数据。故浮点求和只发生在 `weight`（各上下文的整数等级之和）
//! 累加上，跨进程哈希序不改变结果。
//!
//! 结构：本模块保留模块文档与公开面（`pub use` 重导出）；实现按职责分到子模块：
//! `text`（文本与帧工具）、`hash`（哈希与融合键）、`model`（数据模型与常量）、
//! `score`（评分换算）、`index`（索引本体，`index/query` 为查询）、`reward`（奖励链）、
//! `diff`（路径链差分）；单测就近放在本模块。

mod diff;
mod hash;
mod index;
mod model;
mod reward;
mod score;
mod text;

pub use diff::diff;
pub use hash::{fusion_event, fusion_mode, fusion_pair_code, hash, hash_bytes, hash_parts};
pub use model::LearningIndex;
pub use model::{DiffEvent, DiffItem, DiffPathNode, Event, RewardNode};
pub use reward::{early_commit_contribution, early_commit_maturity, reward};
pub use text::{character_count, chars, context, frame, static_text, unframe};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_matches_reference_vectors() {
        // 取自参照实现（也是线上学习库名后缀的来源）。
        assert_eq!(hash(""), "811c9dc500001505");
        assert_eq!(hash("tiger_sentence"), "f2d1c028532c0d94");
        assert_eq!(hash("虎句"), "2b025b23302e3acd");
        assert_eq!(hash_bytes(b""), hash(""));
    }

    /// 分段累加与拼接后一次哈希同值（`hash_parts` 的契约；分段处也得对上）。
    #[test]
    fn hash_parts_matches_concatenation() {
        assert_eq!(hash_parts(&[]), hash(""));
        assert_eq!(hash_parts(&[""]), hash(""));
        assert_eq!(hash_parts(&["ab", "c"]), hash("abc"));
        assert_eq!(hash_parts(&["", "虎句"]), hash("虎句"));
        assert_eq!(
            hash_parts(&["\u{feff}甲\ta\n", "\0", "乙\n"]),
            hash("\u{feff}甲\ta\n\0乙\n")
        );
    }

    #[test]
    fn fusion_keys_match_reference_vectors() {
        // 向量取自参照 `lua/tiger_sentence_learning.lua` @ d7b01e5 直算
        // （`hash`/`fusion_*` 在 bd83900..d7b01e5 区间内未改动，与 b2bbd23 逐字节相同）。
        assert_eq!(fusion_mode(""), "");
        assert_eq!(fusion_mode("m"), "fusion-v1|m");
        assert_eq!(
            fusion_pair_code(b"ii", "C", "A"),
            "~f422122101c436982",
            "`raw \"\\0D\\0\" direct \"\\0C\\0\" composed` 的哈希 + `~f` 前缀"
        );
        assert_eq!(
            fusion_pair_code(b"", "", ""),
            "~f4576f3060cad8fac",
            "空组件按 `a or \"\"` 处理"
        );
        assert_eq!(
            fusion_pair_code(b"ab", "疒否", "交否"),
            "~f39bc299c6ab38db6"
        );
        // 融合码在 `codes` 中排在全部 raw 码之后（`~` > `z`）：命名空间隔离的前提。
        assert!("~f422122101c436982" > "zzzz");
    }

    #[test]
    fn fusion_score_is_pairwise_difference() {
        // 提交点把 `DiffEvent` 投影成落库的 `Event`（丢弃只用于筛选的偏移）。
        let persisted = |direct: &str| {
            let event = fusion_event("m", b"ii", direct, "A", true, 2, 1000.0).expect("事件");
            Event {
                time: event.time,
                mode: event.mode,
                code: event.code,
                text: event.text,
                context: event.context,
            }
        };
        let events = vec![persisted("C"), persisted("B")];
        let mut index = LearningIndex::build(&events, 1000.0);
        // 单笔确认权重 1 → 9；未记录的 pair 恒 0。
        assert_eq!(index.fusion_score("m", b"ii", "C", "A"), 9.0);
        assert_eq!(index.fusion_score("m", b"ii", "B", "A"), 9.0);
        assert_eq!(index.fusion_score("m", b"ii", "A", "C"), 0.0);
        assert_eq!(index.fusion_score("m", b"ii", "Z", "A"), 0.0);
        // 空模式 = 不学习。
        assert_eq!(index.fusion_score("", b"ii", "C", "A"), 0.0);
    }

    #[test]
    fn fusion_event_encodes_direction_and_offsets() {
        assert!(fusion_event("", b"ii", "C", "A", true, 2, 0.0).is_none());
        let event = fusion_event("m", b"ii", "C", "A", true, 2, 1700000000.0).expect("事件");
        assert_eq!(event.time, 1700000000.0);
        assert_eq!(event.mode, "fusion-v1|m");
        assert_eq!(event.code, "~f422122101c436982");
        assert_eq!(event.text, "D");
        assert_eq!(event.context, "");
        assert_eq!(
            (
                event.raw_start,
                event.raw_end,
                event.text_start,
                event.text_end
            ),
            (0, 2, 0, 1)
        );
        let reversed = fusion_event("m", b"ii", "C", "A", false, 2, 0.0).expect("事件");
        assert_eq!(reversed.text, "C");
    }

    #[test]
    fn context_keeps_last_two_characters() {
        assert_eq!(context("甲"), "甲");
        assert_eq!(context("甲乙"), "甲乙");
        assert_eq!(context("甲乙丙"), "乙丙");
        assert_eq!(context(""), "");
    }

    #[test]
    fn chars_validates_utf8_tags() {
        assert!(chars("甲乙").is_some());
        assert!(chars("\u{fffd}").is_some());
    }

    #[test]
    fn static_text_constrains_tags() {
        assert!(!static_text(""));
        assert!(static_text("甲"));
        assert!(!static_text(&"甲".repeat(17)));
        assert!(!static_text("甲{乙"));
        assert!(!static_text("\u{e000}")); // 私用区（参照排除）
    }

    #[test]
    fn frame_roundtrip() {
        let values = vec!["1".to_string(), "ab".to_string(), String::new()];
        assert_eq!(unframe(&frame(&values)), Some(values));
    }

    #[test]
    fn early_commit_maturity_maps_correction_levels() {
        // 取自参照测试：L1/L2/L3 次同上下文纠错 → 0 / 0.5 / 1。
        assert_eq!(early_commit_maturity(9.0), 0.0);
        assert_eq!(early_commit_maturity(8.0), 0.0);
        assert_eq!(early_commit_maturity(11.0), 0.5);
        assert_eq!(early_commit_maturity(13.0), 1.0);
        assert_eq!(early_commit_maturity(100.0), 1.0);
    }

    #[test]
    fn early_commit_contribution_is_bounded() {
        let close = |left: f64, right: f64| (left - right).abs() < 1e-12;
        assert_eq!(early_commit_contribution(0.0), 0.0);
        assert_eq!(early_commit_contribution(-5.0), 0.0);
        assert_eq!(early_commit_contribution(9.0), 0.0);
        // 未成熟的观测只给部分贡献；成熟后 saturate 到上限 0.75。
        assert!(close(early_commit_contribution(10.0), 0.187_5));
        // 等级分 12（成熟度 0.75）尚未到上限；13（成熟度 1）才封顶。
        assert!(close(early_commit_contribution(12.0), 0.675));
        assert_eq!(early_commit_contribution(13.0), 0.75);
        assert_eq!(early_commit_contribution(20.0), 0.75);
    }

    /// 参照 `tools/test_sentence_learning.lua` @ d7b01e5 的等级语义断言。
    #[test]
    fn correction_levels_advance_two_points_and_cap_at_ten() {
        let event = |code: &str, text: &str, context: &str| Event {
            time: 1000.0,
            mode: "test".to_string(),
            code: code.to_string(),
            text: text.to_string(),
            context: context.to_string(),
        };
        // 单次确认 = 同上下文 L1 = 9；跨上下文 L1 = 6；模式隔离。
        let mut single = LearningIndex::build(&[event("ab", "疒", "")], 1000.0);
        assert_eq!(single.score("test", "ab", "疒", ""), 9.0);
        assert_eq!(single.score("test", "ab", "疒", "甲"), 6.0);
        assert_eq!(single.score("other", "ab", "疒", ""), 0.0);
        // 无时间衰减：时间推后 10 年分值不变。
        let mut aged = LearningIndex::build(&[event("ab", "疒", "")], 1000.0 + 3650.0 * 86400.0);
        assert_eq!(aged.score("test", "ab", "疒", ""), 9.0);
        // 每次确认 +1 级、等级分 +2，第 10 级封顶（同上下文 27 / 跨上下文 24）。
        let mut repeated = Vec::new();
        for i in 1..=40 {
            repeated.push(event("ab", "疒", ""));
            let level = 10.0f64.min(i as f64);
            let mut index = LearningIndex::build(&repeated, 1000.0);
            assert_eq!(
                index.score("test", "ab", "疒", ""),
                7.0 + 2.0 * level,
                "repeated corrections at {i}"
            );
            assert_eq!(
                index.score("test", "ab", "疒", "其他"),
                4.0 + 2.0 * level,
                "cross-context score at {i}"
            );
        }
        let mut capped = LearningIndex::build(&repeated, 1000.0);
        assert_eq!(capped.score("test", "ab", "疒", "其他"), 24.0);
        // 三次确认跨上下文达 L3 = 10；手工竞争纠错把旧选择等级归零。
        let mut competing = vec![
            event("ab", "甲乙", "前"),
            event("ab", "甲乙", "后"),
            event("ab", "甲乙", "后"),
        ];
        let mut index = LearningIndex::build(&competing, 1000.0);
        assert_eq!(index.score("test", "ab", "甲乙", "新"), 10.0);
        competing.push(event("ab", "甲丙", "后"));
        let mut demoted = LearningIndex::build(&competing, 1000.0);
        assert_eq!(demoted.score("test", "ab", "甲乙", "后"), 6.0);
    }

    #[test]
    fn unframe_rejects_bad_input() {
        assert_eq!(unframe("5:abc"), None);
        assert_eq!(unframe("8193:ab"), None);
        assert_eq!(unframe("ab"), None);
    }

    /// 长度前缀合法但落点**不在字符边界**：`&str` 的字节切片会 panic（`Option` 签名承诺不 panic）。
    ///
    /// 生产触发面：`platform/fcitx5/src/learning_store.rs` 把 LevelDB 的任意值经
    /// `String::from_utf8_lossy` 交给本函数（非法 UTF-8 换成 U+FFFD 后长度错位），
    /// 且发生在 `hux_engine_new`（`extern "C"`）⇒ 坏库会让 addon 加载即 abort。
    #[test]
    fn unframe_rejects_non_char_boundary_slices() {
        // "1:é"：`é` 占 2 字节，长度 1 的切片正好落在其内部。
        assert_eq!(unframe("1:é"), None);
        // 尾段落点不在边界（前段良构）：先切出 "1:a"，再对 `é` 切 1 字节。
        assert_eq!(unframe("3:1:a1:é"), None);
        // lossy 替换后的形态（非法 UTF-8 → U+FFFD，3 字节）同样只是坏帧，不 panic。
        let lossy = String::from_utf8_lossy(b"2:\xff\xfe").to_string();
        assert_eq!(unframe(&lossy), None);
        // 良构帧不受影响（含多字节字符的正常切分）。
        assert_eq!(
            unframe("1:a2:é"),
            Some(vec!["a".to_string(), "é".to_string()])
        );
    }
}
