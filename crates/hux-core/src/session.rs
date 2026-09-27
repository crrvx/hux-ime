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

    /// 参照 `Composition::GetScriptText(keep_selection)`：**脚本文本**（`Ctrl+Return` 提交）。
    ///
    /// 每段按参照的三级判据取文本：① `keep_selection` 且段已确认（`status >= kSelected`）
    /// 且候选文字非空 ⇒ 候选文字；② 否则候选 `preedit` 非空 ⇒ `preedit` **去掉首个 `\t`**
    /// （`erase_first_copy`）；③ 否则非 `phony` 段 ⇒ 原始输入切片。末尾追加未被段覆盖的输入。
    ///
    /// 与 [`Composition::commit_text`] 的差异即「脚本文本 ≠ 提交文本」：确认段取候选文字
    /// （`keep_selection`）或 preedit，而不是候选 `text`；与参照一致地**不**按候选 `end`
    /// 截断段的原始切片。候选 `end` 超出输入时按输入长度钳制（参照无此护栏；此处与
    /// `commit_text` 同口径，避免越界切片）。
    pub fn script_text(&self, input: &[u8], keep_selection: bool) -> String {
        let mut out = Vec::new();
        let mut end = 0usize;
        for segment in &self.segments {
            let start = end;
            let candidate = segment.selected_candidate();
            end = candidate
                .map(|candidate| candidate.end)
                .unwrap_or(segment.end)
                .min(input.len());
            let stop = start.min(end);
            if keep_selection
                && let Some(candidate) = candidate
                && !candidate.text.is_empty()
                && segment.selected
            {
                out.extend_from_slice(candidate.text.as_bytes());
            } else if let Some(candidate) = candidate
                && !candidate.preedit.is_empty()
            {
                match candidate.preedit.split_once('\t') {
                    Some((head, tail)) => {
                        out.extend_from_slice(head.as_bytes());
                        out.extend_from_slice(tail.as_bytes());
                    }
                    None => out.extend_from_slice(candidate.preedit.as_bytes()),
                }
            } else if !segment.has_tag("phony") {
                out.extend_from_slice(&input[stop..end]);
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

    /// 参照 `Context::GetScriptText`（`context.cc`）：`composition_.GetScriptText()`
    /// ——`Ctrl+Return`（`Editor::CommitScriptText`）提交的「脚本文本」。
    ///
    /// 参照不带实参调用，故取 `composition.h` 的默认实参 `keep_selection = true`。
    pub fn get_script_text(&self) -> String {
        self.composition.script_text(&self.input, true)
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

    /// 丢弃队列中指定选项名的 `Event::Option`（**写入方抑制自身事件**用）。
    ///
    /// 参照 `M.options.sync` 以 `live.syncing` 在**写入时**抑制自身的选项通知；
    /// 本实现的 `set_option` 是入队语义，故由写入方在写完后丢弃这些事件——
    /// 否则它们会在稍后被当成用户改动观察（并吞掉紧随其后的第一次真实改动）。
    pub fn discard_option_events(&mut self, names: &[String]) {
        self.events.retain(|event| match event {
            Event::Option(name) => !names.contains(name),
            _ => true,
        });
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
    ///
    /// 段**未建立菜单**（本模型的 [`Segment::translated`] ⇒ 参照 `!back().menu`）时
    /// 直接返回 false：不改写 `selected_index`、不推 `Update`（参照 `context.cc`
    /// 首行即 `if (composition_.empty() || !composition_.back().menu) return false;`）。
    pub fn highlight(&mut self, index: usize) -> bool {
        let Some(segment) = self.composition.back_mut() else {
            return false;
        };
        if !segment.translated {
            return false;
        }
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
            // 有候选即「已建立菜单」（参照 `menu` 非空）；`highlight` 依此判据。
            translated: true,
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

    /// 提交必须彻底退出组合态；判据同时看组合段与裸输入，避免提交后残留输入被当成仍在输入中。
    #[test]
    fn is_composing_includes_raw_input() {
        let mut context = Context::new();
        assert!(
            !context.is_composing(),
            "新建上下文不得处于组合态：input 与 composition 都应为空"
        );
        context.push_input(b"a");
        assert!(
            context.is_composing(),
            "有裸输入（尚无组合段）也必须算组合中：IsComposing 判据含 input 非空"
        ); // 无组合但 input 非空（librime 语义）
        assert!(
            context.commit(),
            "存在裸输入时提交必须成功，否则输入无法上屏"
        );
        assert_eq!(
            context.last_commit_text(),
            "a",
            "无候选段时提交文本必须是原始输入切片"
        );
        assert!(
            !context.is_composing(),
            "提交后必须退出组合态：input 与 composition 都应被清空"
        );
    }

    /// 退格按「自光标向前 pop_n」计数，越界即整条拒绝且不动缓冲区，不允许部分删除。
    #[test]
    fn pop_input_rejects_out_of_range() {
        let mut context = Context::new();
        context.push_input(b"ab");
        context.set_caret(1);
        assert!(
            !context.pop_input(2),
            "光标前的字节数不足时必须整条拒绝（caret=1 < count=2），不得部分删除"
        ); // caret < count：不改动
        assert_eq!(
            context.input(),
            b"ab",
            "越界退格不得改动输入缓冲区：被拒绝的请求必须是零副作用"
        );
        assert!(context.pop_input(1), "光标前恰好有 1 字节时退格必须成功");
        assert_eq!(
            context.input(),
            b"b",
            "退格应删掉光标前 1 字节并留下其余输入"
        );
    }

    /// 删除与退格共用越界判据；0 长度仍是有效请求，必须触发一次刷新通知。
    #[test]
    fn delete_input_rejects_out_of_range() {
        let mut context = Context::new();
        context.push_input(b"ab");
        context.set_caret(1);
        assert!(
            !context.delete_input(2),
            "caret+count 超出输入长度时必须整条拒绝，不得部分删除"
        ); // 超出末尾：不改动
        assert_eq!(
            context.input(),
            b"ab",
            "越界删除不得改动输入缓冲区：被拒绝的请求必须是零副作用"
        );
        context.drain_events();
        assert!(
            context.delete_input(0),
            "0 长度删除是有效请求：必须返回 true，不得当成越界"
        ); // 0 长度：触发更新并返回 true
        assert_eq!(
            context.drain_events(),
            vec![Event::Update],
            "0 长度删除仍须推一次 Update：宿主靠事件刷新视图"
        );
    }

    /// selected 标记只影响高亮与显示，不参与取文：未标记段的选中候选同样要被取用。
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
        assert_eq!(
            context.composition.commit_text(context.input()),
            "甲cd",
            "未标 selected 的段同样按选中候选取文本：候选 text 参与，段外输入原样追加"
        );
    }

    /// 无候选段回退为输入切片，末尾未被段覆盖的输入必须原样追加，不得丢字。
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
        assert_eq!(
            context.composition.commit_text(context.input()),
            "abcd",
            "无候选段取输入切片，末尾未被段覆盖的输入必须原样追加，不得丢字"
        );
    }

    /// 「已建菜单」与「有候选」是两件事：菜单在而候选空时要复位选中并通知，未翻译段则完全不碰。
    #[test]
    fn empty_menu_highlight_resets_selection() {
        let mut context = Context::new();
        context.composition.segments.push(Segment::default());
        context.composition.segments[0].selected_index = 2;
        // 未翻译段（参照 `menu == null`）不改写、不通知。
        assert!(
            !context.highlight(0),
            "段未建立菜单时高亮必须不动作：不得改写 selected_index，也不得推 Update"
        );
        assert_eq!(
            context.composition.segments[0].selected_index, 2,
            "未建菜单的段不得被改写选中索引（应保持原值 2）"
        );
        // 已建立菜单但候选为空（参照 `menu` 存在、`Prepare` 返回 0）：归 0 并在变化时通知。
        context.composition.segments[0].translated = true;
        assert!(
            context.highlight(0),
            "菜单存在而候选为空时必须归 0，并因确有变化返回 true"
        );
        assert_eq!(
            context.composition.segments[0].selected_index, 0,
            "空菜单高亮必须把选中索引夹到 0"
        );
    }

    /// 无菜单时高亮既不动作也不推事件：事件队列是宿主唯一信号，多推会让宿主重排候选。
    #[test]
    fn highlight_skips_untranslated_segment_without_update() {
        let mut context = Context::new();
        context.composition.segments.push(Segment {
            selected_index: 3,
            ..Segment::default()
        });
        context.drain_events();
        assert!(!context.highlight(0), "参照 `Highlight` 在无菜单时不动作");
        assert_eq!(
            context.composition.back().unwrap().selected_index,
            3,
            "无菜单时高亮不得改写选中索引（应保持原值 3）"
        );
        assert!(context.drain_events().is_empty(), "无菜单时不得推 Update");
    }

    /// 显示串取三段优先级（保留选中的候选文字、候选 preedit、原始输入），keep_selection 决定第一段是否参与。
    #[test]
    fn script_text_prefers_preedit_then_raw_input() {
        // 参照 `Composition::GetScriptText`：① 确认段 + `keep_selection` ⇒ 候选文字；
        // ② 候选 preedit 非空 ⇒ preedit（去掉首个 `\t`）；③ 否则原始输入切片。
        let mut context = Context::new();
        context.set_input(b"abcd");
        let mut segment = Segment {
            start: 0,
            end: 2,
            translated: true,
            ..Segment::default()
        };
        let mut candidate = Candidate::new("sentence", 0, 2, "甲", "");
        candidate.preedit = "xi\tan".to_string();
        segment.candidates.push(candidate);
        context.composition.segments.push(segment);
        assert_eq!(
            context.get_script_text(),
            "xiancd",
            "preedit 优先且去首个 \\t"
        );
        // 段已确认：`keep_selection = true`（`Context::GetScriptText` 的默认实参）取候选文字。
        context.composition.segments[0].selected = true;
        assert_eq!(
            context.get_script_text(),
            "甲cd",
            "确认段且 keep_selection 时脚本文本必须取候选文字，而不是 preedit 或原文"
        );
        // `keep_selection = false`：确认段仍走 preedit 分支（候选 `text` 不参与）。
        assert_eq!(
            context.composition.script_text(context.input(), false),
            "xiancd",
            "keep_selection=false 时确认段仍须走 preedit 分支（去掉首个 \t），候选 text 不参与脚本文本"
        );
        // 候选既无 preedit 也不保留选中：退回原始输入切片。
        context.composition.segments[0].candidates[0]
            .preedit
            .clear();
        assert_eq!(
            context.composition.script_text(context.input(), false),
            "abcd",
            "候选无 preedit 且不保留选中时必须退回原始输入切片"
        );
    }

    /// phony 段是内部占位，不得出现在显示串里；段未覆盖的尾巴照常追加。
    #[test]
    fn script_text_skips_phony_segments_and_appends_tail() {
        let mut context = Context::new();
        context.set_input(b"abcd");
        context.composition.segments.push(Segment {
            start: 0,
            end: 2,
            translated: true,
            tags: vec!["phony".to_string()],
            ..Segment::default()
        });
        // `phony` 段不产出原文；末尾未被段覆盖的输入照常追加。
        assert_eq!(
            context.get_script_text(),
            "cd",
            "phony 段是内部占位：不得产出原文，段外尾巴照常追加"
        );
    }

    /// 光标是字节下标：插入删除都按字节移动，set_caret 超界夹到末尾而不是报错。
    #[test]
    fn edits_follow_byte_caret_semantics() {
        let mut context = Context::new();
        context.push_input(b"ab");
        context.set_caret(1);
        context.push_input(b"x");
        assert_eq!(context.input(), b"axb", "插入必须落在 caret 处而不是末尾");
        assert_eq!(context.caret(), 2, "插入后 caret 必须前移插入的字节数");
        assert!(context.pop_input(1), "caret 前有 1 字节时退格必须成功");
        assert_eq!(context.input(), b"ab", "退格应删掉刚插入的那个字节");
        assert_eq!(context.caret(), 1, "退格后 caret 必须回到被删字节之前");
        assert!(
            context.delete_input(1),
            "caret 处恰好有 1 字节时删除必须成功"
        );
        assert_eq!(context.input(), b"a", "删除应删掉 caret 处的字节");
        assert!(
            !context.delete_input(1),
            "caret 已在末尾时删除必须失败，不得越过输入长度"
        );
        context.set_input(b"xyz");
        assert_eq!(
            context.caret(),
            3,
            "整体替换输入后 caret 必须移到新输入的末尾"
        );
        context.set_caret(99);
        assert_eq!(
            context.caret(),
            3,
            "set_caret 超界必须夹到输入末尾，而不是报错或越界"
        );
    }

    /// 高亮越界夹到末位候选，且只有真正变化才返回 true 并通知宿主。
    #[test]
    fn highlight_clamps_and_reports_changes() {
        let mut context = context_with_menu(&["甲", "乙", "丙"]);
        assert!(
            context.highlight(1),
            "高亮到与当前不同的索引必须返回 true（宿主据此刷新候选）"
        );
        assert_eq!(
            context.composition.back().unwrap().selected_index,
            1,
            "高亮索引必须写入末段的 selected_index"
        );
        assert!(
            !context.highlight(1),
            "索引未变化必须返回 false：不得让宿主做无谓重排"
        );
        assert!(
            context.highlight(99),
            "越界高亮必须夹到末位候选，并因确有变化返回 true"
        );
        assert_eq!(
            context.composition.back().unwrap().selected_index,
            2,
            "越界高亮必须夹到 count-1，不得超出候选数"
        );
    }

    /// set_option 无条件通知（同值也发），去重交给接收方，宿主据此刷新状态栏。
    #[test]
    fn set_option_notifies_unconditionally() {
        let mut context = Context::new();
        context.set_option("t", true);
        assert_eq!(
            context.drain_events(),
            vec![Event::Option("t".to_string())],
            "set_option 必须无条件入队一条 Option 事件"
        );
        // 参照 `Context::set_option` 无条件通知：同值再设仍触发。
        context.set_option("t", true);
        assert_eq!(
            context.drain_events(),
            vec![Event::Option("t".to_string())],
            "同值重设仍须通知：去重交给接收方，写入方不得自行吞事件"
        );
    }

    /// 没有组合段时高亮与确认都必须失败，不能凭空造出候选。
    #[test]
    fn empty_menu_highlight_fails() {
        let mut context = Context::new();
        assert!(
            !context.highlight(0),
            "没有组合段时高亮必须失败，不得凭空造段"
        );
        context.composition.segments.push(Segment::default());
        assert!(!context.highlight(0), "段未建立菜单时高亮必须失败");
        assert!(
            !context.confirm_current_selection(),
            "无候选且零长度的段确认必须失败，不得凭空造出候选"
        );
    }

    /// 缓冲模式下 ~ 是标记而非输入：live_* 视图要去掉标记并把光标相应左移，退出缓冲后回归原样。
    #[test]
    fn buffered_marker_live_views() {
        let mut context = Context::new();
        context.set_buffered(true);
        context.set_input(b"~ab");
        assert_eq!(
            context.live_input(),
            b"ab",
            "缓冲态下 ~ 是私有标记：live_input 必须去掉它"
        );
        context.set_caret(2); // "~a|b"
        assert_eq!(
            context.live_caret(),
            1,
            "live_caret 必须随去掉的 ~ 标记左移一位"
        );
        context.set_buffered(false);
        assert_eq!(
            context.live_input(),
            b"~ab",
            "退出缓冲态后 ~ 不再是标记：live_input 必须原样返回"
        );
        assert_eq!(
            context.live_caret(),
            2,
            "退出缓冲态后 live_caret 必须与 caret 一致（不再减一）"
        );
    }

    /// 提交串由各段拼接：选中段取候选文字，未覆盖段取输入切片，段之间不得重叠或跳字。
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
        assert_eq!(
            context.composition.commit_text(context.input()),
            "甲cd",
            "选中段取候选文字、其余段取输入切片：段间不得重叠或跳字"
        );
    }

    /// 刷新只丢未确认的尾段，已确认段必须保留，并重新追加一个空的开放尾段。
    #[test]
    fn refresh_pops_open_tail_only() {
        let mut context = context_with_menu(&["甲"]);
        context.composition.segments[0].selected = true;
        assert!(
            !context.refresh_non_confirmed_composition(),
            "只有已确认段时刷新必须返回 false：没有段可回退"
        );
        context.composition.segments.push(Segment::default());
        assert!(
            context.refresh_non_confirmed_composition(),
            "存在未确认尾段时刷新必须回退并返回 true"
        );
        // 已确认段保留 + 追加空尾段（参照 `Segmentation::Forward`）。
        assert_eq!(
            context.composition.segments.len(),
            2,
            "刷新后必须是「已确认段 + 新空尾段」两段"
        );
        assert!(
            context.composition.segments[0].selected,
            "刷新不得清掉已确认段的 selected 标记：用户已上屏的选择会被回退"
        );
        assert_eq!(
            context.composition.segments[1].start, 2,
            "新追加的尾段必须从保留段的末尾开始（Forward 语义）"
        );
        assert_eq!(
            context.composition.segments[1].end, 2,
            "新追加的尾段必须是零长度的开放段"
        );
    }

    /// 确认把当前高亮位置钉成选中：selected_index 保持、selected 置位。
    #[test]
    fn confirm_current_selection_accepts_highlight() {
        let mut context = context_with_menu(&["甲", "乙"]);
        context.highlight(1);
        assert!(
            context.confirm_current_selection(),
            "末段有候选时确认必须成功"
        );
        let segment = context.composition.back().unwrap();
        assert_eq!(
            segment.selected_index, 1,
            "确认不得改动高亮位置：selected_index 必须保持确认前的值"
        );
        assert!(
            segment.selected,
            "确认必须把末段标记为已选（status >= kSelected）"
        );
    }

    /// 提交的事件序是 Commit 在前、Update 在后，随后输入与组合态一并清空。
    #[test]
    fn commit_emits_text_and_clears() {
        let mut context = context_with_menu(&["甲", "乙"]);
        context.highlight(1);
        assert!(
            context.confirm_current_selection(),
            "末段有候选时确认必须成功，否则提交流程无法前进"
        );
        let expected = context.composition.commit_text(context.input());
        context.drain_events(); // 清掉 highlight/confirm 的残留事件，只断言提交本身
        assert!(context.commit(), "组合非空时提交必须成功");
        assert_eq!(
            context.last_commit_text(),
            expected,
            "last_commit 必须等于提交时的即时 commit_text"
        );
        assert!(!context.is_composing(), "提交后不得残留组合态");
        assert!(
            context.input().is_empty(),
            "提交后输入缓冲区必须清空，避免残留输入被当成仍在输入中"
        );
        let events = context.drain_events();
        assert!(
            matches!(events.first(), Some(Event::Commit(text)) if *text == expected),
            "提交应先派发 Commit：{events:?}"
        );
        assert!(
            matches!(events.get(1), Some(Event::Update)),
            "提交事件序必须是 Commit 在前、Update 在后（宿主靠 Update 刷新视图）"
        );
    }

    /// 刷新不得动已确认段的选中标记，否则用户已上屏的选择会被回退。
    #[test]
    fn refresh_keeps_selected_segments() {
        let mut context = context_with_menu(&["甲"]);
        context.composition.segments[0].selected = true;
        context.composition.segments.push(Segment::default());
        assert!(
            context.refresh_non_confirmed_composition(),
            "存在未确认尾段时刷新必须回退并返回 true"
        );
        assert_eq!(
            context.composition.segments.len(),
            2,
            "刷新后必须保留已确认段并追加空尾段，共两段"
        );
        assert!(
            context.composition.segments[0].selected,
            "刷新不得动已确认段的 selected 标记，否则用户已上屏的选择会被回退"
        );
    }
}
