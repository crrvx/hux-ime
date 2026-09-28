// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! 参照上游 `processor(key_event, env)` 的主入口、按键借用束与键分发接线。

use super::*;
use hux_core::host::HostOptions;

mod backspace;
mod dispatch;
mod printable;
mod reverse;
mod selection;
mod tab;

pub use selection::select_candidate_at;
pub(crate) use selection::{digit_page_position, select_page_candidate};

use reverse::{handle_recognizer, handle_reverse_lookup_triggers};

// ---------------------------------------------------------------- 处理器

/// 处理器宿主环境（对应参照 `env` 的非会话部分；内存 / 词库 / 选项由宿主层补）。
pub struct ProcessorEnv<'a> {
    /// 参照 `os.time()`（学习事件时间戳）。
    pub now: f64,
    /// 参照 `env._tiger_sentence_dot_armed`（数字后小数点待发）。
    pub dot_armed: &'a mut bool,
    /// 参照 `get_min_retained_raw_length(env)` 的配置值（负数已在合约层归一为 `0`）。
    pub min_retained: usize,
    /// 每页候选个数（addon 设置；数字直选按页定位）。
    pub page_size: usize,
    /// 宿主链选项（翻页键绑定）：菜单可见的标点分支据此先问
    /// [`hux_core::host::paging_action`]，让出会被它遮蔽的翻页绑定
    /// （**本仓有意偏离上游 `abad411`**）。
    pub host_options: &'a HostOptions,
}

/// 处理器结果：`Consume` 对应参照返回 1（拦截），`Forward` 对应 2（交后续处理器）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ProcessorResult {
    Consume,
    Forward,
}

// ---------------------------------------------------------------- 键分发

/// 学习提交句柄（`decoder` / `live` / `now` 的成组借用）。
fn learning_commit<'a>(
    decoder: &'a mut Decoder,
    live: &'a mut LiveLearning,
    now: f64,
) -> LearningCommit<'a> {
    LearningCommit { decoder, live, now }
}

/// 一次按键的借用束：`process_key_event()` 的后续分支都要这组可变借用，逐个透传会超出参数上限，
/// 故在此一次成组、按分支取用。
struct KeyDispatch<'a, 'b> {
    key_event: &'b KeyEvent,
    repr: &'b str,
    context: &'b mut Context,
    state: &'b mut SentenceState,
    decoder: &'b mut Decoder,
    live: &'b mut LiveLearning,
    env: &'b mut ProcessorEnv<'a>,
    /// `set_allow_duplicate_single(context)` 的结果（与 `EarlyCommitParams` 同值）。
    allow_duplicate_single: bool,
    /// 参照 `_dotAfterDigitArmed`：进入分发前取到的待发值。
    dot_armed: bool,
}

// ---------------------------------------------------------------- 主入口

/// 参照上游 `processor(key_event, env)`。宿主职责（内存配置、词库懒加载、选项同步、
/// 学习库存储）由调用方在进入前完成。
pub fn process_key_event(
    key_event: &KeyEvent,
    context: &mut Context,
    state: &mut SentenceState,
    decoder: &mut Decoder,
    live: &mut LiveLearning,
    env: &mut ProcessorEnv<'_>,
) -> anyhow::Result<ProcessorResult> {
    if key_event.release() {
        return Ok(ProcessorResult::Forward);
    }
    if let Some(result) = handle_reverse_lookup_triggers(key_event, context) {
        return Ok(result);
    }
    if handle_recognizer(key_event, context) {
        return Ok(ProcessorResult::Consume);
    }
    let repr = key_event.repr();
    let repr = repr.as_str();
    let allow_duplicate_single = set_allow_duplicate_single(context);
    decoder.set_allow_duplicate_single(allow_duplicate_single);
    live.submitted_raw = None;
    if let Some(result) = buffered_idle_guard(state, context, repr) {
        return Ok(result);
    }
    if !context.is_composing() {
        live.pending.clear();
        live.baseline = None;
    }
    let dot_armed = consume_dot_armed(env, repr);
    let params = EarlyCommitParams {
        allow_duplicate_single,
        generation: state.model_generation,
        min_retained: env.min_retained,
    };
    let mut keys = KeyDispatch {
        key_event,
        repr,
        context,
        state,
        decoder,
        live,
        env,
        allow_duplicate_single,
        dot_armed,
    };
    keys.dispatch_key(params)
}

/// 缓冲空闲时把菜单导航键留给宿主；缓冲前缀不可编辑，光标退化到 `0` 时钳到 `1`。
fn buffered_idle_guard(
    state: &SentenceState,
    context: &mut Context,
    repr: &str,
) -> Option<ProcessorResult> {
    // 缓冲空闲时把菜单导航键留给宿主。
    if !state.buffered_text.is_empty()
        && live_input(context).is_empty()
        && matches!(
            repr,
            "Tab" | "ISO_Left_Tab" | "Shift+Tab" | "Up" | "Down" | "Page_Up" | "Page_Down"
        )
    {
        return Some(ProcessorResult::Consume);
    }
    if !state.buffered_text.is_empty() && context.caret() < 1 {
        context.set_caret(1);
    }
    None
}

/// 参照 `_dotAfterDigitArmed`：先取待发值；非修饰键随后消耗它。
fn consume_dot_armed(env: &mut ProcessorEnv<'_>, repr: &str) -> bool {
    let dot_armed = *env.dot_armed;
    if !is_modifier_repr(repr) {
        *env.dot_armed = false;
    }
    dot_armed
}
