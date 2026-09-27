// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! 处理器主循环、确认选择与反查（`interaction/processor.rs`、`interaction/select.rs`）的用例。

use super::*;

struct Harness {
    decoder: Decoder,
    context: Context,
    state: SentenceState,
    live: LiveLearning,
    dot_armed: bool,
    page_size: usize,
}

impl Harness {
    fn new() -> Self {
        Self {
            decoder: lexicon_fixture(),
            context: Context::new(),
            state: SentenceState::fresh(1),
            live: LiveLearning::default(),
            dot_armed: false,
            page_size: 5,
        }
    }

    fn press_event(&mut self, key: &KeyEvent) -> ProcessorResult {
        let host_options = HostOptions::default();
        let mut env = ProcessorEnv {
            now: 0.0,
            dot_armed: &mut self.dot_armed,
            min_retained: 0,
            page_size: self.page_size,
            host_options: &host_options,
        };
        process_key_event(
            key,
            &mut self.context,
            &mut self.state,
            &mut self.decoder,
            &mut self.live,
            &mut env,
        )
        .expect("processor")
    }

    fn press(&mut self, repr: &str) -> ProcessorResult {
        let key = key_of(repr);
        self.press_event(&key)
    }

    fn push_segment(&mut self, input: &[u8], texts: &[&str]) {
        self.push_tagged_segment(input, texts, &[]);
    }

    /// 带标签的段（音反查段等）：`push_segment` 建的是主候选段（无标签）。
    fn push_tagged_segment(&mut self, input: &[u8], texts: &[&str], tags: &[&str]) {
        self.context.set_input(input);
        let candidates = texts
            .iter()
            .map(|text| Candidate::new("sentence", 0, input.len(), text, ""))
            .collect();
        self.context.composition.segments.push(Segment {
            start: 0,
            end: input.len(),
            tags: tags.iter().map(|tag| (*tag).to_string()).collect(),
            prompt: String::new(),
            selected_index: 0,
            candidates,
            selected: false,
            translated: true,
        });
    }
}

#[test]
fn processor_forwards_release_and_idle_punct() {
    let mut h = Harness::new();
    // 释放事件交宿主
    let release = KeyEvent::new(
        hux_core::key::keycode_by_name("a").expect("a"),
        hux_core::key::K_RELEASE_MASK,
    );
    assert_eq!(h.press_event(&release), ProcessorResult::Forward);
    // 空闲分号/引号交标点处理器
    assert_eq!(h.press("semicolon"), ProcessorResult::Forward);
    assert_eq!(h.press("apostrophe"), ProcessorResult::Forward);
}

#[test]
fn processor_commits_idle_digit_and_arms_dot() {
    let mut h = Harness::new();
    // 空闲数字直接上屏并置待发
    assert_eq!(h.press("5"), ProcessorResult::Consume);
    assert_eq!(h.context.last_commit_text(), "5");
    assert!(h.dot_armed);
    // 紧随的句点按 ASCII 小数点上屏
    assert_eq!(h.press("period"), ProcessorResult::Consume);
    assert_eq!(h.context.last_commit_text(), ".");
    assert!(!h.dot_armed);
    // 无待发状态时句点交宿主
    assert_eq!(h.press("period"), ProcessorResult::Forward);
}

#[test]
fn processor_return_commits_buffer_and_input() {
    let mut h = Harness::new();
    assert_eq!(h.press("a"), ProcessorResult::Consume);
    assert_eq!(h.press("b"), ProcessorResult::Consume);
    assert_eq!(h.context.input(), b"ab");
    // 真实会话中组合由 translator 建立；这里手工合成后再走提交/清空分支。
    h.push_segment(b"ab", &["交"]);
    // Return：提交「缓冲 + 实时输入」并清空
    assert_eq!(h.press("Return"), ProcessorResult::Consume);
    assert_eq!(h.context.last_commit_text(), "ab");
    assert!(h.context.input().is_empty());
}

#[test]
fn processor_escape_clears_composition() {
    let mut h = Harness::new();
    h.push_segment(b"a", &["甲"]);
    // Escape：直接清空
    assert_eq!(h.press("Escape"), ProcessorResult::Consume);
    assert!(h.context.input().is_empty());
    assert!(h.state.committed_raw.is_empty());
}

/// 回归（既有缺陷）：Tab 锁确认路径必须**在清 `state.tab_pending` 之前** stage 学习事件。
///
/// 参照 `processor` 的 Tab 确认分支顺序是
/// `learning_stage(env, state, selected, full_before)` → `state.tab_pending = false`；
/// `learning_stage` 用该标志选基线（`tab_pending and live.baseline or submitted_first`），
/// 且稳定确认（`reinforce`）路线要求 `!tab_pending`。本仓曾只用分支末尾的提交点补 stage，
/// 于是基线取成 `submitted_first` 并可能误走 reinforce——金样覆盖不到（探针
/// `store_ready == false` 使该路径短路），故这里经 `processor` 走端到端。
#[test]
fn processor_tab_confirm_stages_against_live_baseline() {
    let mode = "sentence-v2|rules=|optimal=1500|dup=1";
    let mut h = Harness::new();
    h.live.mode = mode.to_string();
    h.live.store_ready = true;
    for repr in ["a", "b", "a", "b"] {
        assert_eq!(h.press(repr), ProcessorResult::Consume);
    }
    assert_eq!(h.context.input(), b"abab");
    // 真实会话里菜单由 translator 建立（交交 / 交疒 = 两条 2 码边，均为 composed-only）。
    h.push_segment(b"abab", &["交交", "交疒"]);
    // Tab：写基线（本菜单首个可见候选）并把高亮移到下一项
    assert_eq!(h.press("Tab"), ProcessorResult::Consume);
    assert!(h.state.tab_pending);
    let baseline = h.live.baseline.clone().expect("Tab 按下时应写入基线");
    assert_eq!(baseline.text, "交交");
    // 高亮已在第 2 项：此刻按同一解码取到的 selected 就是确认分支将选中的候选。
    let selection = learning_selection(&mut h.decoder, &h.context, &h.state).expect("selection");
    let selected = selection.selected.clone().expect("第 2 个候选");
    assert_eq!(selected.text, "交疒");
    let expected = learning::diff(
        b"abab",
        Some(&baseline.diff),
        Some(&selected.diff),
        0,
        mode,
        0.0,
    );
    assert_eq!(expected.len(), 1, "夹具前提：交交 / 交疒 仅末段不同");
    // 字母确认走 Tab 确认分支（候选 raw 长度超过已确认前缀）
    assert_eq!(h.press("c"), ProcessorResult::Consume);
    assert!(!h.state.tab_pending);
    assert!(
        h.live.baseline.is_none(),
        "stage 必须已按 tab_pending 分支消费基线"
    );
    // 早提交选项关闭 ⇒ 不触发提交点通知器：pending 只能来自 Tab 分支里的 stage。
    assert_eq!(h.live.pending.len(), expected.len());
    for (got, want) in h.live.pending.iter().zip(expected.iter()) {
        assert_eq!(got, want);
    }
    // 融合事件不参与：DiffEvent 的模式仍是实时模式，而非 `fusion-v1|…`。
    assert_eq!(h.live.pending[0].mode, mode);
}

/// 参照 `abad411`：**菜单可见**（不要求缓冲态）时遇可打印 ASCII 标点，必须先按当前选中项
/// stage 学习、确认组合（`_auto_commit` 下即上屏），再把原键交标点表。
///
/// 参照依据（提交信息）：标点段一旦被 punctuator 追加进组合，`learning_selection` 就再也
/// 解不出该输入（如 `zhhbi,`）或取不回句子的选中项；故判据由 `state.buffered_text ~= ""`
/// 改为 `context:has_menu()`。金样覆盖不到该学习路径（探针 `store_ready == false` 短路），
/// 故这里经 `processor` 端到端钉住：本用例的 `buffered_text` 为空，旧判据**不**成立。
#[test]
fn processor_menu_punctuation_stages_learning_before_the_punctuator() {
    let mode = "sentence-v2|rules=|optimal=1500|dup=1";
    let mut h = Harness::new();
    h.context.set_option("_auto_commit", true);
    h.live.mode = mode.to_string();
    h.live.store_ready = true;
    for repr in ["a", "b", "a", "b"] {
        assert_eq!(h.press(repr), ProcessorResult::Consume);
    }
    // 真实会话里菜单由 translator 建立（交交 / 交疒 = 两条 2 码边，均为 composed-only）。
    h.push_segment(b"abab", &["交交", "交疒"]);
    assert!(h.state.buffered_text.is_empty(), "旧判据在此不成立");
    h.context.highlight(1); // 人工纠错：选中第二项
    let selection = learning_selection(&mut h.decoder, &h.context, &h.state).expect("selection");
    let selected = selection.selected.clone().expect("第 2 个候选");
    let first = selection.first.clone().expect("首个候选");
    let expected = learning::diff(
        b"abab",
        Some(&first.diff),
        Some(&selected.diff),
        0,
        mode,
        0.0,
    );
    assert_eq!(expected.len(), 1, "夹具前提：交交 / 交疒 仅末段不同");
    // 逗号：确认组合（上屏「交疒」）后原键仍交标点表。
    assert_eq!(h.press("comma"), ProcessorResult::Forward);
    assert!(h.context.input().is_empty());
    assert_eq!(h.context.last_commit_text(), "交疒");
    assert!(h.live.pending.is_empty(), "提交点已消费 pending");
    assert_eq!(
        h.live.submitted.len(),
        expected.len(),
        "标点路径的纠错必须落学习"
    );
    for (got, want) in h.live.submitted.iter().zip(expected.iter()) {
        assert_eq!(got.time, want.time);
        assert_eq!(got.mode, want.mode);
        assert_eq!(got.code, want.code);
        assert_eq!(got.text, want.text);
        assert_eq!(got.context, want.context);
    }
    assert_eq!(h.live.submitted[0].mode, mode);
}

/// **本仓有意偏离上游 `abad411`**（用户决定 B：菜单可见时翻页键一律拦截）：
/// 菜单可见时，凡会落入宿主翻页绑定的标点键都不由标点分支消费。
///
/// 上游对「菜单可见 + 可打印 ASCII 标点」一律「暂存学习 + 确认组合 + 交标点表」，于是
/// `-/=`（以及 schema 绑到翻页的 `[/]`）的 key_binder 绑定被永久遮蔽（最小复现 `j a equal`）。
/// 本仓在分支入口先问宿主同一套判据 `hux_core::host::paging_action`：
/// `=`（下翻）与 `-`（上翻，**不再要求「已翻过页」**）都让给宿主翻页，
/// 不确认组合、不暂存学习；`ascii_mode` 打开时判据不成立 ⇒ 回到上游路径。
/// 代价：菜单可见时 `-`/`=`/`[`/`]` 不能作为标点打出；相关上游金样用例按 `DEVIATIONS` 登记。
#[test]
fn processor_menu_paging_keys_bypass_the_punctuation_branch() {
    let mut h = Harness::new();
    h.context.set_option("_auto_commit", true);
    h.live.store_ready = true;
    h.push_segment(b"ab", &["交", "疒"]);
    assert!(h.context.has_menu());
    h.context.highlight(1); // 人工纠错候选：若走上游标点分支会立刻上屏「疒」
    assert_eq!(h.context.last_commit_text(), "");

    // `=`：`when: has_menu` 成立 ⇒ 让给宿主下翻页（不消费、不确认组合、不 stage 学习）。
    assert_eq!(h.press("equal"), ProcessorResult::Forward);
    assert_eq!(h.context.last_commit_text(), "", "`=` 不得确认组合");
    assert_eq!(h.context.input(), b"ab", "`=` 后组合原样保留（交宿主翻页）");
    assert!(h.context.has_menu(), "`=` 后菜单仍在（交宿主翻页）");

    // `-`：菜单可见即判上翻页（新语义，不再看 `paging` 标签）⇒ 同样让给宿主。
    assert_eq!(h.press("minus"), ProcessorResult::Forward);
    assert_eq!(
        h.context.last_commit_text(),
        "",
        "菜单可见时 `-` 不得确认组合"
    );
    assert_eq!(h.context.input(), b"ab", "`-` 后组合原样保留（交宿主翻页）");
    assert!(h.context.has_menu(), "`-` 后菜单仍在（交宿主翻页）");
    assert!(
        h.live.submitted.is_empty(),
        "让给宿主的键不暂存学习（否则标点路径会记一次人工纠错）"
    );

    // 判据的另一半：`ascii_mode` 打开时翻页键**不**拦截 ⇒ `-` 回到上游标点路径
    // （确认组合后交标点表；本用例不跑宿主链，故只断言处理器的确认行为）。
    let mut ascii = Harness::new();
    ascii.context.set_option("_auto_commit", true);
    ascii.context.set_option("ascii_mode", true);
    ascii.live.store_ready = true;
    ascii.push_segment(b"ab", &["交", "疒"]);
    ascii.context.highlight(1);
    assert_eq!(ascii.press("minus"), ProcessorResult::Forward);
    assert_eq!(
        ascii.context.last_commit_text(),
        "疒",
        "`ascii_mode` 下 `-` 退回上游标点路径（确认组合）"
    );
    assert!(ascii.context.input().is_empty());
}

fn segment_with_candidate(candidate: Candidate) -> Segment {
    Segment {
        start: 0,
        end: 2,
        tags: Vec::new(),
        prompt: String::new(),
        selected_index: 0,
        candidates: vec![candidate],
        selected: false,
        translated: true,
    }
}

#[test]
fn confirm_selection_honors_auto_commit() {
    let mut context = Context::new();
    context.set_input(b"ab");
    context
        .composition
        .segments
        .push(segment_with_candidate(Candidate::new(
            "sentence", 0, 2, "甲", "",
        )));
    // `_auto_commit` 关闭：只标记选中，不提交（对应 librime 的 Forward 分支）
    confirm_selection(None, &mut context, &mut SentenceState::fresh(1));
    assert!(context.composition.back().unwrap().selected);
    assert_eq!(context.input(), b"ab");
    // 打开后：确认即提交
    context.set_option("_auto_commit", true);
    confirm_selection(None, &mut context, &mut SentenceState::fresh(1));
    assert_eq!(context.last_commit_text(), "甲");
    assert!(context.input().is_empty());
}

#[test]
fn confirm_selection_merges_buffered_prefix() {
    let mut context = Context::new();
    context.set_option("_auto_commit", true);
    hux_core::session::set_property_if_changed(&mut context, K_BUFFERED, "乙");
    context.set_input(b"~c");
    context
        .composition
        .segments
        .push(segment_with_candidate(Candidate::new(
            "sentence_buffered",
            0,
            2,
            "c",
            "",
        )));
    // 缓冲候选：提交前并入缓冲前缀
    confirm_selection(None, &mut context, &mut SentenceState::fresh(1));
    assert_eq!(context.last_commit_text(), "乙c");
}

#[test]
fn processor_inserts_at_caret() {
    let mut h = Harness::new();
    h.context.set_input(b"ab");
    h.context.set_caret(1);
    assert_eq!(h.press("c"), ProcessorResult::Consume);
    assert_eq!(h.context.input(), b"acb");
    assert_eq!(h.context.caret(), 2);
}

#[test]
fn processor_guards_menu_navigation_while_buffered() {
    let mut h = Harness::new();
    // 缓冲空闲：菜单导航键拦给宿主
    h.state.buffered_text = "交".to_string();
    assert_eq!(h.press("Tab"), ProcessorResult::Consume);
    assert_eq!(h.press("Up"), ProcessorResult::Consume);
}

#[test]
fn processor_forwards_navigation_without_menu() {
    let mut h = Harness::new();
    // 无缓冲：Up 交宿主；Tab 无菜单可用时同样交宿主
    assert_eq!(h.press("Up"), ProcessorResult::Forward);
    assert_eq!(h.press("Tab"), ProcessorResult::Forward);
}

#[test]
fn processor_space_confirms_candidate() {
    let mut h = Harness::new();
    h.push_segment(b"ab", &["交"]);
    assert_eq!(h.press("space"), ProcessorResult::Consume);
    assert!(h.state.committed_raw.is_empty());
}

/// 数字直选（`DigitSelect`）：菜单可见时按页位置直接上屏（1–9；0=10）。
#[test]
fn processor_digit_select_commits_page_candidate() {
    let mut h = Harness::new();
    h.context.set_option(OPTION_DIGIT_SELECT, true);
    h.context.set_option("_auto_commit", true);
    h.push_segment(b"ab", &["交", "疒"]);
    assert!(h.context.has_menu());
    assert_eq!(h.press("2"), ProcessorResult::Consume);
    assert_eq!(h.context.last_commit_text(), "疒");
    assert!(h.context.input().is_empty());
}

/// 数字直选默认关：数字仍作为编码字符（选重后缀）。
///
/// 跨层分工：平台侧 `platform/fcitx5/src/tests.rs` 的
/// `digit_select_off_keeps_rank_suffix` 负责引擎可见结果（按键被消费、候选列表为空、
/// 输入尾部是 `2`）；本用例只补充内核独有的结构：数字进的是**编码路径**，
/// 候选段（候选、高亮、区间、确认位）逐字段不变。
#[test]
fn processor_digit_select_off_keeps_rank_suffix() {
    let mut h = Harness::new();
    h.context.set_option("_auto_commit", true);
    h.push_segment(b"ab", &["交", "疒"]);
    let before = h.context.composition.back().expect("段存在").clone();
    assert_eq!(h.press("2"), ProcessorResult::Consume);
    let after = h.context.composition.back().expect("段仍存在");
    // 编码后缀：进入原始输入并把光标推后。
    assert_eq!(h.context.input(), b"ab2");
    assert_eq!(h.context.caret(), 3);
    assert_eq!(h.context.last_commit_text(), "");
    // 结构不变：digit_select 关掉时不会走 `select_candidate_at` ⇒ 段未被确认、高亮未动。
    assert!(!after.selected, "编码分支不得确认候选段");
    assert_eq!(after.selected_index, before.selected_index);
    assert_eq!(after.translated, before.translated);
    assert_eq!(after.candidates, before.candidates);
    assert_eq!((after.start, after.end), (before.start, before.end));
    assert!(h.state.locks.is_empty() && h.state.committed_raw.is_empty());
}

/// 数字直选：页内没有该位置时不消费（交回普通数字处理）。
///
/// 跨层分工：平台侧 `digit_select_out_of_page_falls_through` 负责引擎可见结果
/// （按键被消费、无上屏、输入尾部是 `0`）；本用例补充内核独有的结构：
/// 越页判定（页大小 5、`0` 的页内位置是第 10 槽）发生在**改动候选段之前**，
/// 整段逐字段保持原样。
#[test]
fn processor_digit_select_out_of_page_falls_through() {
    let mut h = Harness::new();
    h.context.set_option(OPTION_DIGIT_SELECT, true);
    h.context.set_option("_auto_commit", true);
    h.push_segment(b"ab", &["交", "疒"]);
    let before = h.context.composition.back().expect("段存在").clone();
    assert_eq!(h.press("0"), ProcessorResult::Consume);
    assert_eq!(h.context.last_commit_text(), "");
    assert_eq!(h.context.input(), b"ab0");
    let after = h.context.composition.back().expect("段仍存在");
    assert!(!after.selected);
    assert_eq!(after.selected_index, before.selected_index);
    assert_eq!(after.candidates, before.candidates);
    assert_eq!((after.start, after.end), (before.start, before.end));
    assert_eq!(h.context.caret(), 3);
    assert!(h.state.locks.is_empty());
}

/// 数字直选：页大小 10 时 `0` 上屏当前页第 10 个候选。
///
/// 跨层分工：平台侧 `digit_select_commits_page_candidate` 负责引擎可见结果
/// （候选列表长度、`COMMITS` 末项）；本用例补充内核独有的结构：候选数是 12
/// 而页大小是 10，`0` 必须落在**页内第 10 槽**（索引 9）而不是末位候选（索引 11），
/// 且整段确认上屏后组合与句子状态机一并归零。
#[test]
fn processor_digit_select_zero_picks_tenth_on_ten_page() {
    let mut h = Harness::new();
    h.context.set_option(OPTION_DIGIT_SELECT, true);
    h.page_size = 10;
    h.context.set_option("_auto_commit", true);
    let texts: Vec<String> = (0..12).map(|index| format!("候{index}")).collect();
    let refs: Vec<&str> = texts.iter().map(String::as_str).collect();
    h.push_segment(b"ab", &refs);
    let before = h.context.composition.back().expect("段存在").clone();
    assert_eq!(before.candidates.len(), 12);
    let tenth = before.candidates[9].text.clone();
    assert_ne!(tenth, before.candidates[11].text, "第 10 槽不是末位候选");
    assert_eq!(h.press("0"), ProcessorResult::Consume);
    assert_eq!(h.context.last_commit_text(), tenth);
    assert!(h.context.input().is_empty());
    assert_eq!(h.context.caret(), 0);
    assert!(h.context.composition.segments.is_empty());
    assert!(!h.context.is_composing());
    assert!(h.state.locks.is_empty());
    assert!(h.state.committed_raw.is_empty());
    assert!(h.state.committed_text.is_empty());
    assert!(h.state.buffered_text.is_empty());
}

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

#[test]
fn processor_backspace_pops_locked_input() {
    let mut h = Harness::new();
    // 锁分支：退格在锁下走 pop_input
    h.push_segment(b"ab", &["交"]);
    h.state.locks.push(Lock {
        raw: "a".to_string(),
        text: "交".to_string(),
        boundaries: "1,3;".to_string(),
    });
    h.state.committed_raw = "a".to_string();
    h.state.committed_text = "交".to_string();
    assert_eq!(h.press("BackSpace"), ProcessorResult::Consume);
    assert_eq!(h.context.input(), b"a");
}

#[test]
fn backspace_with_inconsistent_committed_text_does_not_panic() {
    let mut h = Harness::new();
    // 组合存在（缓冲退格分支的前提）且 live input 为空。
    h.push_segment(b"", &[]);
    // 属性可能来自旧版本/外部：committed_text 尾字符与 buffered 尾字符不一致时，
    // 退格只按字符边界截断，不得 panic。
    h.state.buffered_text = "A".to_string();
    h.state.committed_text = "甲".to_string();
    h.state.committed_raw = "a".to_string();
    assert_eq!(h.press("BackSpace"), ProcessorResult::Consume);
}
