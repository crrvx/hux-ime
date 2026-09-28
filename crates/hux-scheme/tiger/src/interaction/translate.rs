// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! 组合翻译门面：组合翻译、分段与逐段翻译、通知器的统一出口。
//!
//! 子模块 `composition`（组合翻译）、`segmentation`（分段器）与 `segments`（逐段翻译）
//! 在此按原名与原可见性重导出，`translate.rs` 的对外路径与可见性保持不变。

use super::*;

mod composition;
mod segmentation;
mod segments;

pub use composition::translate_composition;
pub(crate) use segmentation::{calculate_segmentation, common_prefix_length};
// 这四个分段器在原文即为 `pub(crate)`（crate 内除 `calculate_segmentation` 外无调用者，
// 域外无引用）。按原名转出以保持 `translate::` 路径与可见性不变，故此处显式允许未使用导入。
#[allow(unused_imports)]
pub(crate) use segmentation::{abc_segmentor, add_segment, fallback_segmentor, matcher};
pub(crate) use segments::translate_segments;

/// 参照 `trim_segmented_after_raw_prefix`：去掉前 `raw_prefix_length` 个原始字符
/// 对应的片段（片段为 ASCII，按字节计数即可）。
pub fn trim_segmented_after_raw_prefix(segmented: &str, raw_prefix_length: usize) -> String {
    if raw_prefix_length == 0 || segmented.is_empty() {
        return segmented.to_string();
    }
    let bytes = segmented.as_bytes();
    let mut raw_count = 0usize;
    let mut index = 0usize;
    while index < bytes.len() && raw_count < raw_prefix_length {
        if bytes[index] != b' ' {
            raw_count += 1;
        }
        index += 1;
    }
    while index < bytes.len() && bytes[index] == b' ' {
        index += 1;
    }
    if index < bytes.len() {
        segmented[index..].to_string()
    } else {
        String::new()
    }
}

/// 分段常量（参照 schema `speller/alphabet|initials|delimiter`；`finals` 未设置）。
///
/// `delimiter` 追踪**反查分支尖端 `92a0b54`** 的 `" '"`（主干 pin `abad411` 为 `" "`）：
/// 撇号即 [`crate::sound_to_char_shape::SYLLABLE_DELIMITER`]（**断音**）⇒ 段内 `'` 之后必须是
/// 首字母，数字/`;` 在此断开（本仓 `speller/finals` 未设置，故只有「断段」效果，不做音节重拼）。
///
/// 该口径与主干 pin 的 `key_sequence` 金样在「`'` + 数字/`;`」序列上**确有可见差异**：
/// 本仓末段是 raw 段（无菜单）⇒ `Up`/`Down`/`Page_*` 不被消费，上游主干单段 abc ⇒ 消费。
/// 已按期望值登记在 `tests/key_sequence_differential.rs` 的 `DEVIATIONS`
/// （种类 `BranchPinDelimiter`，用例 `apostrophe_digit_page`/`apostrophe_semicolon_page`）。
pub(crate) const SEGMENTATION_ALPHABET: &str = "zyxwvutsrqponmlkjihgfedcba;';0123456789~";
pub(crate) const SEGMENTATION_INITIALS: &str = "abcdefghijklmnopqrstuvwxyz~";
pub(crate) const SEGMENTATION_DELIMITER: &str = " '";

/// 组合重建器：参照 `ConcreteEngine::Compose`（分段输入随光标；增量重置保留未变段的
/// 菜单与高亮），供宿主在每次按键后调用。
#[derive(Clone, Debug, Default)]
pub struct CompositionBuilder {
    /// 当前分段输入（参照 `Segmentation::input_`）。
    built_input: Vec<u8>,
}

impl CompositionBuilder {
    /// 重建组合：执行重置（按公共前缀丢弃段）→ 分段（abc/raw）→ 翻译未翻译段。
    /// `invalidated` 表示本次按键发生过提交（提交会重建翻译，旧段不复用）。
    pub fn rebuild(
        &mut self,
        decoder: &mut Decoder,
        context: &mut Context,
        state: &SentenceState,
        invalidated: bool,
        punct: Option<&PunctTable>,
    ) -> anyhow::Result<bool> {
        let input = context.input().to_vec();
        let caret = context.caret().min(input.len());
        if invalidated {
            // 提交重建了翻译（参照：提交后 `Compose` 以新输入重新分段，旧段不再复用）。
            context.composition.segments.clear();
        }
        // 参照 Compose：常态分段输入为 caret 之前的前缀。
        let caret_input = input[..caret].to_vec();
        self.apply_reset(context, &caret_input);
        // `caret < input.len() && caret == 已确认位置`：翻译到 caret 之后一段（完整输入）。
        if caret < input.len() && context.composition.confirmed_position() == caret {
            self.apply_reset(context, &input);
        }
        let seg_input = self.built_input.clone();
        let prefixes = sound_to_char_shape_prefixes(context);
        let characters = char_to_sound_shape_keys(context);
        calculate_segmentation(
            &mut context.composition,
            &seg_input,
            caret,
            &prefixes,
            &characters,
        );
        translate_segments(decoder, context, state, &seg_input, punct)?;
        Ok(true)
    }

    /// 参照 `Segmentation::Reset`：按新旧输入的公共前缀丢弃段，必要时追加空尾段。
    pub(crate) fn apply_reset(&mut self, context: &mut Context, new_input: &[u8]) {
        let diff_pos = common_prefix_length(&self.built_input, new_input);
        let mut disposed = false;
        while context
            .composition
            .segments
            .last()
            .map(|segment| segment.end > diff_pos)
            .unwrap_or(false)
        {
            context.composition.segments.pop();
            disposed = true;
        }
        if disposed {
            context.composition.forward();
        }
        self.built_input = new_input.to_vec();
    }

    /// 清空记录（会话重置；下次调用必重建）。
    pub fn reset(&mut self) {
        self.built_input.clear();
    }
}

/// 参照 update 通知器（`live.update_connection`）：非组合清暂存；缓冲且实况为空时隐藏候选。
/// 提交落库由宿主另行处理。
pub fn update_notifier(context: &mut Context, state: &mut SentenceState, live: &mut LiveLearning) {
    if !context.is_composing() {
        live.pending.clear();
        live.baseline = None;
        live.submitted_raw = None;
        if !buffered_text(context).is_empty() {
            state.reset(context, false);
        }
    }
    let hide = !buffered_text(context).is_empty() && context.live_input().is_empty();
    if hide || live.hide_owned {
        live.hide_owned = hide;
        if context.get_option("_hide_candidate") != hide {
            context.set_option("_hide_candidate", hide);
        }
    }
}

/// 参照 `ends_with_digit`：**选重数字表**（半角 + 全角数字）判定提交文本末字符。
///
/// 与断音（[`crate::sound_to_char_shape::SYLLABLE_DELIMITER`]）、以及 raw 输入里的选重后缀
/// （[`crate::decode::has_selection_suffix`]：分号/撇号/半角数字，判定对象是原始字节）都不合并：
/// 三者判定的对象与字符集均不同。
pub fn ends_with_digit(text: &str) -> bool {
    let Some(last) = text.chars().last() else {
        return false;
    };
    last.is_ascii_digit() || ('\u{ff10}'..='\u{ff19}').contains(&last)
}
