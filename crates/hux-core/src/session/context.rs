// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! 输入上下文：实况输入视图、选项/属性、编辑操作、菜单高亮、提交与事件队列。

use super::Composition;
use hashbrown::HashMap;
use std::collections::VecDeque;

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
mod tests;
