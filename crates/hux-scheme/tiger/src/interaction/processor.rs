// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;
use hux_core::host::{self, HostOptions};

// ---------------------------------------------------------------- ascii 策略

// ---------------------------------------------------------------- 处理器

/// 处理器宿主环境（对应参照 `env` 的非会话部分；内存 / 词库 / 选项由宿主层补）。
pub struct ProcessorEnv<'a> {
    /// 参照 `os.time()`（学习事件时间戳）。
    pub now: f64,
    /// 参照 `env._tiger_sentence_dot_armed`（数字后小数点待发）。
    pub dot_armed: &'a mut bool,
    /// 参照 `get_min_retained_raw_length(env)` 的配置值。
    pub min_retained: Option<i64>,
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

/// 数字直选位置（0-based）：`1`–`9` → `0`–`8`，`0` → `9`（第 10 个）。
pub(crate) fn digit_page_position(ch: char) -> Option<usize> {
    match ch {
        '1'..='9' => Some(ch as usize - '1' as usize),
        '0' => Some(9),
        _ => None,
    }
}

/// 数字直选（`DigitSelect`，addon 扩展）：选择当前页第 `position`（0-based）个候选，
/// 走与 `space` 相同的确认/学习链并直接上屏；候选不在页内时不消费（交回普通数字处理）。
pub(crate) fn select_page_candidate(
    decoder: &mut Decoder,
    context: &mut Context,
    state: &mut SentenceState,
    live: &mut LiveLearning,
    now: f64,
    page_size: usize,
    position: usize,
) -> anyhow::Result<bool> {
    let page_size = page_size.max(1);
    if position >= page_size {
        return Ok(false);
    }
    let Some(segment) = context.composition.back() else {
        return Ok(false);
    };
    let page_start = (segment.selected_index / page_size) * page_size;
    select_candidate_at(decoder, context, state, live, now, page_start + position)
}

/// 候选点击 / 数字直选共用：按**全局索引**选中候选，走与 `space` 相同的确认链
/// 并直接上屏（对齐参照 `ConcreteEngine::OnSelect` + `RimeState::selectCandidate`：
/// 点选后提交整个组合）。索引越界（候选未生成）或无可选段时返回 `false`。
///
/// 学习：候选点击在参照里**只经提交通知器**一次「暂存 + 提交」
/// （`lua/tiger_sentence.lua` 的 `commit_notifier` 回调；参照的候选点击不经过方案处理器），
/// 故这里**不得**再显式 `learning_stage` 一次——否则同一次点击会在 `pending` 里留下
/// 两条完全相同的成对偏好事件（`learning_submit` 全部接受 ⇒ 权重记两次）。
/// 对照：参照 `space`/标点分支确有「处理器先暂存 + 通知器再暂存」的两段（本仓照搬），
/// 反查段数字直选则同样只走通知器一次。
pub fn select_candidate_at(
    decoder: &mut Decoder,
    context: &mut Context,
    state: &mut SentenceState,
    live: &mut LiveLearning,
    now: f64,
    index: usize,
) -> anyhow::Result<bool> {
    {
        let Some(segment) = context.composition.back() else {
            return Ok(false);
        };
        if index >= segment.prepare(index + 1) {
            return Ok(false);
        }
    }
    context.highlight(index);
    confirm_selection(
        Some(&mut LearningCommit {
            decoder: &mut *decoder,
            live: &mut *live,
            now,
        }),
        context,
        state,
    );
    live.pending.clear();
    live.baseline = None;
    state.reset(context, false);
    Ok(true)
}

/// 参照 `processor(key_event, env)`。宿主职责（内存配置、词库懒加载、选项同步、
/// 学习库存储）由调用方在进入前完成。
pub fn processor(
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
    // 触发键（音反查 / 字反查）：空闲时进入组合、段内再按则退出（同参照的标签语义）。
    for (triggers, tag) in [
        (
            sound_to_char_shape_triggers(context),
            sound_to_char_shape::SOUND_TO_CHAR_SHAPE_TAG,
        ),
        (
            char_to_sound_shape_triggers(context),
            char_to_sound_shape::TAG,
        ),
    ] {
        let Some(configured) = triggers
            .iter()
            .find(|configured| key_matches(key_event, configured))
        else {
            continue;
        };
        // 触发字符取命中键实际产生的字符（多触发键各自字符可不同）。
        let Some(prefix) = key_char(configured) else {
            continue;
        };
        let active = context
            .composition
            .back()
            .is_some_and(|segment| segment.has_tag(tag));
        if active {
            context.clear();
            return Ok(ProcessorResult::Consume);
        }
        if !context.is_composing() {
            let mut buffer = [0u8; 4];
            context.push_input(prefix.encode_utf8(&mut buffer).as_bytes());
            return Ok(ProcessorResult::Consume);
        }
        // 组合中：交由后续处理器（标点等）处理。
    }
    // 参照处理器链 `recognizer`（位于 speller/标点之前）：音反查段内继续接受模式内按键。
    if context
        .composition
        .back()
        .is_some_and(|segment| segment.has_tag(sound_to_char_shape::SOUND_TO_CHAR_SHAPE_TAG))
        && let Some(ch) = recognizer_char(key_event)
    {
        let prefixes = sound_to_char_shape_prefixes(context);
        let mut next = context.input().to_vec();
        next.push(ch as u8);
        if prefixes
            .iter()
            .any(|prefix| sound_to_char_shape::matches_pattern(&next, *prefix))
        {
            context.push_input(&[ch as u8]);
            return Ok(ProcessorResult::Consume);
        }
    }
    let repr = key_event.repr();
    let repr = repr.as_str();
    let allow_duplicate_single = set_allow_duplicate_single(context);
    decoder.set_allow_duplicate_single(allow_duplicate_single);
    live.submitted_raw = None;
    // 缓冲空闲时把菜单导航键留给宿主。
    if !state.buffered_text.is_empty()
        && live_input(context).is_empty()
        && matches!(
            repr,
            "Tab" | "ISO_Left_Tab" | "Shift+Tab" | "Up" | "Down" | "Page_Up" | "Page_Down"
        )
    {
        return Ok(ProcessorResult::Consume);
    }
    if !state.buffered_text.is_empty() && context.caret() < 1 {
        context.set_caret(1);
    }
    if !context.is_composing() {
        live.pending.clear();
        live.baseline = None;
    }
    // 参照 `_dotAfterDigitArmed`：先取待发值；非修饰键随后消耗它。
    let dot_armed = *env.dot_armed;
    if !is_modifier_repr(repr) {
        *env.dot_armed = false;
    }
    let params = EarlyCommitParams {
        allow_duplicate_single,
        generation: state.model_generation,
        min_retained: min_retained_raw_length(env.min_retained),
    };
    if let Some(ch) = is_plain_char_key(key_event, repr) {
        // 字反查段：其它普通键先清空组合，随后照常处理该键。
        if context
            .composition
            .back()
            .is_some_and(|segment| segment.has_tag(char_to_sound_shape::TAG))
        {
            context.clear();
        }
        if !context.is_composing()
            && (!state.committed_raw.is_empty()
                || !state.last_seen_raw.is_empty()
                || !state.trackers.is_empty()
                || state.suspended
                || state.continuation_after_auto_commit
                || state.active_lock().is_some()
                || state.tab_pending)
        {
            state.reset(context, false);
        }
        // 分号/引号只在组合中作 rank 选择器；空闲时交标点处理器。
        if !context.is_composing() && (ch == ';' || ch == '\'') {
            return Ok(ProcessorResult::Forward);
        }
        if live_input(context).len() >= MAX_RAW_LENGTH {
            return Ok(ProcessorResult::Consume);
        }
        let is_letter = ch.is_ascii_lowercase();
        // 音反查段（`` ` `` 前缀）不得把选择键并入拼音：拼写表会把它追加进输入并打断
        // 反查段。数字在此按**上游的绝对索引**（`index = digit - 1`，越界惰性消费）
        // 高亮 + 确认后提交，分号惰性；撇号由识别模式放行（是否参与音节切分见
        // `sound_to_char_shape::matches_pattern` 的说明）。
        //
        // 该判据必须**先于**下方的 addon 数字直选：后者是**页相对**落点
        // （`page_start + position`），二者仅在「反查段菜单停在第 1 页且
        // `page_size >= digit`」时巧合一致；菜单翻到第 2 页起（或 `page_size >= 10`）
        // 时上游按绝对索引选、addon 按本页位置选，结果不同。
        // 参照 `lua/tiger_sentence.lua` @ `92a0b54`（`local index = tonumber(ch) - 1`）；
        // 上游该分支位于 `max_raw_length` 早退与「空闲数字直接上屏」之后，
        // 本仓同序（空闲数字要求 `!is_composing`，与反查段互斥）。
        if !is_letter
            && context.composition.back().is_some_and(|segment| {
                segment.has_tag(sound_to_char_shape::SOUND_TO_CHAR_SHAPE_TAG)
            })
        {
            if ch.is_ascii_digit() {
                live.pending.clear();
                live.baseline = None;
                let index = (ch as u8 - b'0') as isize - 1;
                let count = context
                    .composition
                    .back()
                    .map(|segment| segment.candidates.len())
                    .unwrap_or(0);
                if index >= 0 && (index as usize) < count {
                    // 高亮 + 确认与 Space 同路（`Context::select` 可能提交整句）。
                    context.highlight(index as usize);
                    confirm_selection(
                        Some(&mut LearningCommit {
                            decoder: &mut *decoder,
                            live: &mut *live,
                            now: env.now,
                        }),
                        context,
                        state,
                    );
                }
                state.reset(context, false);
                return Ok(ProcessorResult::Consume);
            }
            if ch == ';' {
                return Ok(ProcessorResult::Consume);
            }
        }
        // 数字直选（`OPTION_DIGIT_SELECT`；addon 扩展）：菜单可见时直接上屏当前页候选。
        if context.get_option(OPTION_DIGIT_SELECT)
            && ch.is_ascii_digit()
            && context.has_menu()
            && let Some(position) = digit_page_position(ch)
            && select_page_candidate(
                decoder,
                context,
                state,
                live,
                env.now,
                env.page_size,
                position,
            )?
        {
            return Ok(ProcessorResult::Consume);
        }
        // 空闲数字直接上屏（全角选项下为全角）。
        if ch.is_ascii_digit() && !context.is_composing() {
            if context.get_option("full_shape") {
                const FULL_SHAPE_DIGITS: [char; 10] =
                    ['０', '１', '２', '３', '４', '５', '６', '７', '８', '９'];
                let index = (ch as u8 - b'0') as usize;
                context.direct_commit(&FULL_SHAPE_DIGITS[index].to_string());
            } else {
                context.direct_commit(&ch.to_string());
            }
            *env.dot_armed = true;
            return Ok(ProcessorResult::Consume);
        }
        let live_before = live_input(context);
        let caret = input_caret(context);
        let mut full_before = state.committed_raw.as_bytes().to_vec();
        full_before.extend_from_slice(&live_before);
        if caret != live_before.len() {
            live.pending.clear();
            live.baseline = None;
            invalidate_edit_state(
                context,
                state,
                state.committed_raw.len() + caret,
                full_before.len() + ch.len_utf8(),
            );
            context.push_input(ch.to_string().as_bytes());
            return Ok(ProcessorResult::Consume);
        }
        if state.tab_pending && is_letter {
            let target = context
                .composition
                .back()
                .map(|segment| segment.selected_index)
                .unwrap_or(0);
            let lock = state.active_lock().map(|lock| DecodeLock {
                raw: &lock.raw,
                text: &lock.text,
                boundaries: &lock.boundaries,
            });
            let raw_text = String::from_utf8_lossy(&full_before).into_owned();
            let decoded =
                decoder.decode_with_lock(&raw_text, false, &state.committed_text, lock)?;
            let mut candidate: Option<Selected> = None;
            let mut seen: Vec<FusionAhead> = Vec::new();
            let mut visible = 0usize;
            for item in &decoded.items {
                if implicit_rank_allowed(
                    item,
                    &full_before,
                    state.continuation_after_auto_commit,
                    allow_duplicate_single,
                ) && item.text.starts_with(&state.committed_text)
                    && item.text.len() > state.committed_text.len()
                {
                    if visible == target {
                        let (raw_length, diff) = decoder.path_summary(item);
                        candidate = Some(Selected {
                            text: item.text.clone(),
                            raw_length,
                            diff,
                            buffered_fallback: false,
                            source_mask: item.source_mask,
                            // 参照：命中即 `break`，`_fusion_ahead` = 此前通过过滤的候选。
                            fusion_ahead: seen,
                        });
                        break;
                    }
                    seen.push(FusionAhead {
                        text: item.text.clone(),
                        source_mask: item.source_mask,
                    });
                    visible += 1;
                }
            }
            if let Some(candidate) = candidate {
                if candidate.raw_length > state.committed_raw.len() {
                    // 参照 `processor` 的 Tab 确认分支：**先** stage、**再**清 `tab_pending`。
                    // `learning_stage` 以该标志选择基线（`tab_pending and live.baseline or
                    // submitted_first`），且 `reinforce_eligible` 要求 `!tab_pending`；
                    // 清标志后再 stage 会把基线取成 `submitted_first` 并误走 reinforce 路线。
                    // 参照此处不传 `submitted_first`（nil）；清理后分支即 `return`，本调用不与
                    // 提交点的 `learning_commit` 重复（后者对应参照的 commit 通知器，参照同样会走）。
                    learning_stage(live, state, Some(&candidate), &full_before, None, env.now);
                    state.tab_pending = false;
                    let boundaries: String = candidate
                        .diff
                        .path
                        .iter()
                        .map(|node| format!("{},{};", node.raw_length, node.text_length))
                        .collect();
                    let locked_raw =
                        String::from_utf8_lossy(&full_before[..candidate.raw_length]).into_owned();
                    state.locks.push(Lock {
                        raw: locked_raw.clone(),
                        text: candidate.text.clone(),
                        boundaries,
                    });
                    let commit = if context.get_option(OPTION_EARLY_COMMIT) {
                        let commit = candidate.text[state.committed_text.len()..].to_string();
                        state.committed_text = candidate.text.clone();
                        state.committed_raw = locked_raw;
                        Some(commit)
                    } else {
                        None
                    };
                    reset_early_evidence(state);
                    state.empty_code_pending = None;
                    state.suspended = false;
                    state.continuation_after_auto_commit = false;
                    state.save(context);
                    if let Some(commit) = commit {
                        if let Some(text) = submit_early(context, state, &commit) {
                            LearningCommit {
                                decoder: &mut *decoder,
                                live: &mut *live,
                                now: env.now,
                            }
                            .commit_with_learning(
                                context,
                                state,
                                &text,
                                &candidate.text,
                                candidate.raw_length,
                            );
                        }
                    }
                    let mut restored = full_before[state.committed_raw.len()..].to_vec();
                    restored.extend_from_slice(ch.to_string().as_bytes());
                    restore_composition_input(context, &restored);
                    return Ok(ProcessorResult::Consume);
                }
            }
        }
        state.tab_pending = false;
        live.baseline = None;
        if !is_letter {
            state.empty_code_pending = None;
            state.save(context);
        }
        context.push_input(ch.to_string().as_bytes());
        if is_letter
            && try_empty_code_commit(
                &mut LearningCommit {
                    decoder: &mut *decoder,
                    live: &mut *live,
                    now: env.now,
                },
                context,
                state,
                &full_before,
                ch.to_string().as_bytes(),
                params,
                env.dot_armed,
            )?
        {
            return Ok(ProcessorResult::Consume);
        }
        try_early_commit(
            &mut LearningCommit {
                decoder: &mut *decoder,
                live: &mut *live,
                now: env.now,
            },
            context,
            state,
            params,
            env.dot_armed,
        )?;
        return Ok(ProcessorResult::Consume);
    }
    if !context.is_composing() {
        if dot_armed
            && (repr == "period" || repr == "KP_Decimal")
            && !key_event.shift()
            && !key_event.ctrl()
            && !key_event.alt()
            && !key_event.super_modifier()
        {
            context.direct_commit(".");
            return Ok(ProcessorResult::Consume);
        }
        return Ok(ProcessorResult::Forward);
    }
    let codepoint = key_event.keycode;
    // 菜单可见（不必处于缓冲态）时遇可打印标点：先按当前选中项暂存学习、确认组合，
    // 再把原键交标点处理器（参照 `abad411`：标点段一旦追加进组合，
    // `learning_selection` 就再也不能解码该输入——例如 `zhhbi,`——或取回句子的选中项）。
    //
    // **本仓有意偏离上游 `abad411`**：
    // 上游对该分支内的**所有**可打印 ASCII 标点一律先确认组合再交标点表，于是宿主
    // `key_binder` 的翻页绑定（缺省 `-`/`=`，以及 schema 绑到翻页的 `[`/`]`）被永久遮蔽
    // （最小复现 `j a equal`：期望翻页，实际提交「一=」）。此处先问**与宿主同一套**翻页判据
    // [`host::paging_action`]：会被判为翻页的键不消费、落回宿主链执行翻页；其余标点维持上游行为。
    if context.has_menu()
        && (33..=126).contains(&codepoint)
        && (codepoint as u8 as char).is_ascii_punctuation()
        && !key_event.ctrl()
        && !key_event.alt()
        && !key_event.super_modifier()
        && host::paging_action(context, env.host_options, key_event).is_none()
    {
        let selection = learning_selection(decoder, context, state)?;
        learning_stage(
            live,
            state,
            selection.selected.as_ref(),
            &selection.raw,
            None,
            env.now,
        );
        confirm_selection(
            Some(&mut LearningCommit {
                decoder: &mut *decoder,
                live: &mut *live,
                now: env.now,
            }),
            context,
            state,
        );
        return Ok(ProcessorResult::Forward);
    }
    if repr == "Return" || repr == "KP_Enter" {
        live.pending.clear();
        live.baseline = None;
        let text = format!(
            "{}{}",
            state.buffered_text,
            String::from_utf8_lossy(&live_input(context))
        );
        context.direct_commit(&text);
        context.clear();
        state.reset(context, false);
        return Ok(ProcessorResult::Consume);
    }
    if repr == "Escape" {
        live.pending.clear();
        live.baseline = None;
        context.clear();
        state.reset(context, false);
        return Ok(ProcessorResult::Consume);
    }
    if repr == "BackSpace" || repr == "Delete" {
        live.pending.clear();
        live.baseline = None;
        state.tab_pending = false;
        reset_early_evidence(state);
        state.empty_code_pending = None;
        if !state.buffered_text.is_empty() {
            let raw = live_input(context);
            let caret = input_caret(context);
            if repr == "BackSpace" && raw.is_empty() {
                let mut letters: Vec<char> = state.buffered_text.chars().collect();
                let removed = letters.pop();
                state.buffered_text = letters.into_iter().collect();
                let removed_length = removed.map(char::len_utf8).unwrap_or(0);
                let mut keep = state.committed_text.len().saturating_sub(removed_length);
                // 属性可能来自旧版本/外部：仅在字符边界上截断，避免 panic。
                while keep > 0 && !state.committed_text.is_char_boundary(keep) {
                    keep -= 1;
                }
                state.committed_text.truncate(keep);
                if state.buffered_text.is_empty() {
                    state.reset(context, false);
                } else {
                    state.locks = vec![Lock {
                        raw: state.committed_raw.clone(),
                        text: state.committed_text.clone(),
                        boundaries: format!(
                            "{},{};",
                            state.committed_raw.len(),
                            state.committed_text.len()
                        ),
                    }];
                    state.save(context);
                }
                restore_composition_input(context, &raw);
                return Ok(ProcessorResult::Consume);
            }
            let first = if repr == "BackSpace" {
                caret as isize - 1
            } else {
                caret as isize
            };
            if first < 0 || first >= raw.len() as isize {
                return Ok(ProcessorResult::Consume);
            }
            let first = first as usize;
            let mut remaining = raw[..first].to_vec();
            remaining.extend_from_slice(&raw[first + 1..]);
            invalidate_edit_state(
                context,
                state,
                state.committed_raw.len() + first,
                state.committed_raw.len() + remaining.len(),
            );
            restore_composition_input(context, &remaining);
            context.set_caret(first + 1);
            return Ok(ProcessorResult::Consume);
        }
        if state.active_lock().is_some() {
            let raw = live_input(context);
            let caret = input_caret(context);
            let first = if repr == "BackSpace" {
                caret as isize - 1
            } else {
                caret as isize
            };
            if first < 0 || first >= raw.len() as isize {
                state.save(context);
                return Ok(ProcessorResult::Forward);
            }
            let first = first as usize;
            let mut remaining = raw[..first].to_vec();
            remaining.extend_from_slice(&raw[first + 1..]);
            invalidate_edit_state(
                context,
                state,
                state.committed_raw.len() + first,
                state.committed_raw.len() + remaining.len(),
            );
            if remaining.is_empty() {
                context.clear();
                state.reset(context, false);
            } else if repr == "BackSpace" {
                context.pop_input(1);
            } else {
                context.delete_input(1);
            }
            return Ok(ProcessorResult::Consume);
        }
        state.save(context);
        return Ok(ProcessorResult::Forward);
    }
    if repr == "Left" || repr == "Right" || repr == "Home" || repr == "End" {
        live.pending.clear();
        live.baseline = None;
        // 手动光标导航不保留追加证据与待确认 Tab。
        state.tab_pending = false;
        reset_early_evidence(state);
        state.empty_code_pending = None;
        state.save(context);
        if !state.buffered_text.is_empty()
            && (repr == "Home" || (repr == "Left" && input_caret(context) == 0))
        {
            context.set_caret(1);
            return Ok(ProcessorResult::Consume);
        }
        return Ok(ProcessorResult::Forward);
    }
    if repr == "Tab" || repr == "ISO_Left_Tab" || repr == "Shift+Tab" {
        if !state.tab_pending && live.store_ready {
            let selection = learning_selection(decoder, context, state)?;
            live.baseline = selection.first;
        }
        reset_early_evidence(state);
        state.suspended = true;
        state.empty_code_pending = None;
        state.save(context);
        if cycle_candidate_highlight(context, if repr == "Tab" { 1 } else { -1 }) {
            state.tab_pending = true;
            state.save(context);
            return Ok(ProcessorResult::Consume);
        }
        // 菜单不可用时交给 schema 的 Down/Up 绑定与 navigate。
        return Ok(ProcessorResult::Forward);
    }
    if repr == "Up" || repr == "Down" || repr == "Page_Up" || repr == "Page_Down" {
        reset_early_evidence(state);
        state.suspended = true;
        state.empty_code_pending = None;
        state.save(context);
        return Ok(ProcessorResult::Forward);
    }
    if repr == "space" {
        if context.has_menu() {
            let selection = learning_selection(decoder, context, state)?;
            learning_stage(
                live,
                state,
                selection.selected.as_ref(),
                &selection.raw,
                None,
                env.now,
            );
            confirm_selection(
                Some(&mut LearningCommit {
                    decoder: &mut *decoder,
                    live: &mut *live,
                    now: env.now,
                }),
                context,
                state,
            );
        }
        live.pending.clear();
        live.baseline = None;
        state.reset(context, false);
        return Ok(ProcessorResult::Consume);
    }
    Ok(ProcessorResult::Forward)
}
