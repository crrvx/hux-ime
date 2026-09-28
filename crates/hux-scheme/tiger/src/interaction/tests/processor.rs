// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! 处理器主循环、确认选择与反查（`interaction/processor.rs`、`interaction/select.rs`）的用例。
//!
//! 按子主题归档到子模块：按键分派（`dispatch`）、可打印字符与标点（`printable`）、
//! Tab 确认（`tab`）、确认选择与数字直选（`selection`）、音反查（`reverse`）、退格（`backspace`）；
//! 共享夹具 `Harness` 留在本模块，子模块经 `use super::*` 取用。

use super::*;

mod backspace;
mod dispatch;
mod printable;
mod reverse;
mod selection;
mod tab;

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
