// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! 子模块单测共用的上下文夹具（无上游对应组件）：带菜单的 [`Context`] 与按键辅助。

use super::{HostOptions, HostResult, process_key};
use crate::key::KeyEvent;
use crate::punct::PunctTable;
use crate::session::{Candidate, Context, Segment};

pub(super) fn context_with_menu(texts: &[&str], highlight: usize) -> Context {
    let mut context = Context::new();
    context.set_input(b"ab");
    let mut segment = Segment {
        start: 0,
        end: 2,
        tags: vec!["abc".to_string()],
        translated: true,
        selected_index: highlight,
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

pub(super) fn process(
    context: &mut Context,
    repr: &str,
    punct: Option<&PunctTable>,
    options: &HostOptions,
) -> HostResult {
    let key = KeyEvent::from_repr(repr).expect("key repr");
    process_key(&key, context, punct, options, None)
}

pub(super) fn press(context: &mut Context, repr: &str) -> HostResult {
    press_with(context, repr, &HostOptions::default())
}

pub(super) fn press_with(context: &mut Context, repr: &str, options: &HostOptions) -> HostResult {
    process(context, repr, None, options)
}

pub(super) fn selected(context: &Context) -> usize {
    context.composition.back().unwrap().selected_index
}

pub(super) fn custom_page_options(page_size: usize) -> HostOptions {
    HostOptions {
        page_size,
        page_up_keys: vec![KeyEvent::from_repr("comma").expect("key")],
        page_down_keys: vec![KeyEvent::from_repr("period").expect("key")],
        page_cycle: false,
    }
}

pub(super) fn key_of(repr: &str) -> KeyEvent {
    KeyEvent::from_repr(repr).expect("key repr")
}
