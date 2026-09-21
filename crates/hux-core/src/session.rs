// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! 会话运行时：实现参照交互层依赖的 librime Context/Composition/Menu 子集。
//!
//! 对齐点（与参照用法一一对应）：
//! - 输入为**字节串**；`caret` 为字节偏移（0 = 最前）。
//! - `input` 可带私有缓冲标记 `~`（`buffered` 属性非空时），`live_input` 去除。
//! - 菜单：段内候选列表 + `selected_index`；`highlight` 为**绝对**索引：
//!   截断到 `count-1`，且索引未变化时返回 false（librime `Context::Highlight`）。
//! - `confirm_current_selection`：标记末段为选中（librime 语义），随后的 `commit`
//!   触发提交事件（事件含提交文本）并清空组合。
//! - 事件（update/commit/option）入队；调用方在每个操作后 `drain_events()`，
//!   与参照的同步 notifier 在可观测行为上等价。
//! - `last_commit` 保留最近一次组合提交文本，供诊断；参照的 `get_commit_text()`
//!   为即时计算（任何时刻可读），跨实现一律以 [`Event::Commit`] 携带的文本为准。
//! - 属性写入不产生事件（参照未使用 `property_update_notifier`）。
//! - 组合重建（分段/翻译/过滤）由**方案侧**交互层负责（`hux-scheme/*`）；
//!   本模块只提供 Context/Composition/Menu 子集，不感知任何方案。

use hashbrown::HashMap;
use std::collections::VecDeque;

/// 候选（对应参照经 `Candidate(...)` 构造的对象）。
#[derive(Clone, Debug, PartialEq)]
pub struct Candidate {
    pub kind: String,
    pub start: usize,
    pub end: usize,
    pub text: String,
    pub comment: String,
    pub preedit: String,
    pub quality: f64,
}

impl Candidate {
    pub fn new(kind: &str, start: usize, end: usize, text: &str, comment: &str) -> Self {
        Self {
            kind: kind.to_string(),
            start,
            end,
            text: text.to_string(),
            comment: comment.to_string(),
            preedit: String::new(),
            quality: 0.0,
        }
    }
}

/// 组合段（对应 librime `Segment` 的常用子集）。
#[derive(Clone, Debug, Default)]
pub struct Segment {
    pub start: usize,
    pub end: usize,
    pub tags: Vec<String>,
    /// 段提示（参照 `Segment::prompt`；如音反查段的「〔拼音〕」）。
    pub prompt: String,
    pub selected_index: usize,
    pub candidates: Vec<Candidate>,
    /// 是否已被确认（librime `Segment::status >= kSelected`）。
    pub selected: bool,
    /// 是否已建立菜单（librime `Segment::status >= kGuess`；`menu` 非空）。
    /// 已翻译的段在重分段时保留菜单与高亮。
    pub translated: bool,
}

impl Segment {
    pub fn has_tag(&self, tag: &str) -> bool {
        self.tags.iter().any(|value| value == tag)
    }

    pub fn selected_candidate(&self) -> Option<&Candidate> {
        self.candidates.get(self.selected_index)
    }

    /// 参照 `Menu::Prepare(n)`：返回当前可用数量（本实现候选为即时生成）。
    pub fn prepare(&self, count: usize) -> usize {
        self.candidates.len().min(count)
    }
}
/// 组合（对应 librime `Composition`）。
#[derive(Clone, Debug, Default)]
pub struct Composition {
    pub segments: Vec<Segment>,
}

impl Composition {
    pub fn empty(&self) -> bool {
        self.segments.is_empty()
    }

    pub fn back(&self) -> Option<&Segment> {
        self.segments.last()
    }

    /// 参照 `Segmentation::Forward`：末段非空时追加空尾段（下一轮起点）。
    pub fn forward(&mut self) -> bool {
        let Some(back) = self.segments.last() else {
            return false;
        };
        if back.start == back.end {
            return false;
        }
        let position = back.end;
        self.segments.push(Segment {
            start: position,
            end: position,
            ..Segment::default()
        });
        true
    }

    pub fn back_mut(&mut self) -> Option<&mut Segment> {
        self.segments.last_mut()
    }

    /// 参照 `Segmentation::GetCurrentStartPosition`。
    pub fn current_start_position(&self) -> usize {
        self.segments
            .last()
            .map(|segment| segment.start)
            .unwrap_or(0)
    }

    /// 参照 `Segmentation::GetCurrentEndPosition`。
    pub fn current_end_position(&self) -> usize {
        self.segments.last().map(|segment| segment.end).unwrap_or(0)
    }

    /// 参照 `Segmentation::HasFinishedSegmentation`。
    pub fn has_finished_segmentation(&self, input: &[u8]) -> bool {
        self.current_end_position() >= input.len()
    }

    /// 参照 `Segmentation::Trim`：移除末尾空段。
    pub fn trim(&mut self) -> bool {
        if self
            .segments
            .last()
            .map(|segment| segment.start == segment.end)
            .unwrap_or(false)
        {
            self.segments.pop();
            return true;
        }
        false
    }

    /// 参照 `Segmentation::GetConfirmedPosition`：最后一个已选段的末尾。
    pub fn confirmed_position(&self) -> usize {
        let mut confirmed = 0usize;
        for segment in &self.segments {
            if segment.selected {
                confirmed = segment.end;
            }
        }
        confirmed
    }

    /// 参照 `Composition::GetCommitText`：有选中候选的段取候选文本（不论段状态），
    /// 否则取原始输入切片（`phony` 段跳过）；末尾追加未被段覆盖的输入。
    pub fn commit_text(&self, input: &[u8]) -> String {
        let mut out = Vec::new();
        let mut end = 0usize;
        for segment in &self.segments {
            if let Some(candidate) = segment.selected_candidate() {
                end = candidate.end.min(input.len());
                out.extend_from_slice(candidate.text.as_bytes());
                continue;
            }
            end = segment.end.min(input.len());
            let start = segment.start.min(end);
            if !segment.has_tag("phony") {
                out.extend_from_slice(&input[start..end]);
            }
        }
        if input.len() > end {
            out.extend_from_slice(&input[end..]);
        }
        String::from_utf8_lossy(&out).into_owned()
    }
}

/// 运行时不变量/事件（调用方在每个操作后取走）。
#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    Update,
    /// 组合提交：携带提交文本（对应 `commit_notifier` + `get_commit_text()`）。
    Commit(String),
    Option(String),
}

/// 上下文（librime `Context` 的常用子集）。
pub struct Context {
    input: Vec<u8>,
    caret: usize,
    pub composition: Composition,
    options: HashMap<String, bool>,
    properties: HashMap<String, String>,
    /// 缓冲态（由方案设置；内核不解释来源，只影响实况输入视图）。
    buffered: bool,
    /// 标点成对符号的交替状态（**会话态**：每输入上下文一份）。
    punct_pairs: crate::punct::PairState,
    last_commit: String,
    events: VecDeque<Event>,
}

impl Default for Context {
    fn default() -> Self {
        Self::new()
    }
}

impl Context {
    pub fn new() -> Self {
        Self {
            input: Vec::new(),
            caret: 0,
            composition: Composition::default(),
            options: HashMap::new(),
            properties: HashMap::new(),
            buffered: false,
            punct_pairs: crate::punct::PairState::default(),
            last_commit: String::new(),
            events: VecDeque::new(),
        }
    }

    // ------------------------------------------------------------ 基本信息

    pub fn input(&self) -> &[u8] {
        &self.input
    }

    pub fn caret(&self) -> usize {
        self.caret
    }

    pub fn set_caret(&mut self, caret: usize) {
        self.caret = caret.min(self.input.len());
    }

    /// 参照 `Context::IsComposing`：`!input.empty() || !composition.empty()`。
    pub fn is_composing(&self) -> bool {
        !self.input.is_empty() || !self.composition.empty()
    }

    pub fn has_menu(&self) -> bool {
        self.composition
            .back()
            .map(|segment| !segment.candidates.is_empty())
            .unwrap_or(false)
    }

    pub fn live_input(&self) -> &[u8] {
        let value = self.input();
        if self.is_buffered() && value.first() == Some(&b'~') {
            &value[1..]
        } else {
            value
        }
    }

    /// 与 `input` 对应的 caret 在 live 输入中的字节偏移（参照 `input_caret`）。
    pub fn live_caret(&self) -> usize {
        // 判据与 [`Context::live_input`] 一致：只有**确实带 `~` 标记**时才算少一个字节。
        let live = self.live_input();
        let caret = if self.is_buffered() && self.input().first() == Some(&b'~') {
            self.caret.saturating_sub(1)
        } else {
            self.caret
        };
        caret.min(live.len())
    }

    /// 参照 `Context::GetCommitText`：按当前组合即时计算（未组合时为空串）。
    pub fn get_commit_text(&self) -> String {
        self.composition.commit_text(&self.input)
    }

    /// 最近一次组合提交文本（诊断用；事件文本以 [`Event::Commit`] 为准）。
    pub fn last_commit_text(&self) -> &str {
        &self.last_commit
    }

    /// 参照 `Engine::CommitText`：不经过组合的直接提交（事件供前端上屏）。
    pub fn direct_commit(&mut self, text: &str) {
        self.last_commit = text.to_string();
        self.events.push_back(Event::Commit(text.to_string()));
    }

    // ------------------------------------------------------------ 选项/属性

    pub fn get_option(&self, name: &str) -> bool {
        self.options.get(name).copied().unwrap_or(false)
    }
    /// 带缺省的选项读取（参照对缺省值有特殊约定的选项使用）。
    pub fn get_option_or(&self, name: &str, default: bool) -> bool {
        self.options.get(name).copied().unwrap_or(default)
    }

    /// 参照 `Context::set_option`：无条件触发选项通知（librime 语义）。
    pub fn set_option(&mut self, name: &str, value: bool) {
        self.options.insert(name.to_string(), value);
        self.events.push_back(Event::Option(name.to_string()));
    }

    pub fn get_property(&self, key: &str) -> Option<&str> {
        self.properties.get(key).map(String::as_str)
    }

    pub fn set_property(&mut self, key: &str, value: &str) {
        if value.is_empty() {
            self.properties.remove(key);
        } else {
            self.properties.insert(key.to_string(), value.to_string());
        }
    }

    /// 缓冲态（方案在写入自己的缓冲属性后，经 [`Context::set_buffered`] 同步）。
    pub fn is_buffered(&self) -> bool {
        self.buffered
    }

    /// 设置缓冲态；内核据此调整 [`Context::live_input`] / `live_caret` 的实况视图。
    pub fn set_buffered(&mut self, value: bool) {
        self.buffered = value;
    }

    /// 标点成对符号的交替状态（随 `Context` 隔离；标点表本身只读）。
    pub fn punct_pairs(&mut self) -> &mut crate::punct::PairState {
        &mut self.punct_pairs
    }

    // ------------------------------------------------------------ 编辑操作

    /// 参照 `Context::PushInput`：在 caret 处插入并触发更新。
    pub fn push_input(&mut self, text: &[u8]) {
        let at = self.caret.min(self.input.len());
        self.input.splice(at..at, text.iter().copied());
        self.caret = at + text.len();
        self.events.push_back(Event::Update);
    }

    /// 参照 `Context::PopInput`：删除 caret 前 n 字节；越界不改动并返回 false。
    pub fn pop_input(&mut self, count: usize) -> bool {
        if self.caret < count {
            return false;
        }
        let start = self.caret - count;
        self.input.drain(start..self.caret);
        self.caret = start;
        self.events.push_back(Event::Update);
        true
    }

    /// 参照 `Context::DeleteInput`：删除 caret 处 n 字节；越界不改动并返回 false。
    pub fn delete_input(&mut self, count: usize) -> bool {
        let Some(end) = self.caret.checked_add(count) else {
            return false;
        };
        if end > self.input.len() {
            return false;
        }
        self.input.drain(self.caret..end);
        self.events.push_back(Event::Update);
        true
    }

    /// 参照 `Context::set_input`：整体替换输入串（caret 移到末尾）。
    pub fn set_input(&mut self, value: &[u8]) {
        self.input.clear();
        self.input.extend_from_slice(value);
        self.caret = self.input.len();
        self.events.push_back(Event::Update);
    }

    pub fn clear(&mut self) {
        self.input.clear();
        self.caret = 0;
        self.composition = Composition::default();
        self.events.push_back(Event::Update);
    }

    // ------------------------------------------------------------ 菜单操作

    /// 参照 `Context::Highlight`：截断到 `count-1`；空菜单归 0；索引未变化返回 false。
    pub fn highlight(&mut self, index: usize) -> bool {
        let Some(segment) = self.composition.back_mut() else {
            return false;
        };
        if segment.candidates.is_empty() {
            let changed = segment.selected_index != 0;
            segment.selected_index = 0;
            if changed {
                self.events.push_back(Event::Update);
            }
            return changed;
        }
        let count = segment.prepare(index.saturating_add(1));
        let new_index = if count > 0 { (count - 1).min(index) } else { 0 };
        if segment.selected_index == new_index {
            return false;
        }
        segment.selected_index = new_index;
        self.events.push_back(Event::Update);
        true
    }

    /// 参照 `Context::ConfirmCurrentSelection`：标记末段为已选。
    pub fn confirm_current_selection(&mut self) -> bool {
        let Some(segment) = self.composition.back_mut() else {
            return false;
        };
        segment.selected = true;
        segment.selected_candidate().is_some() || segment.end > segment.start
    }

    /// 参照 `Context::Commit`：先发提交事件（含提交文本），再清空。
    pub fn commit(&mut self) -> bool {
        if !self.is_composing() {
            return false;
        }
        let text = self.composition.commit_text(&self.input);
        self.last_commit = text.clone();
        self.events.push_back(Event::Commit(text));
        self.clear();
        true
    }

    /// 参照 `ClearNonConfirmedComposition` / `RefreshNonConfirmedComposition`：
    /// 从尾部弹出未确认段（`status < kSelected`）后追加空尾段（`Forward`）。
    pub fn refresh_non_confirmed_composition(&mut self) -> bool {
        let mut reverted = false;
        while self
            .composition
            .segments
            .last()
            .map(|segment| !segment.selected)
            .unwrap_or(false)
        {
            self.composition.segments.pop();
            reverted = true;
        }
        if reverted {
            self.composition.forward();
            self.events.push_back(Event::Update);
        }
        reverted
    }

    // ------------------------------------------------------------ 事件

    pub fn drain_events(&mut self) -> Vec<Event> {
        self.events.drain(..).collect()
    }
}

/// 参照 `set_property_if_changed`：仅在值变化时写入属性（属性写入不产生事件，
/// 但避免无谓的属性更新）。方案侧与配置层共用。
pub fn set_property_if_changed(context: &mut Context, key: &str, value: &str) {
    if context.get_property(key).unwrap_or("") != value {
        context.set_property(key, value);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn context_with_menu(texts: &[&str]) -> Context {
        let mut context = Context::new();
        context.set_input(b"ab");
        let mut segment = Segment {
            start: 0,
            end: 2,
            tags: vec!["abc".to_string()],
            prompt: String::new(),
            ..Segment::default()
        };
        for text in texts {
            segment
                .candidates
                .push(Candidate::new("sentence", 0, 2, text, ""));
        }
        context.composition.segments.push(segment);
        context.drain_events();
        context
    }

    #[test]
    fn is_composing_includes_raw_input() {
        let mut context = Context::new();
        assert!(!context.is_composing());
        context.push_input(b"a");
        assert!(context.is_composing()); // 无组合但 input 非空（librime 语义）
        assert!(context.commit());
        assert_eq!(context.last_commit_text(), "a");
        assert!(!context.is_composing());
    }

    #[test]
    fn pop_input_rejects_out_of_range() {
        let mut context = Context::new();
        context.push_input(b"ab");
        context.set_caret(1);
        assert!(!context.pop_input(2)); // caret < count：不改动
        assert_eq!(context.input(), b"ab");
        assert!(context.pop_input(1));
        assert_eq!(context.input(), b"b");
    }

    #[test]
    fn delete_input_rejects_out_of_range() {
        let mut context = Context::new();
        context.push_input(b"ab");
        context.set_caret(1);
        assert!(!context.delete_input(2)); // 超出末尾：不改动
        assert_eq!(context.input(), b"ab");
        context.drain_events();
        assert!(context.delete_input(0)); // 0 长度：触发更新并返回 true
        assert_eq!(context.drain_events(), vec![Event::Update]);
    }

    #[test]
    fn commit_text_uses_candidate_without_selected_flag() {
        // 未标记 selected 的段同样按选中候选取文本（librime 语义）
        let mut context = Context::new();
        context.set_input(b"abcd");
        context.composition.segments.push(Segment {
            start: 0,
            end: 2,
            candidates: vec![Candidate::new("sentence", 0, 2, "甲", "")],
            ..Segment::default()
        });
        assert_eq!(context.composition.commit_text(context.input()), "甲cd");
    }

    #[test]
    fn commit_text_appends_uncovered_input() {
        // 无候选段取输入切片；末尾未被覆盖的输入追加
        let mut context = Context::new();
        context.set_input(b"abcd");
        context.composition.segments.push(Segment {
            start: 0,
            end: 2,
            ..Segment::default()
        });
        assert_eq!(context.composition.commit_text(context.input()), "abcd");
    }

    #[test]
    fn empty_menu_highlight_resets_selection() {
        let mut context = Context::new();
        context.composition.segments.push(Segment::default());
        context.composition.segments[0].selected_index = 2;
        assert!(context.highlight(0));
        assert_eq!(context.composition.segments[0].selected_index, 0);
    }

    #[test]
    fn edits_follow_byte_caret_semantics() {
        let mut context = Context::new();
        context.push_input(b"ab");
        context.set_caret(1);
        context.push_input(b"x");
        assert_eq!(context.input(), b"axb");
        assert_eq!(context.caret(), 2);
        assert!(context.pop_input(1));
        assert_eq!(context.input(), b"ab");
        assert_eq!(context.caret(), 1);
        assert!(context.delete_input(1));
        assert_eq!(context.input(), b"a");
        assert!(!context.delete_input(1));
        context.set_input(b"xyz");
        assert_eq!(context.caret(), 3);
        context.set_caret(99);
        assert_eq!(context.caret(), 3);
    }

    #[test]
    fn highlight_clamps_and_reports_changes() {
        let mut context = context_with_menu(&["甲", "乙", "丙"]);
        assert!(context.highlight(1));
        assert_eq!(context.composition.back().unwrap().selected_index, 1);
        assert!(!context.highlight(1));
        assert!(context.highlight(99));
        assert_eq!(context.composition.back().unwrap().selected_index, 2);
    }

    #[test]
    fn set_option_notifies_unconditionally() {
        let mut context = Context::new();
        context.set_option("t", true);
        assert_eq!(context.drain_events(), vec![Event::Option("t".to_string())]);
        // 参照 `Context::set_option` 无条件通知：同值再设仍触发。
        context.set_option("t", true);
        assert_eq!(context.drain_events(), vec![Event::Option("t".to_string())]);
    }

    #[test]
    fn empty_menu_highlight_fails() {
        let mut context = Context::new();
        assert!(!context.highlight(0));
        context.composition.segments.push(Segment::default());
        assert!(!context.highlight(0));
        assert!(!context.confirm_current_selection());
    }

    #[test]
    fn buffered_marker_live_views() {
        let mut context = Context::new();
        context.set_buffered(true);
        context.set_input(b"~ab");
        assert_eq!(context.live_input(), b"ab");
        context.set_caret(2); // "~a|b"
        assert_eq!(context.live_caret(), 1);
        context.set_buffered(false);
        assert_eq!(context.live_input(), b"~ab");
        assert_eq!(context.live_caret(), 2);
    }

    #[test]
    fn commit_text_spans_selected_and_raw_segments() {
        let mut context = Context::new();
        context.set_input(b"abcd");
        context.composition.segments.push(Segment {
            start: 0,
            end: 2,
            selected: true,
            candidates: vec![Candidate::new("sentence", 0, 2, "甲", "")],
            selected_index: 0,
            tags: Vec::new(),
            prompt: String::new(),
            translated: true,
        });
        context.composition.segments.push(Segment {
            start: 2,
            end: 4,
            ..Segment::default()
        });
        assert_eq!(context.composition.commit_text(context.input()), "甲cd");
    }

    #[test]
    fn refresh_pops_open_tail_only() {
        let mut context = context_with_menu(&["甲"]);
        context.composition.segments[0].selected = true;
        assert!(!context.refresh_non_confirmed_composition());
        context.composition.segments.push(Segment::default());
        assert!(context.refresh_non_confirmed_composition());
        // 已确认段保留 + 追加空尾段（参照 `Segmentation::Forward`）。
        assert_eq!(context.composition.segments.len(), 2);
        assert!(context.composition.segments[0].selected);
        assert_eq!(context.composition.segments[1].start, 2);
        assert_eq!(context.composition.segments[1].end, 2);
    }

    #[test]
    fn confirm_current_selection_accepts_highlight() {
        let mut context = context_with_menu(&["甲", "乙"]);
        context.highlight(1);
        assert!(context.confirm_current_selection());
        let segment = context.composition.back().unwrap();
        assert_eq!(segment.selected_index, 1);
        assert!(segment.selected);
    }

    #[test]
    fn commit_emits_text_and_clears() {
        let mut context = context_with_menu(&["甲", "乙"]);
        context.highlight(1);
        assert!(context.confirm_current_selection());
        let expected = context.composition.commit_text(context.input());
        context.drain_events(); // 清掉 highlight/confirm 的残留事件，只断言提交本身
        assert!(context.commit());
        assert_eq!(context.last_commit_text(), expected);
        assert!(!context.is_composing());
        assert!(context.input().is_empty());
        let events = context.drain_events();
        assert!(
            matches!(events.first(), Some(Event::Commit(text)) if *text == expected),
            "提交应先派发 Commit：{events:?}"
        );
        assert!(matches!(events.get(1), Some(Event::Update)));
    }

    #[test]
    fn refresh_keeps_selected_segments() {
        let mut context = context_with_menu(&["甲"]);
        context.composition.segments[0].selected = true;
        context.composition.segments.push(Segment::default());
        assert!(context.refresh_non_confirmed_composition());
        assert_eq!(context.composition.segments.len(), 2);
        assert!(context.composition.segments[0].selected);
    }
}
