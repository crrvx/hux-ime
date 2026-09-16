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
//! - 管线（分段/翻译/过滤）由 [`Pipeline`] 注入；本增量提供模型与编辑语义，
//!   交互层（processor/translator/filters）在后续增量接入。

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
    pub selected_index: usize,
    pub candidates: Vec<Candidate>,
    /// 是否已被确认（librime `Segment::status >= kSelected`）。
    pub selected: bool,
}

impl Segment {
    pub fn has_tag(&self, tag: &str) -> bool {
        self.tags.iter().any(|value| value == tag)
    }

    pub fn selected_candidate(&self) -> Option<&Candidate> {
        self.candidates.get(self.selected_index)
    }

    /// 参照 `Menu::Prepare(n)`：物化到至少 n 条，返回当前可用数量。
    pub fn prepare(&self, count: usize) -> usize {
        self.candidates.len().min(count.max(1))
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

    pub fn back_mut(&mut self) -> Option<&mut Segment> {
        self.segments.last_mut()
    }

    /// 参照 `Composition::GetCommitText`：已确认段取选中候选文本，
    /// 未确认段取原始输入切片。
    pub fn commit_text(&self, input: &[u8]) -> String {
        let mut out = Vec::new();
        for segment in &self.segments {
            if segment.selected
                && let Some(candidate) = segment.selected_candidate()
            {
                out.extend_from_slice(candidate.text.as_bytes());
                continue;
            }
            let end = segment.end.min(input.len());
            let start = segment.start.min(end);
            out.extend_from_slice(&input[start..end]);
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

/// 管线注入点：根据当前输入重建组合（分段 → 翻译 → 过滤）。
pub trait Pipeline {
    fn build(&mut self, input: &[u8], composition: &mut Composition);
}

/// 上下文（librime `Context` 的常用子集）。
pub struct Context {
    input: Vec<u8>,
    caret: usize,
    pub composition: Composition,
    options: HashMap<String, bool>,
    properties: HashMap<String, String>,
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

    pub fn is_composing(&self) -> bool {
        !self.composition.empty()
    }

    pub fn has_menu(&self) -> bool {
        self.composition
            .back()
            .map(|segment| !segment.candidates.is_empty())
            .unwrap_or(false)
    }

    pub fn live_input(&self) -> &[u8] {
        let value = self.input();
        if !self.buffered().is_empty() && value.first() == Some(&b'~') {
            &value[1..]
        } else {
            value
        }
    }

    /// 与 `input` 对应的 caret 在 live 输入中的字节偏移（参照 `input_caret`）。
    pub fn live_caret(&self) -> usize {
        let length = self.live_input().len();
        let caret = if !self.buffered().is_empty() {
            self.caret.saturating_sub(1)
        } else {
            self.caret
        };
        caret.min(length)
    }

    pub fn get_commit_text(&self) -> &str {
        &self.last_commit
    }

    // ------------------------------------------------------------ 选项/属性

    pub fn get_option(&self, name: &str) -> bool {
        self.options.get(name).copied().unwrap_or(false)
    }

    pub fn set_option(&mut self, name: &str, value: bool) {
        if self.get_option(name) == value {
            self.options.insert(name.to_string(), value);
            return;
        }
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

    pub fn buffered(&self) -> &str {
        self.get_property("tiger_sentence_buffered_text")
            .unwrap_or("")
    }

    // ------------------------------------------------------------ 编辑操作

    /// 参照 `Context::PushInput`：在 caret 处插入并触发更新。
    pub fn push_input(&mut self, text: &[u8]) {
        let at = self.caret.min(self.input.len());
        self.input.splice(at..at, text.iter().copied());
        self.caret = at + text.len();
        self.events.push_back(Event::Update);
    }

    /// 参照 `Context::PopInput`：删除 caret 前 n 字节。
    pub fn pop_input(&mut self, count: usize) -> bool {
        if count == 0 || self.caret == 0 {
            return false;
        }
        let start = self.caret.saturating_sub(count);
        self.input.drain(start..self.caret);
        self.caret = start;
        self.events.push_back(Event::Update);
        true
    }

    /// 参照 `Context::DeleteInput`：删除 caret 处 n 字节。
    pub fn delete_input(&mut self, count: usize) -> bool {
        if count == 0 || self.caret >= self.input.len() {
            return false;
        }
        let end = (self.caret + count).min(self.input.len());
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

    /// 参照 `Context::Highlight`：截断到 `count-1`；索引未变化返回 false。
    pub fn highlight(&mut self, index: usize) -> bool {
        let Some(segment) = self.composition.back_mut() else {
            return false;
        };
        if segment.candidates.is_empty() {
            return false;
        }
        let count = segment.prepare(index + 1);
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

    /// 参照 `Context::RefreshNonConfirmedComposition`：
    /// 保留已确认段，仅重建未确认尾部（管线由调用方在事件后驱动）。
    pub fn refresh_non_confirmed_composition(&mut self) -> bool {
        if self.composition.empty() {
            return false;
        }
        let first_open = self
            .composition
            .segments
            .iter()
            .position(|segment| !segment.selected)
            .unwrap_or(self.composition.segments.len());
        if first_open >= self.composition.segments.len() {
            return false;
        }
        self.composition.segments.truncate(first_open);
        self.events.push_back(Event::Update);
        true
    }

    // ------------------------------------------------------------ 事件

    pub fn drain_events(&mut self) -> Vec<Event> {
        self.events.drain(..).collect()
    }

    pub fn has_events(&self) -> bool {
        !self.events.is_empty()
    }
}

/// 会话：上下文 + 管线，负责在编辑后重建组合并产生 UI 效果。
pub struct Session {
    pub context: Context,
    pipeline: Option<Box<dyn Pipeline>>,
    /// 直接提交（`engine:commit_text`）产生的 UI 效果。
    effects: Vec<Event>,
}

impl Default for Session {
    fn default() -> Self {
        Self::new()
    }
}

impl Session {
    pub fn new() -> Self {
        Self {
            context: Context::new(),
            pipeline: None,
            effects: Vec::new(),
        }
    }

    pub fn set_pipeline(&mut self, pipeline: Box<dyn Pipeline>) {
        self.pipeline = Some(pipeline);
    }

    /// 重建组合（分段 → 翻译 → 过滤），随后把 context 事件并入效果队列。
    pub fn refresh(&mut self) {
        if let Some(pipeline) = self.pipeline.as_mut() {
            let input = self.context.input().to_vec();
            let mut composition = Composition::default();
            pipeline.build(&input, &mut composition);
            self.context.composition = composition;
            self.context.drain_events();
            self.effects.push(Event::Update);
        }
    }

    /// 参照 `Engine::CommitText`：不经过组合的直接上屏。
    pub fn commit_text(&mut self, text: &str) {
        self.effects.push(Event::Commit(text.to_string()));
    }

    /// 取走自上次调用以来累积的 UI 效果。
    pub fn take_effects(&mut self) -> Vec<Event> {
        let mut events: Vec<Event> = self.context.drain_events();
        events.append(&mut self.effects);
        events
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
        context.drain_events();
        context.set_option("t", true);
        let events = context.drain_events();
        assert_eq!(events, vec![Event::Option("t".to_string())]);
    }

    #[test]
    fn confirm_and_commit_produce_text_and_clear() {
        let mut context = context_with_menu(&["甲", "乙"]);
        context.highlight(1);
        assert!(context.confirm_current_selection());
        let expected = context.composition.commit_text(context.input());
        assert!(context.commit());
        assert_eq!(context.get_commit_text(), expected);
        assert!(!context.is_composing());
        assert!(context.input().is_empty());
        assert!(matches!(
            context.drain_events().first(),
            Some(Event::Update)
        ));
    }

    #[test]
    fn refresh_keeps_selected_segments() {
        let mut context = context_with_menu(&["甲"]);
        context.composition.segments[0].selected = true;
        context.composition.segments.push(Segment::default());
        assert!(context.refresh_non_confirmed_composition());
        assert_eq!(context.composition.segments.len(), 1);
        assert!(context.composition.segments[0].selected);
    }
}
