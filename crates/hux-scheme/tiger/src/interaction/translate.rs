// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;

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

/// 码注释（上游音反查件；当前 pin 的 main 未含，K3 音反查接线用）：单字显示全部编码（源序），词组逐字 `字:码组`。
pub fn code_comment(lexicon: &Lexicon, text: &str) -> Option<String> {
    if !lexicon.built {
        return None;
    }
    let chars: Vec<char> = text.chars().collect();
    if chars.is_empty() {
        return None;
    }
    if chars.len() == 1 {
        let codes = lexicon.character_codes.get(&chars[0].to_string())?;
        if codes.is_empty() {
            return None;
        }
        return Some(format!(" {}", codes.join(" / ")));
    }
    let mut parts = Vec::with_capacity(chars.len());
    for ch in &chars {
        match lexicon.character_codes.get(&ch.to_string()) {
            Some(codes) if !codes.is_empty() => {
                parts.push(format!("{}:{}", ch, codes.join("/")));
            }
            _ => parts.push(format!("{}:?", ch)),
        }
    }
    Some(format!(" {}", parts.join(" ")))
}

/// 参照 `translator(input, seg, env)`：解码产出候选（冷路径，无增量缓存；有锁时按锁播种）。
pub fn translate(
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
    let mut input = input;
    if !buffered.is_empty() {
        if seg_start != 0 || input.first() != Some(&b'~') {
            return Ok(());
        }
        input = &input[1..];
    }
    if !buffered.is_empty()
        && input.is_empty()
        && let Some(lock) = state.active_lock()
        && lock.raw == committed_raw
        && lock.text == committed_text
    {
        let mut candidate = Candidate::new("sentence_buffered", seg_start, seg_end, "", "");
        candidate.quality = 1000.0;
        candidate.preedit = buffered;
        out.push(candidate);
        return Ok(());
    }
    let mut raw = committed_raw.as_bytes().to_vec();
    raw.extend_from_slice(input);
    let raw_text = String::from_utf8_lossy(&raw).into_owned();
    let lock = state.active_lock().map(|lock| DecodeLock {
        raw: &lock.raw,
        text: &lock.text,
        boundaries: &lock.boundaries,
    });
    let decoded = decoder.decode_with_lock(&raw_text, false, &committed_text, lock)?;
    let mut yielded = 0usize;
    for item in &decoded.items {
        if !implicit_rank_allowed(
            item,
            &raw,
            state.continuation_after_auto_commit,
            allow_duplicate_single,
        ) {
            continue;
        }
        if !committed_text.is_empty() && !item.text.starts_with(&committed_text) {
            continue;
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
            continue;
        }
        let kind = if buffered.is_empty() {
            "sentence"
        } else {
            "sentence_buffered"
        };
        let mut candidate = Candidate::new(kind, seg_start, seg_end, &text, "");
        if !buffered.is_empty() {
            candidate.quality = 1000.0;
        }
        let separator = if !buffered.is_empty() && !preedit.is_empty() {
            " "
        } else {
            ""
        };
        candidate.preedit = format!("{buffered}{separator}{preedit}");
        out.push(candidate);
        yielded += 1;
        if yielded >= CANDIDATE_LIMIT {
            return Ok(());
        }
    }
    if yielded == 0 && !buffered.is_empty() {
        let mut candidate = Candidate::new(
            "sentence_buffered",
            seg_start,
            seg_end,
            &String::from_utf8_lossy(input),
            "",
        );
        candidate.quality = 1000.0;
        let separator = if input.is_empty() { "" } else { " " };
        candidate.preedit = format!("{buffered}{separator}{}", String::from_utf8_lossy(input));
        out.push(candidate);
    }
    Ok(())
}

/// 分段常量（参照 schema `speller/alphabet|initials|delimiter`；`finals` 未设置）。
pub(crate) const SEGMENTATION_ALPHABET: &str = "zyxwvutsrqponmlkjihgfedcba;';0123456789~";
pub(crate) const SEGMENTATION_INITIALS: &str = "abcdefghijklmnopqrstuvwxyz~";
pub(crate) const SEGMENTATION_DELIMITER: &str = " ";

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
        punct: Option<&mut PunctTable>,
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

/// 公共前缀字节长度。
pub(crate) fn common_prefix_length(left: &[u8], right: &[u8]) -> usize {
    left.iter()
        .zip(right.iter())
        .take_while(|(a, b)| a == b)
        .count()
}

/// 参照 `ConcreteEngine::CalculateSegmentation`。
pub(crate) fn calculate_segmentation(
    composition: &mut Composition,
    input: &[u8],
    caret: usize,
    prefixes: &[char],
    characters: &[char],
) {
    while !composition.has_finished_segmentation(input) {
        let start = composition.current_start_position();
        // 参照 segmentors 顺序：matcher → abc_segmentor → punct_segmentor → fallback。
        matcher(composition, input, prefixes, characters);
        abc_segmentor(composition, input);
        fallback_segmentor(composition, input);
        if start == composition.current_end_position() {
            break; // 无进展
        }
        if start >= caret {
            break; // 只允许 caret 之后一段
        }
        if !composition.has_finished_segmentation(input) {
            composition.forward();
        }
    }
    // 只在已确认组合末尾追加空段。
    composition.trim();
    if composition
        .back()
        .map(|segment| segment.selected)
        .unwrap_or(false)
    {
        composition.forward();
    }
}

/// 参照 `Matcher::Proceed`（`recognizer/patterns`）：活跃输入匹配
/// `^<前缀>[a-z]*'?$` 时，由本段独占剩余输入（标签 [`sound_to_char_shape::SOUND_TO_CHAR_SHAPE_TAG`]）。
pub(crate) fn matcher(
    composition: &mut Composition,
    input: &[u8],
    prefixes: &[char],
    characters: &[char],
) {
    let start = composition.confirmed_position();
    let Some(active) = input.get(start..) else {
        return;
    };
    // 字反查：活跃输入恰为一个触发字符（单字符段）。
    for character in characters {
        let mut buffer = [0u8; 4];
        if active == character.encode_utf8(&mut buffer).as_bytes() {
            while composition.current_start_position() > start {
                composition.segments.pop();
            }
            add_segment(composition, start, input.len(), &[char_to_sound_shape::TAG]);
            return;
        }
    }
    if prefixes.is_empty() {
        return;
    }
    if !prefixes
        .iter()
        .any(|prefix| sound_to_char_shape::matches_pattern(active, *prefix))
    {
        return;
    }
    // 参照 `GetMatch`：命中段必须覆盖到输入末尾；起点为当前末尾或既有段起点。
    if start != composition.current_end_position()
        && !composition
            .segments
            .iter()
            .any(|segment| segment.start == start)
    {
        return;
    }
    while composition.current_start_position() > start {
        composition.segments.pop();
    }
    add_segment(
        composition,
        start,
        input.len(),
        &[sound_to_char_shape::SOUND_TO_CHAR_SHAPE_TAG],
    );
}

/// 参照 `AbcSegmentor::Proceed`：从当前位置取最长合法拼写段。
pub(crate) fn abc_segmentor(composition: &mut Composition, input: &[u8]) {
    let start = composition.current_start_position();
    let mut end = start;
    let mut expecting_an_initial = true;
    while end < input.len() {
        let byte = input[end] as char;
        let is_letter = SEGMENTATION_ALPHABET.contains(byte);
        let is_delimiter = end != 0 && SEGMENTATION_DELIMITER.contains(byte);
        if !is_letter && !is_delimiter {
            break;
        }
        let is_initial = SEGMENTATION_INITIALS.contains(byte);
        let is_final = false; // schema 未设置 `speller/finals`
        if expecting_an_initial && !is_initial && !is_delimiter {
            break;
        }
        expecting_an_initial = is_final || is_delimiter;
        end += 1;
    }
    if start < end {
        add_segment(composition, start, end, &["abc"]);
    }
}

/// 参照 `FallbackSegmentor::Proceed`：无可拼写时生成（或延长）raw 段。
pub(crate) fn fallback_segmentor(composition: &mut Composition, input: &[u8]) {
    if composition.current_end_position() != composition.current_start_position() {
        return; // 本轮已有段
    }
    let k = composition.current_start_position();
    if k == input.len() {
        return;
    }
    composition.trim();
    if let Some(last) = composition.back_mut()
        && last.has_tag("raw")
    {
        last.end = k + 1;
        last.candidates.clear();
        last.selected_index = 0;
        last.translated = false;
        return;
    }
    composition.forward();
    add_segment(composition, k, k + 1, &["raw"]);
}

/// 参照 `Segmentation::AddSegment`：同起点段按长度取胜/覆盖/合并标签。
pub(crate) fn add_segment(composition: &mut Composition, start: usize, end: usize, tags: &[&str]) {
    if start != composition.current_start_position() {
        return;
    }
    if composition.segments.is_empty() {
        composition.segments.push(Segment {
            start,
            end,
            tags: tags.iter().map(|tag| tag.to_string()).collect(),
            ..Segment::default()
        });
        return;
    }
    let last = composition.segments.last_mut().expect("segment");
    if last.end > end {
        // 保留较长的旧段
    } else if last.end < end {
        *last = Segment {
            start,
            end,
            tags: tags.iter().map(|tag| tag.to_string()).collect(),
            ..Segment::default()
        };
    } else {
        for tag in tags {
            if !last.has_tag(tag) {
                last.tags.push(tag.to_string());
            }
        }
    }
}

/// 参照 `ConcreteEngine::TranslateSegments`：仅翻译未建立菜单的段。
pub(crate) fn translate_segments(
    decoder: &mut Decoder,
    context: &mut Context,
    state: &SentenceState,
    input: &[u8],
    mut punct: Option<&mut PunctTable>,
) -> anyhow::Result<()> {
    let prefixes = sound_to_char_shape_prefixes(context);
    let full_shape = context.get_option("full_shape");
    for index in 0..context.composition.segments.len() {
        let segment = &context.composition.segments[index];
        if segment.translated || segment.selected {
            continue;
        }
        let (start, end) = (segment.start.min(input.len()), segment.end.min(input.len()));
        if start >= end {
            let segment = &mut context.composition.segments[index];
            segment.translated = true;
            segment.candidates.clear();
            segment.selected_index = 0;
            continue;
        }
        if segment.has_tag(char_to_sound_shape::TAG) {
            // 默认可上屏候选：仅当触发字符来自**单字符键**（无 Ctrl/Alt/Super）时提供。
            let pressed = single_char(&input[start..end]);
            let candidates = match pressed.filter(|character| {
                char_to_sound_shape_triggers(context)
                    .iter()
                    .any(|key| single_char_trigger(key) == Some(*character))
            }) {
                Some(character) => sound_to_char_shape::punct_candidate(
                    punct.as_deref_mut(),
                    character,
                    full_shape,
                    start,
                    end,
                )
                .into_iter()
                .collect(),
                None => Vec::new(),
            };
            let segment = &mut context.composition.segments[index];
            segment.translated = true;
            segment.selected_index = 0;
            segment.candidates = candidates;
            continue;
        }
        if segment.has_tag(sound_to_char_shape::SOUND_TO_CHAR_SHAPE_TAG) {
            // 裸前缀（无编码）：默认可上屏候选**仅当触发字符来自单字符键**时提供；带修饰键无候选。
            let pressed = single_char(&input[start..end]);
            if pressed.is_some_and(|character| prefixes.contains(&character)) {
                let candidates = match pressed.filter(|character| {
                    sound_to_char_shape_triggers(context)
                        .iter()
                        .any(|key| single_char_trigger(key) == Some(*character))
                }) {
                    Some(character) => sound_to_char_shape::punct_candidate(
                        punct.as_deref_mut(),
                        character,
                        full_shape,
                        start,
                        end,
                    )
                    .into_iter()
                    .collect(),
                    None => Vec::new(),
                };
                let segment = &mut context.composition.segments[index];
                segment.translated = true;
                segment.selected_index = 0;
                segment.candidates = candidates;
                continue;
            }
            let slice = input[start..end].to_vec();
            let candidates = match prefixes
                .iter()
                .find(|prefix| sound_to_char_shape::matches_pattern(&slice, **prefix))
            {
                Some(prefix) => decoder.sound_to_char_shape_candidates(
                    &slice,
                    *prefix,
                    start,
                    end,
                    punct.as_deref_mut(),
                    full_shape,
                ),
                None => Vec::new(),
            };
            let segment = &mut context.composition.segments[index];
            segment.translated = true;
            segment.selected_index = 0;
            segment.prompt = if prefixes
                .iter()
                .any(|prefix| slice.first() == Some(&(*prefix as u8)))
            {
                sound_to_char_shape::SOUND_TO_CHAR_SHAPE_TIPS.to_string()
            } else {
                String::new()
            };
            segment.candidates = candidates;
            continue;
        }
        let mut candidates = Vec::new();
        translate(
            decoder,
            context,
            state,
            &input[start..end],
            start,
            end,
            &mut candidates,
        )?;
        let segment = &mut context.composition.segments[index];
        segment.translated = true;
        segment.selected_index = 0;
        segment.candidates = candidates;
    }
    Ok(())
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

/// 参照 `buffer_filter`：缓冲态只保留 `sentence_buffered` 候选。
pub fn buffer_filter(candidates: &[Candidate], buffered: bool) -> Vec<Candidate> {
    candidates
        .iter()
        .filter(|candidate| !buffered || candidate.kind == "sentence_buffered")
        .cloned()
        .collect()
}

/// 码注释过滤器（同上；K3 音反查接线用）：音反查段候选写入虎码注释。
pub fn code_comment_filter(candidates: &mut [Candidate], active: bool, lexicon: &Lexicon) {
    if !active {
        return;
    }
    for candidate in candidates {
        if let Some(comment) = code_comment(lexicon, &candidate.text) {
            candidate.comment = comment;
        }
    }
}

/// 参照 `ends_with_digit`。
pub fn ends_with_digit(text: &str) -> bool {
    let Some(last) = text.chars().last() else {
        return false;
    };
    last.is_ascii_digit() || ('\u{ff10}'..='\u{ff19}').contains(&last)
}
