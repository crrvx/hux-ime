// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! 组合翻译：把解码产出翻译为分段候选（[`translate_composition`] 及其三个阶段）。
//!
//! 由父模块 `translate` 按原名与可见性重导出。

use super::*;

/// 缓冲段前置判定的两态结果。
enum BufferedOutcome {
    /// 锁命中：缓冲候选已产出，本段结束。
    Handled,
    /// 本段不翻译（音反查或缓冲前缀不符）。
    Skip,
}

/// 参照 `translator(input, seg, env)`：解码产出候选（冷路径，无增量缓存；有锁时按锁播种）。
pub fn translate_composition(
    decoder: &mut Decoder,
    context: &Context,
    state: &SentenceState,
    input: &[u8],
    seg_start: usize,
    seg_end: usize,
    out: &mut Vec<Candidate>,
) -> anyhow::Result<()> {
    if input.first() == Some(&b'`') {
        return Ok(()); // 音反查段（` 前缀）由 `sound_to_char_shape` 模块处理，本翻译不产出候选
    }
    let allow_duplicate_single = set_allow_duplicate_single(context);
    decoder.set_allow_duplicate_single(allow_duplicate_single);
    let committed_text = state.committed_text.clone();
    let committed_raw = state.committed_raw.clone();
    let buffered = state.buffered_text.clone();
    let mut encoded = input;
    match prepare_buffered(
        encoded,
        &buffered,
        &committed_raw,
        &committed_text,
        state,
        (seg_start, seg_end),
        out,
    ) {
        Some(BufferedOutcome::Handled | BufferedOutcome::Skip) => return Ok(()),
        None => {}
    }
    if !buffered.is_empty() {
        encoded = &encoded[1..];
    }
    let mut raw = committed_raw.as_bytes().to_vec();
    raw.extend_from_slice(encoded);
    let raw_text = String::from_utf8_lossy(&raw).into_owned();
    let lock = state.active_lock().map(|lock| DecodeLock {
        raw: &lock.raw,
        text: &lock.text,
        boundaries: &lock.boundaries,
    });
    let decoded = decoder.decode_with_lock(&raw_text, false, &committed_text, lock)?;
    emit_sentence_candidates(
        &decoded.items,
        &raw,
        state,
        &committed_raw,
        &committed_text,
        encoded,
        &buffered,
        allow_duplicate_single,
        (seg_start, seg_end),
        out,
    );
    Ok(())
}

/// 缓冲段前置判定；`Some` = 本段处理完毕（锁命中已产出候选 / 本段不翻译）。
fn prepare_buffered(
    mut input: &[u8],
    buffered: &str,
    committed_raw: &str,
    committed_text: &str,
    state: &SentenceState,
    span: (usize, usize),
    out: &mut Vec<Candidate>,
) -> Option<BufferedOutcome> {
    let (seg_start, seg_end) = span;
    if !buffered.is_empty() {
        if seg_start != 0 || input.first() != Some(&b'~') {
            return Some(BufferedOutcome::Skip);
        }
        input = &input[1..];
    }
    if !buffered.is_empty()
        && input.is_empty()
        && let Some(lock) = state.active_lock()
        && lock.raw == committed_raw
        && lock.text == committed_text
    {
        let mut candidate = Candidate::new(KIND_SENTENCE_BUFFERED, seg_start, seg_end, "", "");
        candidate.preedit = buffered.to_string();
        out.push(candidate);
        return Some(BufferedOutcome::Handled);
    }
    None
}

/// 单个解码条目产出候选；返回 `None` 表示本条目被过滤（原 `continue`）。
#[allow(clippy::too_many_arguments)]
fn emit_item(
    item: &Evaluated,
    raw: &[u8],
    state: &SentenceState,
    committed_raw: &str,
    committed_text: &str,
    buffered: &str,
    allow_duplicate_single: bool,
    span: (usize, usize),
) -> Option<Candidate> {
    let (seg_start, seg_end) = span;
    if !implicit_rank_allowed(
        item,
        raw,
        state.continuation_after_auto_commit,
        allow_duplicate_single,
    ) {
        return None;
    }
    if !committed_text.is_empty() && !item.text.starts_with(committed_text) {
        return None;
    }
    let text = if committed_text.is_empty() {
        item.text.clone()
    } else {
        item.text[committed_text.len()..].to_string()
    };
    let mut preedit = item.segmented.clone();
    if !committed_raw.is_empty() {
        preedit = trim_segmented_after_raw_prefix(&item.segmented, committed_raw.len());
    }
    if text.is_empty() && buffered.is_empty() {
        return None;
    }
    let kind = if buffered.is_empty() {
        "sentence"
    } else {
        KIND_SENTENCE_BUFFERED
    };
    let mut candidate = Candidate::new(kind, seg_start, seg_end, &text, "");
    let separator = if !buffered.is_empty() && !preedit.is_empty() {
        " "
    } else {
        ""
    };
    candidate.preedit = format!("{buffered}{separator}{preedit}");
    Some(candidate)
}

/// 解码产出候选主体：逐条产出，达到上限即停止。
#[allow(clippy::too_many_arguments)]
fn emit_sentence_candidates(
    items: &[Evaluated],
    raw: &[u8],
    state: &SentenceState,
    committed_raw: &str,
    committed_text: &str,
    input: &[u8],
    buffered: &str,
    allow_duplicate_single: bool,
    span: (usize, usize),
    out: &mut Vec<Candidate>,
) {
    let (seg_start, seg_end) = span;
    let mut yielded = 0usize;
    for item in items {
        let Some(candidate) = emit_item(
            item,
            raw,
            state,
            committed_raw,
            committed_text,
            buffered,
            allow_duplicate_single,
            span,
        ) else {
            continue;
        };
        out.push(candidate);
        yielded += 1;
        if yielded >= crate::decode::CANDIDATE_LIMIT {
            return;
        }
    }
    if yielded == 0 && !buffered.is_empty() {
        let mut candidate = Candidate::new(
            KIND_SENTENCE_BUFFERED,
            seg_start,
            seg_end,
            &String::from_utf8_lossy(input),
            "",
        );
        let separator = if input.is_empty() { "" } else { " " };
        candidate.preedit = format!("{buffered}{separator}{}", String::from_utf8_lossy(input));
        out.push(candidate);
    }
}
