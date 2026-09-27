// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;
use hux_core::host::{self, HostOptions};

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
    if let Some(ch) = is_plain_char_key(key_event, repr) {
        return keys.handle_printable(ch, params);
    }
    if !keys.context.is_composing() {
        return keys.handle_idle();
    }
    if let Some(result) = keys.handle_menu_punctuation()? {
        return Ok(result);
    }
    if repr == "Return" || repr == "KP_Enter" {
        return keys.handle_enter();
    }
    if repr == "Escape" {
        return keys.handle_escape();
    }
    if repr == "BackSpace" || repr == "Delete" {
        return keys.handle_backspace();
    }
    if repr == "Left" || repr == "Right" || repr == "Home" || repr == "End" {
        return keys.handle_navigation();
    }
    if repr == "Tab" || repr == "ISO_Left_Tab" || repr == "Shift+Tab" {
        return keys.handle_tab();
    }
    if repr == "Up" || repr == "Down" || repr == "Page_Up" || repr == "Page_Down" {
        return keys.handle_page_keys();
    }
    if repr == "space" {
        return keys.handle_space();
    }
    Ok(ProcessorResult::Forward)
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

impl KeyDispatch<'_, '_> {
    /// 未处于组合态：`_dotAfterDigitArmed` 的小数点直接上屏，其余交宿主处理器。
    fn handle_idle(&mut self) -> anyhow::Result<ProcessorResult> {
        if self.dot_armed
            && (self.repr == "period" || self.repr == "KP_Decimal")
            && !self.key_event.shift()
            && modifier_free(self.key_event)
        {
            self.context.direct_commit(".");
            return Ok(ProcessorResult::Consume);
        }
        Ok(ProcessorResult::Forward)
    }

    /// 纯字符键（`is_plain_char_key` 命中）：反查段选重 / 数字直选 / 空闲数字上屏 /
    /// Tab 确认 / 照常录入与早提交。
    fn handle_printable(
        &mut self,
        ch: char,
        params: EarlyCommitParams,
    ) -> anyhow::Result<ProcessorResult> {
        // 字反查段：其它普通键先清空组合，随后照常处理该键。
        if char_to_sound_shape::tagged(self.context) {
            self.context.clear();
        }
        if !self.context.is_composing()
            && (!self.state.committed_raw.is_empty()
                || !self.state.last_seen_raw.is_empty()
                || !self.state.trackers.is_empty()
                || self.state.suspended
                || self.state.continuation_after_auto_commit
                || self.state.active_lock().is_some()
                || self.state.tab_pending)
        {
            self.state.reset(self.context, false);
        }
        // 分号/引号只在组合中作 rank 选择器；空闲时交标点处理器。
        if !self.context.is_composing() && (ch == ';' || ch == '\'') {
            return Ok(ProcessorResult::Forward);
        }
        if live_input(self.context).len() >= MAX_RAW_LENGTH {
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
            && self.context.composition.back().is_some_and(|segment| {
                segment.has_tag(sound_to_char_shape::SOUND_TO_CHAR_SHAPE_TAG)
            })
        {
            if ch.is_ascii_digit() {
                self.live.pending.clear();
                self.live.baseline = None;
                let index = (ch as u8 - b'0') as isize - 1;
                let count = self
                    .context
                    .composition
                    .back()
                    .map(|segment| segment.candidates.len())
                    .unwrap_or(0);
                if index >= 0 && (index as usize) < count {
                    // 高亮 + 确认与 Space 同路（`Context::select` 可能提交整句）。
                    self.context.highlight(index as usize);
                    confirm_selection(
                        Some(&mut learning_commit(self.decoder, self.live, self.env.now)),
                        self.context,
                        self.state,
                    );
                }
                self.state.reset(self.context, false);
                return Ok(ProcessorResult::Consume);
            }
            if ch == ';' {
                return Ok(ProcessorResult::Consume);
            }
        }
        // 数字直选（`OPTION_DIGIT_SELECT`；addon 扩展）：菜单可见时直接上屏当前页候选。
        if self.context.get_option(OPTION_DIGIT_SELECT)
            && ch.is_ascii_digit()
            && self.context.has_menu()
            && let Some(position) = digit_page_position(ch)
            && select_page_candidate(
                self.decoder,
                self.context,
                self.state,
                self.live,
                self.env.now,
                self.env.page_size,
                position,
            )?
        {
            return Ok(ProcessorResult::Consume);
        }
        // 空闲数字直接上屏（全角选项下为全角）。
        if ch.is_ascii_digit() && !self.context.is_composing() {
            if self.context.get_option("full_shape") {
                const FULL_SHAPE_DIGITS: [char; 10] =
                    ['０', '１', '２', '３', '４', '５', '６', '７', '８', '９'];
                let index = (ch as u8 - b'0') as usize;
                self.context
                    .direct_commit(&FULL_SHAPE_DIGITS[index].to_string());
            } else {
                self.context.direct_commit(&ch.to_string());
            }
            *self.env.dot_armed = true;
            return Ok(ProcessorResult::Consume);
        }
        let live_before = live_input(self.context);
        let caret = input_caret(self.context);
        let mut full_before = self.state.committed_raw.as_bytes().to_vec();
        full_before.extend_from_slice(&live_before);
        if caret != live_before.len() {
            self.live.pending.clear();
            self.live.baseline = None;
            invalidate_edit_state(
                self.context,
                self.state,
                self.state.committed_raw.len() + caret,
                full_before.len() + ch.len_utf8(),
            );
            self.context.push_input(ch.to_string().as_bytes());
            return Ok(ProcessorResult::Consume);
        }
        if self.state.tab_pending && is_letter {
            let target = self
                .context
                .composition
                .back()
                .map(|segment| segment.selected_index)
                .unwrap_or(0);
            let lock = self.state.active_lock().map(|lock| DecodeLock {
                raw: &lock.raw,
                text: &lock.text,
                boundaries: &lock.boundaries,
            });
            let raw_text = String::from_utf8_lossy(&full_before).into_owned();
            let decoded = self.decoder.decode_with_lock(
                &raw_text,
                false,
                &self.state.committed_text,
                lock,
            )?;
            let mut candidate: Option<Selected> = None;
            let mut seen: Vec<FusionAhead> = Vec::new();
            let mut visible = 0usize;
            for item in &decoded.items {
                if implicit_rank_allowed(
                    item,
                    &full_before,
                    self.state.continuation_after_auto_commit,
                    self.allow_duplicate_single,
                ) && item.text.starts_with(&self.state.committed_text)
                    && item.text.len() > self.state.committed_text.len()
                {
                    if visible == target {
                        let (raw_length, diff) = self.decoder.path_summary(item);
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
                if candidate.raw_length > self.state.committed_raw.len() {
                    // 参照 `processor` 的 Tab 确认分支：**先** stage、**再**清 `tab_pending`。
                    // `learning_stage` 以该标志选择基线（`tab_pending and live.baseline or
                    // submitted_first`），且 `reinforce_eligible` 要求 `!tab_pending`；
                    // 清标志后再 stage 会把基线取成 `submitted_first` 并误走 reinforce 路线。
                    // 参照此处不传 `submitted_first`（nil）；清理后分支即 `return`，本调用不与
                    // 提交点的 `learning_commit` 重复（后者对应参照的 commit 通知器，参照同样会走）。
                    learning_stage(
                        self.live,
                        self.state,
                        Some(&candidate),
                        &full_before,
                        None,
                        self.env.now,
                    );
                    self.state.tab_pending = false;
                    let boundaries: String = candidate
                        .diff
                        .path
                        .iter()
                        .map(|node| format!("{},{};", node.raw_length, node.text_length))
                        .collect();
                    let locked_raw =
                        String::from_utf8_lossy(&full_before[..candidate.raw_length]).into_owned();
                    self.state.locks.push(Lock {
                        raw: locked_raw.clone(),
                        text: candidate.text.clone(),
                        boundaries,
                    });
                    let commit = if self.context.get_option(OPTION_EARLY_COMMIT) {
                        let commit = candidate.text[self.state.committed_text.len()..].to_string();
                        self.state.committed_text = candidate.text.clone();
                        self.state.committed_raw = locked_raw;
                        Some(commit)
                    } else {
                        None
                    };
                    reset_early_evidence(self.state);
                    self.state.empty_code_pending = None;
                    self.state.suspended = false;
                    self.state.continuation_after_auto_commit = false;
                    self.state.save(self.context);
                    if let Some(commit) = commit {
                        if let Some(text) = submit_early(self.context, self.state, &commit) {
                            learning_commit(self.decoder, self.live, self.env.now)
                                .commit_with_learning(
                                    self.context,
                                    self.state,
                                    &text,
                                    &candidate.text,
                                    candidate.raw_length,
                                );
                        }
                    }
                    let mut restored = full_before[self.state.committed_raw.len()..].to_vec();
                    restored.extend_from_slice(ch.to_string().as_bytes());
                    restore_composition_input(self.context, &restored);
                    return Ok(ProcessorResult::Consume);
                }
            }
        }
        self.state.tab_pending = false;
        self.live.baseline = None;
        if !is_letter {
            self.state.empty_code_pending = None;
            self.state.save(self.context);
        }
        self.context.push_input(ch.to_string().as_bytes());
        if is_letter
            && try_empty_code_commit(
                &mut learning_commit(self.decoder, self.live, self.env.now),
                self.context,
                self.state,
                &full_before,
                ch.to_string().as_bytes(),
                params,
                self.env.dot_armed,
            )?
        {
            return Ok(ProcessorResult::Consume);
        }
        try_early_commit(
            &mut learning_commit(self.decoder, self.live, self.env.now),
            self.context,
            self.state,
            params,
            self.env.dot_armed,
        )?;
        Ok(ProcessorResult::Consume)
    }

    /// 菜单可见时遇可打印标点：按当前选中项暂存学习、确认组合后把原键交标点处理器。
    /// 命中返回 `Some(Forward)`，否则 `None` 继续后续分发。
    fn handle_menu_punctuation(&mut self) -> anyhow::Result<Option<ProcessorResult>> {
        let codepoint = self.key_event.keycode;
        // 菜单可见（不必处于缓冲态）时遇可打印标点：先按当前选中项暂存学习、确认组合，
        // 再把原键交标点处理器（参照 `abad411`：标点段一旦追加进组合，
        // `learning_selection` 就再也不能解码该输入——例如 `zhhbi,`——或取回句子的选中项）。
        //
        // **本仓有意偏离上游 `abad411`**：
        // 上游对该分支内的**所有**可打印 ASCII 标点一律先确认组合再交标点表，于是宿主
        // `key_binder` 的翻页绑定（缺省 `-`/`=`，以及 schema 绑到翻页的 `[`/`]`）被永久遮蔽
        // （最小复现 `j a equal`：期望翻页，实际提交「一=」）。此处先问**与宿主同一套**翻页判据
        // [`host::paging_action`]：会被判为翻页的键不消费、落回宿主链执行翻页；其余标点维持上游行为。
        if self.context.has_menu()
            && (33..=126).contains(&codepoint)
            && (codepoint as u8 as char).is_ascii_punctuation()
            && modifier_free(self.key_event)
            && host::paging_action(self.context, self.env.host_options, self.key_event).is_none()
        {
            let selection = learning_selection(self.decoder, self.context, self.state)?;
            learning_stage(
                self.live,
                self.state,
                selection.selected.as_ref(),
                &selection.raw,
                None,
                self.env.now,
            );
            confirm_selection(
                Some(&mut learning_commit(self.decoder, self.live, self.env.now)),
                self.context,
                self.state,
            );
            return Ok(Some(ProcessorResult::Forward));
        }
        Ok(None)
    }

    /// 参照 `Return` / `KP_Enter`：缓冲文本 + 实时输入直接上屏。
    fn handle_enter(&mut self) -> anyhow::Result<ProcessorResult> {
        self.live.pending.clear();
        self.live.baseline = None;
        let text = format!(
            "{}{}",
            self.state.buffered_text,
            String::from_utf8_lossy(&live_input(self.context))
        );
        self.context.direct_commit(&text);
        self.context.clear();
        self.state.reset(self.context, false);
        Ok(ProcessorResult::Consume)
    }

    /// 参照 `Escape`：丢弃组合与瞬态状态。
    fn handle_escape(&mut self) -> anyhow::Result<ProcessorResult> {
        self.live.pending.clear();
        self.live.baseline = None;
        self.context.clear();
        self.state.reset(self.context, false);
        Ok(ProcessorResult::Consume)
    }

    /// 参照 `BackSpace` / `Delete`：缓冲删除、锁内编辑，其余交宿主。
    fn handle_backspace(&mut self) -> anyhow::Result<ProcessorResult> {
        self.live.pending.clear();
        self.live.baseline = None;
        self.state.tab_pending = false;
        reset_early_evidence(self.state);
        self.state.empty_code_pending = None;
        if !self.state.buffered_text.is_empty() {
            let raw = live_input(self.context);
            let caret = input_caret(self.context);
            if self.repr == "BackSpace" && raw.is_empty() {
                let mut letters: Vec<char> = self.state.buffered_text.chars().collect();
                let removed = letters.pop();
                self.state.buffered_text = letters.into_iter().collect();
                let removed_length = removed.map(char::len_utf8).unwrap_or(0);
                let mut keep = self
                    .state
                    .committed_text
                    .len()
                    .saturating_sub(removed_length);
                // 属性可能来自旧版本/外部：仅在字符边界上截断，避免 panic。
                while keep > 0 && !self.state.committed_text.is_char_boundary(keep) {
                    keep -= 1;
                }
                self.state.committed_text.truncate(keep);
                if self.state.buffered_text.is_empty() {
                    self.state.reset(self.context, false);
                } else {
                    self.state.locks = vec![Lock {
                        raw: self.state.committed_raw.clone(),
                        text: self.state.committed_text.clone(),
                        boundaries: format!(
                            "{},{};",
                            self.state.committed_raw.len(),
                            self.state.committed_text.len()
                        ),
                    }];
                    self.state.save(self.context);
                }
                restore_composition_input(self.context, &raw);
                return Ok(ProcessorResult::Consume);
            }
            let first = if self.repr == "BackSpace" {
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
                self.context,
                self.state,
                self.state.committed_raw.len() + first,
                self.state.committed_raw.len() + remaining.len(),
            );
            restore_composition_input(self.context, &remaining);
            self.context.set_caret(first + 1);
            return Ok(ProcessorResult::Consume);
        }
        if self.state.active_lock().is_some() {
            let raw = live_input(self.context);
            let caret = input_caret(self.context);
            let first = if self.repr == "BackSpace" {
                caret as isize - 1
            } else {
                caret as isize
            };
            if first < 0 || first >= raw.len() as isize {
                self.state.save(self.context);
                return Ok(ProcessorResult::Forward);
            }
            let first = first as usize;
            let mut remaining = raw[..first].to_vec();
            remaining.extend_from_slice(&raw[first + 1..]);
            invalidate_edit_state(
                self.context,
                self.state,
                self.state.committed_raw.len() + first,
                self.state.committed_raw.len() + remaining.len(),
            );
            if remaining.is_empty() {
                self.context.clear();
                self.state.reset(self.context, false);
            } else if self.repr == "BackSpace" {
                self.context.pop_input(1);
            } else {
                self.context.delete_input(1);
            }
            return Ok(ProcessorResult::Consume);
        }
        self.state.save(self.context);
        Ok(ProcessorResult::Forward)
    }

    /// 参照 `Left` / `Right` / `Home` / `End`：缓冲态下钳制光标，其余交宿主。
    fn handle_navigation(&mut self) -> anyhow::Result<ProcessorResult> {
        self.live.pending.clear();
        self.live.baseline = None;
        // 手动光标导航不保留追加证据与待确认 Tab。
        self.state.tab_pending = false;
        reset_early_evidence(self.state);
        self.state.empty_code_pending = None;
        self.state.save(self.context);
        if !self.state.buffered_text.is_empty()
            && (self.repr == "Home" || (self.repr == "Left" && input_caret(self.context) == 0))
        {
            self.context.set_caret(1);
            return Ok(ProcessorResult::Consume);
        }
        Ok(ProcessorResult::Forward)
    }

    /// 参照 `Tab` / `ISO_Left_Tab` / `Shift+Tab`：循环高亮并挂起早提交证据。
    fn handle_tab(&mut self) -> anyhow::Result<ProcessorResult> {
        if !self.state.tab_pending && self.live.store_ready {
            let selection = learning_selection(self.decoder, self.context, self.state)?;
            self.live.baseline = selection.first;
        }
        reset_early_evidence(self.state);
        self.state.suspended = true;
        self.state.empty_code_pending = None;
        self.state.save(self.context);
        if cycle_candidate_highlight(self.context, if self.repr == "Tab" { 1 } else { -1 }) {
            self.state.tab_pending = true;
            self.state.save(self.context);
            return Ok(ProcessorResult::Consume);
        }
        // 菜单不可用时交给 schema 的 Down/Up 绑定与 navigate。
        Ok(ProcessorResult::Forward)
    }

    /// 参照 `Up` / `Down` / `Page_Up` / `Page_Down`：挂起早提交证据后交宿主。
    fn handle_page_keys(&mut self) -> anyhow::Result<ProcessorResult> {
        reset_early_evidence(self.state);
        self.state.suspended = true;
        self.state.empty_code_pending = None;
        self.state.save(self.context);
        Ok(ProcessorResult::Forward)
    }

    /// 参照 `space`：菜单可见时确认当前选中项并上屏，随后复位。
    fn handle_space(&mut self) -> anyhow::Result<ProcessorResult> {
        if self.context.has_menu() {
            let selection = learning_selection(self.decoder, self.context, self.state)?;
            learning_stage(
                self.live,
                self.state,
                selection.selected.as_ref(),
                &selection.raw,
                None,
                self.env.now,
            );
            confirm_selection(
                Some(&mut learning_commit(self.decoder, self.live, self.env.now)),
                self.context,
                self.state,
            );
        }
        self.live.pending.clear();
        self.live.baseline = None;
        self.state.reset(self.context, false);
        Ok(ProcessorResult::Consume)
    }
}

/// 触发键（音反查 / 字反查）：空闲时进入组合、段内再按则退出（同参照的标签语义）。
/// 命中即返回 `Some`；未命中返回 `None`，交由后续处理器。
fn handle_reverse_lookup_triggers(
    key_event: &KeyEvent,
    context: &mut Context,
) -> Option<ProcessorResult> {
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
            return Some(ProcessorResult::Consume);
        }
        if !context.is_composing() {
            let mut buffer = [0u8; 4];
            context.push_input(prefix.encode_utf8(&mut buffer).as_bytes());
            return Some(ProcessorResult::Consume);
        }
        // 组合中：交由后续处理器（标点等）处理。
    }
    None
}

/// 参照处理器链 `recognizer`（位于 speller/标点之前）：音反查段内继续接受模式内按键。
/// 消费该键时返回 `true`。
fn handle_recognizer(key_event: &KeyEvent, context: &mut Context) -> bool {
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
            // 连续的音节分隔符只保留第一个：判定与语义都在音反查模块。
            if sound_to_char_shape::repeats_delimiter(context.input(), ch) {
                return true;
            }
            context.push_input(&[ch as u8]);
            return true;
        }
    }
    false
}
