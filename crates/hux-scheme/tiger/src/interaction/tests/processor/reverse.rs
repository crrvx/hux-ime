// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! 音反查段：触发键（多项 `KeyList`）、数字直选的绝对索引与越界惰性消费，
//! 以及段内音节分隔符（撇号）的保留与去重。

use super::*;

/// 音反查段的数字直选按**上游绝对索引**（`digit - 1`）落点，与主菜单的 addon
/// 页相对口径（`page_start + position`）分开：菜单停在第 2 页起时两者结果不同。
///
/// 参照 `lua/tiger_sentence.lua` @ `92a0b54`：反查段的数字分支
/// `local index = tonumber(ch) - 1`（无分页概念，越界惰性消费）。
/// 负向对照：把该分支挪回 addon 数字直选**之后** ⇒ 本用例提交 `候11` 而非 `候1`。
#[test]
fn processor_reverse_lookup_digit_uses_absolute_index_across_pages() {
    let mut h = Harness::new();
    h.context.set_option(OPTION_DIGIT_SELECT, true);
    h.context.set_option("_auto_commit", true);
    let texts: Vec<String> = (0..12).map(|index| format!("候{index}")).collect();
    let refs: Vec<&str> = texts.iter().map(String::as_str).collect();
    h.push_tagged_segment(
        b"`z",
        &refs,
        &[crate::sound_to_char_shape::SOUND_TO_CHAR_SHAPE_TAG],
    );
    // 停在第 3 页（页大小 5 ⇒ `page_start` 10）：绝对索引 1 = `候1`，
    // 页相对口径则是 `候11`。
    h.context.highlight(10);
    assert_eq!(h.context.composition.back().unwrap().selected_index, 10);
    assert!(h.context.has_menu());
    assert_eq!(h.press("2"), ProcessorResult::Consume);
    assert_eq!(h.context.last_commit_text(), "候1");
    assert!(h.context.input().is_empty());
}

/// 音反查段的数字越界（`index >= count`）惰性消费：不改高亮、不提交。
#[test]
fn processor_reverse_lookup_digit_out_of_range_is_inert() {
    let mut h = Harness::new();
    h.context.set_option(OPTION_DIGIT_SELECT, true);
    h.context.set_option("_auto_commit", true);
    h.push_tagged_segment(
        b"`z",
        &["中", "重"],
        &[crate::sound_to_char_shape::SOUND_TO_CHAR_SHAPE_TAG],
    );
    assert_eq!(h.press("9"), ProcessorResult::Consume);
    assert_eq!(h.context.last_commit_text(), "");
    assert_eq!(h.context.input(), b"`z");
    assert_eq!(h.context.composition.back().unwrap().selected_index, 0);
}

/// 多项触发键（`KeyList`）：任一配置键都可进入音反查，入段字符取命中键的字符。
#[test]
fn processor_sound_to_char_shape_accepts_multiple_triggers() {
    let mut h = Harness::new();
    h.context
        .set_property(K_SOUND_TO_CHAR_SHAPE_KEY, "grave,semicolon");
    assert_eq!(h.press("semicolon"), ProcessorResult::Consume);
    assert_eq!(h.context.input(), b";");
    h.context.clear();
    assert_eq!(h.press("grave"), ProcessorResult::Consume);
    assert_eq!(h.context.input(), b"`");
}

/// 音反查索引夹具（`goldens/sound_to_char_shape/`：小 PY_c + `tiger_sentence.pinyin.bin`）——
/// 与差分层 `key_sequence_differential` 的音反查重放同源（该目录即方案数据目录，含
/// `tiger_sentence.codes.txt`）。
fn reverse_lookup_fixture() -> Decoder {
    let dir = hux_test_support::repo_path("goldens/sound_to_char_shape");
    let lexicon = Lexicon::load(std::slice::from_ref(&dir), 0);
    let supplement = crate::lexicon::Supplement::load_default(Some(&dir));
    Decoder::new(lexicon, supplement, None)
}

/// 反查夹具 + 反查前缀属性（`` ` `` = grave）；`reprs` 全部敲完（逐步断言按键被消费）。
fn reverse_lookup_harness(reprs: &[&str]) -> FusionHarness {
    let mut harness = FusionHarness::new(reverse_lookup_fixture());
    harness
        .context
        .set_property(K_SOUND_TO_CHAR_SHAPE_KEY, "grave");
    for repr in reprs {
        assert_eq!(harness.press(repr), ProcessorResult::Consume, "{repr}");
    }
    harness
}

/// 反查段里文本为 `text` 的候选预编辑。
fn candidate_preedit<'a>(harness: &'a FusionHarness, text: &str) -> &'a str {
    harness
        .context
        .composition
        .back()
        .expect("反查段")
        .candidates
        .iter()
        .find(|candidate| candidate.text == text)
        .unwrap_or_else(|| panic!("反查段应有候选 {text:?}"))
        .preedit
        .as_str()
}

/// 反查段首候选的预编辑。
fn first_candidate_preedit(harness: &FusionHarness) -> &str {
    harness
        .context
        .composition
        .back()
        .expect("反查段")
        .candidates
        .first()
        .expect("反查段应有候选")
        .preedit
        .as_str()
}

/// 音反查段内的音节分隔符（撇号）经**真实链路**（`processor` → `CompositionBuilder::rebuild`）
/// 留在段内、不切段：`` `zh'guo `` 的候选是「中国」、预编辑 `` `zh'guo ``（撇号原样保留），
/// 上屏提交「中国」；不含撇号的 `` `zhguo `` 候选相同，但缩写 `zh` 与后续音节合并成
/// `` `zhguo ``（对照见下）。撇号在 abc 段一侧是 `SEGMENTATION_DELIMITER`（切分），在反查段
/// 一侧是**音节分隔符**（透明跳过 + 强制断音，见 `sound_to_char_shape`）⇒ 本用例钉住反查段
/// 整体覆盖到输入末尾，中途不得断段或结束段。
#[test]
fn processor_reverse_lookup_keeps_syllable_delimiter_inside_segment() {
    let steps: [(&str, &[u8]); 7] = [
        ("grave", b"`"),
        ("z", b"`z"),
        ("h", b"`zh"),
        ("apostrophe", b"`zh'"),
        ("g", b"`zh'g"),
        ("u", b"`zh'gu"),
        ("o", b"`zh'guo"),
    ];
    let mut harness = FusionHarness::new(reverse_lookup_fixture());
    harness
        .context
        .set_property(K_SOUND_TO_CHAR_SHAPE_KEY, "grave");
    for (repr, expected) in steps {
        assert_eq!(harness.press(repr), ProcessorResult::Consume, "{repr}");
        assert_eq!(harness.context.input(), expected, "{repr}");
        let segment = harness.context.composition.back().expect("反查段");
        // 每一步（含撇号那一步）都仍是**同一个**反查段，且整段覆盖到输入末尾。
        assert_eq!(segment.start, 0, "{repr}");
        assert_eq!(segment.end, expected.len(), "{repr}");
        assert!(
            segment.has_tag(crate::sound_to_char_shape::SOUND_TO_CHAR_SHAPE_TAG),
            "{repr}"
        );
    }
    assert_eq!(candidate_preedit(&harness, "中国"), "`zh'guo");
    assert_eq!(harness.press("space"), ProcessorResult::Consume);
    assert_eq!(harness.context.last_commit_text(), "中国");

    // 对照：不含撇号的既有路径不变 —— 候选同为「中国」，但缩写 `zh` 与后续音节**合并**成
    // 预编辑 `` `zhguo ``（只有分隔符才强制插空格，正是撇号那一步的差异）；同样上屏「中国」。
    let mut plain = reverse_lookup_harness(&["grave", "z", "h", "g", "u", "o"]);
    assert_eq!(candidate_preedit(&plain, "中国"), "`zhguo");
    assert_eq!(plain.press("space"), ProcessorResult::Consume);
    assert_eq!(plain.context.last_commit_text(), "中国");

    // 段尾的分隔符当场可见（候选预编辑原样保留撇号，不必等后续音节）。
    let trailing = reverse_lookup_harness(&["grave", "z", "h", "apostrophe"]);
    assert_eq!(first_candidate_preedit(&trailing), "`zh'");
}

/// 音反查段内**连续**的音节分隔符只保留第一个：多余的丢弃、不录入——输入串不被改写，
/// 段尾也不前进，候选与预编辑保持与单个撇号完全一致。
#[test]
fn processor_reverse_lookup_drops_consecutive_syllable_delimiters() {
    let mut harness = reverse_lookup_harness(&["grave", "z", "h", "apostrophe"]);
    for _ in 0..2 {
        assert_eq!(harness.press("apostrophe"), ProcessorResult::Consume);
        assert_eq!(harness.context.input(), &b"`zh'"[..]);
        let segment = harness.context.composition.back().expect("反查段");
        assert_eq!(segment.end, 4, "连续撇号不得留在段内");
    }
    assert_eq!(first_candidate_preedit(&harness), "`zh'");
    assert_eq!(harness.press("space"), ProcessorResult::Consume);
    assert_eq!(harness.context.last_commit_text(), "中");
}
