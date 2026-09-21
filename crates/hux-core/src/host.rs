// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! 宿主等价物（K3）：方案侧 `processor` 返回 Forward 后，参照链上由 librime 原生组件
//! （`key_binder` → `speller` → `punctuator` → `selector` → `navigator` → `express_editor`）处理的按键。
//!
//! 分工：`speller` 由**方案侧**处理器承担（`hux-scheme/tiger` 的 `interaction::processor`）；
//! 本模块实现其余组件，不经方案（`punctuator` 用 core 的标点表 [`crate::punct`]）。
//!
//! 映射依据（pin `33e78140` / 参照 schema）：
//! - 菜单布局 `Horizontal | Stacked`（未设 `_vertical`/`_linear`/`_horizontal`）；
//! - `menu/page_size: 5`（`page_down_cycle` 缺省 false）；页大小与翻页键可由 addon 经
//!   [`HostOptions`] 配置（缺省同参照：`-`（paging）→ Page_Up、`=`（has_menu）→ Page_Down）；
//! - `key_binder/bindings`：`Tab` → Down、`Shift+Tab` → Up（`when: has_menu`）。
//!
//! 简化（有金样覆盖的部分一律按真值实现）：
//! - navigator 的 `spans_` 跳转（多段/词组边界）按单段处理：Left/Right 逐字节移动，
//!   `Ctrl/Shift+Left|Right` 直接跳到首/尾（标点段与词组边界不做 spans 细分）；
//! - `editor/char_handler`（Printables 直接提交）按 `DirectCommit` 语义实现。

use crate::key::{K_CONTROL_MASK, K_SHIFT_MASK, KeyEvent};
use crate::punct::PunctTable;
use crate::session::Context;

/// 参照 schema `menu/page_size`。
pub const DEFAULT_PAGE_SIZE: usize = 5;
/// 页大小上限（配置 `PageSize` 1–10；数字直选 `0`=第 10 个）。
pub const MAX_PAGE_SIZE: usize = 10;

/// 宿主可配置项（addon 设置注入）：每页候选个数与上/下翻页键（可多项）。
///
/// 缺省即参照 schema：`menu/page_size: 5`、`-`（`when: paging`）→ Page_Up、
/// `=`（`when: has_menu`）→ Page_Down；Page_Up/Page_Down 等导航键不随此变化。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HostOptions {
    /// 每页候选个数（≥ 1；须与宿主候选面板一致）。
    pub page_size: usize,
    /// 上翻页键列表：**已翻过页时生效**（参照 `when: paging`；标签由翻页写入，
    /// 未翻页时不消费该键，落作标点/输入——见 [`key_binder`] 与 `docs/config.md`）。
    pub page_up_keys: Vec<KeyEvent>,
    /// 下翻页键列表：菜单可用（`has_menu`）时生效。
    pub page_down_keys: Vec<KeyEvent>,
    /// 翻页循环（参照 `menu/page_down_cycle`，默认关）：末页再翻回首页、首页向上翻到末页。
    pub page_cycle: bool,
}

impl Default for HostOptions {
    fn default() -> Self {
        Self {
            page_size: DEFAULT_PAGE_SIZE,
            page_up_keys: vec![KeyEvent::from_repr("minus").expect("minus")],
            page_down_keys: vec![KeyEvent::from_repr("equal").expect("equal")],
            page_cycle: false,
        }
    }
}

/// 宿主处理结果：`Consumed` 对应参照链返回 `kAccepted`（吞键）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HostResult {
    Consumed,
    Forward,
}

/// 宿主链提交点回调：由**方案侧**实现（core 不持有方案状态）。
///
/// 对应 librime `Context::Commit()` 内、清空组合前的 `commit_notifier`：
/// `punctuator` / `express_editor` 提交文本时回调，方案据此落学习等。
pub trait CommitObserver {
    fn on_commit(&mut self, context: &Context, commit_text: &str);
}

/// 参照处理器链（`key_binder` → `speller` → `punctuator` → `selector` → `navigator`
/// → `express_editor`；`speller` 由方案侧 `processor` 承担，见模块头）。
///
/// `observer`（可空）供宿主链的提交点回调方案（学习等）。
pub fn process_key(
    key_event: &KeyEvent,
    context: &mut Context,
    punct: Option<&PunctTable>,
    options: &HostOptions,
    observer: Option<&mut dyn CommitObserver>,
) -> HostResult {
    if key_event.release() {
        return HostResult::Forward;
    }
    let mut observer = observer;
    // 依序执行，前一处理器吞键则不再继续（参照引擎的处理器链）。
    let mut result = key_binder(key_event, context, options);
    if result == HostResult::Consumed {
        return result;
    }
    result = punctuator(key_event, context, punct, &mut observer);
    if result == HostResult::Consumed {
        return result;
    }
    result = selector(key_event, context, options);
    if result == HostResult::Consumed {
        return result;
    }
    result = navigator(key_event, context);
    if result == HostResult::Consumed {
        return result;
    }
    editor(key_event, context, &mut observer)
}

/// 宿主链提交点回调（对应 librime `Context::Commit()` 的通知器：组合仍完整时记录）；
/// `observer` 为 `None` 时 no-op。
fn commit_notifier(
    observer: &mut Option<&mut dyn CommitObserver>,
    context: &Context,
    commit_text: &str,
) {
    if let Some(observer) = observer.as_deref_mut() {
        observer.on_commit(context, commit_text);
    }
}

// ---------------------------------------------------------------- punctuator

/// 参照 `Punctuator::ProcessKeyEvent`（`digit_separators: ""`，`use_space` 缺省 false）。
///
/// 命中标点表后：把当前组合文本（选中候选或原始输入）与标点一并提交并清空
/// （参照 `PushInput` → `punct` 段翻译 → `ConfirmUniquePunct`/`AutoCommitPunct`/`PairPunct`
/// 的净效果；候选菜单形态参照表未使用）。
fn punctuator(
    key_event: &KeyEvent,
    context: &mut Context,
    punct: Option<&PunctTable>,
    observer: &mut Option<&mut dyn CommitObserver>,
) -> HostResult {
    let Some(table) = punct else {
        return HostResult::Forward;
    };
    if key_event.ctrl() || key_event.alt() || key_event.super_modifier() {
        return HostResult::Forward;
    }
    let keycode = key_event.keycode;
    if !(0x20..0x7f).contains(&keycode) {
        return HostResult::Forward;
    }
    if context.get_option("ascii_punct") {
        return HostResult::Forward;
    }
    // `use_space = false`：组合中的空格交后续处理器（方案侧 `processor` 已消费）。
    if keycode == 0x20 && context.is_composing() {
        return HostResult::Forward;
    }
    let full_shape = context.get_option("full_shape");
    let Some(text) = table.resolve(char::from(keycode as u8), full_shape, context.punct_pairs())
    else {
        return HostResult::Forward;
    };
    let commit = format!("{}{}", context.get_commit_text(), text);
    commit_notifier(observer, context, &commit);
    context.clear();
    context.direct_commit(&commit);
    HostResult::Consumed
}

// ---------------------------------------------------------------- key_binder

/// 参照 `KeyBinder::ProcessKeyEvent`：Tab/Shift+Tab 固定，翻页键取 [`HostOptions`]。
/// 条件为 `has_menu`（非 ascii_mode）——上/下翻页键均有候选时生效并消费（参照仅当上翻页键
/// 带 `paging` 标签时绑定；此处放宽，避免其落作标点/输入）。
fn key_binder(key_event: &KeyEvent, context: &mut Context, options: &HostOptions) -> HostResult {
    if context.get_option("ascii_mode") {
        return HostResult::Forward;
    }
    let repr = key_event.repr();
    if !context.has_menu() {
        return HostResult::Forward;
    }
    if options.page_up_keys.iter().any(|key| key.repr() == repr) {
        // 上翻页键的绑定条件是参照 `when: paging`（`KeyBinder` 于末段带 `paging` 标签时置位，
        // 该标签由翻过页写入）。未翻页时**不消费**——交后续处理器落作标点/输入，
        // 与参照一致（此前本实现放宽为「有候选即消费」，会吞掉标点键）。
        if !has_paging_tag(context) {
            return HostResult::Forward;
        }
        return selector_action(SelectorAction::PreviousPage, context, options);
    }
    if options.page_down_keys.iter().any(|key| key.repr() == repr) {
        return selector_action(SelectorAction::NextPage, context, options);
    }
    match repr.as_str() {
        "Tab" => selector_action(SelectorAction::NextCandidate, context, options),
        "Shift+Tab" => selector_action(SelectorAction::PreviousCandidate, context, options),
        _ => HostResult::Forward,
    }
}

// ---------------------------------------------------------------- selector

#[derive(Clone, Copy)]
enum SelectorAction {
    PreviousCandidate,
    NextCandidate,
    PreviousPage,
    NextPage,
    Home,
    End,
}

impl SelectorAction {
    /// 参照 `Selector::ProcessKeyEvent` 的 Horizontal|Stacked keymap（精确修饰匹配）。
    fn from_key(key_event: &KeyEvent, vertical: bool) -> Option<Self> {
        match (key_event.keycode, vertical) {
            (0xff52 | 0xff97, false) => Some(Self::PreviousCandidate), // Up / KP_Up
            (0xff54 | 0xff99, false) => Some(Self::NextCandidate),     // Down / KP_Down
            (0xff55 | 0xff9a, _) => Some(Self::PreviousPage),          // Page_Up / KP_Prior
            (0xff56 | 0xff9b, _) => Some(Self::NextPage),              // Page_Down / KP_Next
            (0xff50 | 0xff95, _) => Some(Self::Home),                  // Home / KP_Home
            (0xff57 | 0xff9c, _) => Some(Self::End),                   // End / KP_End
            (0xff53 | 0xff98, true) => Some(Self::PreviousCandidate),  // Right / KP_Right（竖排）
            (0xff51 | 0xff96, true) => Some(Self::NextCandidate),      // Left / KP_Left（竖排）
            _ => None,
        }
    }
}

/// 参照 `Selector::ProcessKeyEvent`：组合/菜单前置条件 + keymap。
fn selector(key_event: &KeyEvent, context: &mut Context, options: &HostOptions) -> HostResult {
    if key_event.alt() || key_event.super_modifier() {
        return HostResult::Forward;
    }
    // 精确匹配（无修饰）；keymap 无 Ctrl/Shift 绑定，FallbackOptions::None。
    if key_event.modifier != 0 {
        return HostResult::Forward;
    }
    let vertical = context.get_option("_vertical");
    let Some(action) = SelectorAction::from_key(key_event, vertical) else {
        return HostResult::Forward;
    };
    let Some(segment) = context.composition.back() else {
        return HostResult::Forward;
    };
    if !segment.translated || segment.has_tag("raw") {
        return HostResult::Forward;
    }
    selector_action(action, context, options)
}

/// 参照 `Selector` 各动作（返回值即 `kAccepted`/`kNoop`）。
fn selector_action(
    action: SelectorAction,
    context: &mut Context,
    options: &HostOptions,
) -> HostResult {
    let page_size = options.page_size.max(1);
    let linear = context.get_option("_linear") || context.get_option("_horizontal");
    let caret_at_end = context.caret() >= context.input().len();
    let consumed = match action {
        SelectorAction::PreviousCandidate => {
            if (linear && !caret_at_end) || context.composition.back().is_none() {
                false // 行内布局交 navigator；无段则无菜单
            } else {
                let index = context.composition.back().unwrap().selected_index;
                if index == 0 {
                    // 行内布局回退给 navigator；堆叠布局吞键不环绕。
                    !linear
                } else {
                    context.highlight(index - 1);
                    mark_paging(context);
                    true
                }
            }
        }
        SelectorAction::NextCandidate => {
            if linear && !caret_at_end {
                false
            } else {
                let Some(segment) = context.composition.back() else {
                    return HostResult::Forward;
                };
                if !segment.translated {
                    return HostResult::Forward;
                }
                let index = segment.selected_index + 1;
                let candidate_count = segment.prepare(index + 1);
                if candidate_count <= index {
                    true // 末页不再前进，但吞键
                } else {
                    context.highlight(index);
                    mark_paging(context);
                    true
                }
            }
        }
        SelectorAction::PreviousPage => {
            let Some(segment) = context.composition.back() else {
                return HostResult::Forward;
            };
            if !segment.translated {
                return HostResult::Forward;
            }
            let selected = segment.selected_index;
            if selected < page_size {
                // 已在首页：默认停在首页（吞键）；开启循环则回到末页。
                if options.page_cycle {
                    let total = segment.prepare(usize::MAX);
                    let last_page_start = total.saturating_sub(1) / page_size * page_size;
                    context.highlight(last_page_start);
                    mark_paging(context);
                }
                true
            } else {
                let index = selected.saturating_sub(page_size);
                context.highlight(index);
                mark_paging(context);
                true
            }
        }
        SelectorAction::NextPage => {
            let Some(segment) = context.composition.back() else {
                return HostResult::Forward;
            };
            if !segment.translated {
                return HostResult::Forward;
            }
            let index = segment.selected_index + page_size;
            let page_start = (index / page_size) * page_size;
            let candidate_count = segment.prepare(page_start + page_size);
            if candidate_count <= page_start {
                // 已在末页：默认吞键不循环；开启循环则回到首页。
                if options.page_cycle {
                    context.highlight(0);
                    mark_paging(context);
                }
                true
            } else {
                let index = if index >= candidate_count {
                    candidate_count - 1
                } else {
                    index
                };
                context.highlight(index);
                mark_paging(context);
                true
            }
        }
        SelectorAction::Home => {
            if context.composition.back().is_none() {
                false
            } else if context.composition.back().unwrap().selected_index > 0 {
                context.highlight(0);
                true
            } else {
                false // 交给 navigator 移动光标
            }
        }
        SelectorAction::End => {
            if context.caret() < context.input().len() {
                false // navigator should handle this
            } else {
                selector_action(SelectorAction::Home, context, options) == HostResult::Consumed
            }
        }
    };
    if consumed {
        HostResult::Consumed
    } else {
        HostResult::Forward
    }
}

/// 参照 `comp.back().tags.insert("paging")`：翻过页后置位，使上翻页键的 `when: paging` 成立。
/// 末段是否带 `paging` 标签（参照 `KeyBinder` 的 `kWhenPaging` 判据）。
fn has_paging_tag(context: &Context) -> bool {
    context
        .composition
        .back()
        .is_some_and(|segment| segment.has_tag("paging"))
}

fn mark_paging(context: &mut Context) {
    if let Some(segment) = context.composition.back_mut()
        && !segment.has_tag("paging")
    {
        segment.tags.push("paging".to_string());
    }
}

// ---------------------------------------------------------------- navigator

#[derive(Clone, Copy)]
enum NavigatorAction {
    Rewind,
    Forward,
    LeftByChar,
    RightByChar,
    LeftBySyllable,
    RightBySyllable,
    Home,
    End,
}

impl NavigatorAction {
    /// 参照 `Navigator` 的 Horizontal/Vertical keymap（精确修饰匹配：无修饰 / Ctrl）。
    fn from_key(key_event: &KeyEvent, vertical: bool) -> Option<Self> {
        let code = key_event.keycode;
        match (code, key_event.modifier, vertical) {
            (0xff51, 0, false) | (0xff53, 0, true) => Some(Self::Rewind), // Left / Right（竖排）
            (0xff53, 0, false) | (0xff51, 0, true) => Some(Self::Forward),
            (0xff51, K_CONTROL_MASK, false) | (0xff53, K_CONTROL_MASK, true) => {
                Some(Self::LeftBySyllable)
            }
            (0xff53, K_CONTROL_MASK, false) | (0xff51, K_CONTROL_MASK, true) => {
                Some(Self::RightBySyllable)
            }
            (0xff96, 0, false) => Some(Self::LeftByChar), // KP_Left
            (0xff98, 0, false) => Some(Self::RightByChar), // KP_Right
            (0xff50 | 0xff95, 0, _) => Some(Self::Home),
            (0xff57 | 0xff9c, 0, _) => Some(Self::End),
            (0xff52, 0, true) => Some(Self::Rewind), // Up（竖排）
            (0xff54, 0, true) => Some(Self::Forward), // Down（竖排）
            (0xff52, K_CONTROL_MASK, true) => Some(Self::LeftBySyllable),
            (0xff54, K_CONTROL_MASK, true) => Some(Self::RightBySyllable),
            (0xff97, 0, true) => Some(Self::LeftByChar), // KP_Up（竖排）
            (0xff99, 0, true) => Some(Self::RightByChar), // KP_Down（竖排）
            _ => None,
        }
    }
}

/// 参照 `Navigator::ProcessKeyEvent`：组合中生效，含 `FallbackOptions::All` 回退。
fn navigator(key_event: &KeyEvent, context: &mut Context) -> HostResult {
    if !context.is_composing() {
        return HostResult::Forward;
    }
    let vertical = context.get_option("_vertical");
    if let Some(action) = NavigatorAction::from_key(key_event, vertical) {
        return navigator_action(action, context);
    }
    // 回退：Ctrl/Alt 不参与；Shift 依次按「视为 Ctrl」「忽略 Shift」重试。
    if key_event.ctrl() || key_event.alt() {
        return HostResult::Forward;
    }
    if key_event.shift() {
        let shift_as_control = KeyEvent::new(
            key_event.keycode,
            (key_event.modifier & !K_SHIFT_MASK) | K_CONTROL_MASK,
        );
        if let Some(action) = NavigatorAction::from_key(&shift_as_control, vertical) {
            return navigator_action(action, context);
        }
        let ignore_shift = KeyEvent::new(key_event.keycode, key_event.modifier & !K_SHIFT_MASK);
        if let Some(action) = NavigatorAction::from_key(&ignore_shift, vertical) {
            return navigator_action(action, context);
        }
    }
    HostResult::Forward
}

/// 参照 `Navigator` 各动作（除 `Rewind`/`Forward` 的 spans 跳转外均按单段语义）。
fn navigator_action(action: NavigatorAction, context: &mut Context) -> HostResult {
    match action {
        NavigatorAction::Rewind => {
            // 单段等价：MoveLeft（多段跳转见模块注释）。
            move_left(context);
            HostResult::Consumed
        }
        NavigatorAction::Forward => {
            move_right(context);
            HostResult::Consumed
        }
        NavigatorAction::LeftByChar => {
            if !move_left(context) {
                go_to_end(context);
            }
            HostResult::Consumed
        }
        NavigatorAction::RightByChar => {
            if !move_right(context) {
                go_home(context);
            }
            HostResult::Consumed
        }
        NavigatorAction::LeftBySyllable => {
            // `JumpLeft(confirmed_pos, loop)`：单段 → 跳到段首。
            context.set_caret(0);
            HostResult::Consumed
        }
        NavigatorAction::RightBySyllable => {
            // `JumpRight(confirmed_pos, loop)`：单段 → 跳到输入末尾。
            go_to_end(context);
            HostResult::Consumed
        }
        NavigatorAction::Home => {
            go_home(context);
            HostResult::Consumed
        }
        NavigatorAction::End => {
            go_to_end(context);
            HostResult::Consumed
        }
    }
}

/// 参照 `Navigator::MoveLeft`。
fn move_left(context: &mut Context) -> bool {
    let caret = context.caret();
    if caret == 0 {
        return false;
    }
    context.set_caret(caret - 1);
    true
}

/// 参照 `Navigator::MoveRight`。
fn move_right(context: &mut Context) -> bool {
    let caret = context.caret();
    if caret >= context.input().len() {
        return false;
    }
    context.set_caret(caret + 1);
    true
}

/// 参照 `Navigator::GoHome`：跳到首个未确认段的起点，否则回到 0。
fn go_home(context: &mut Context) {
    let caret = context.caret();
    if !context.composition.segments.is_empty() {
        let mut confirmed_pos = caret;
        for segment in context.composition.segments.iter().rev() {
            if segment.selected {
                break;
            }
            confirmed_pos = segment.start;
        }
        if confirmed_pos < caret {
            context.set_caret(confirmed_pos);
            return;
        }
    }
    if caret != 0 {
        context.set_caret(0);
    }
}

/// 参照 `Navigator::GoToEnd`。
fn go_to_end(context: &mut Context) {
    let end = context.input().len();
    if context.caret() != end {
        context.set_caret(end);
    }
}

// ---------------------------------------------------------------- express_editor

/// 参照 `ExpressEditor`（`_auto_commit = true` 变体）的 keymap 子集 + `char_handler`。
///
/// Return/space/Escape 在组合中已被方案侧 `processor` 消费，此处为完整的兜底实现；
/// 可打印字符按 `char_handler`（ExpressEditor = `DirectCommit`）处理：
/// **先提交当前组合**（保证上屏顺序），按键交宿主。
/// 学习链：提交点经 [`commit_notifier`] 记录（对应参照 librime 的提交通知器）。
fn editor(
    key_event: &KeyEvent,
    context: &mut Context,
    observer: &mut Option<&mut dyn CommitObserver>,
) -> HostResult {
    if !context.is_composing() {
        return HostResult::Forward;
    }
    let consumed = match (key_event.keycode, key_event.modifier) {
        (0x20, 0) => {
            // Confirm：`confirm_current_selection() || commit()`
            if !confirm(context) {
                let commit_text = context.get_commit_text();
                commit_notifier(observer, context, &commit_text);
                context.commit();
            }
            true
        }
        (0x20, K_SHIFT_MASK) => {
            // `FallbackOptions::All` 的回退：Shift+space → `{XK_space, 0}` = Confirm。
            if !confirm(context) {
                let commit_text = context.get_commit_text();
                commit_notifier(observer, context, &commit_text);
                context.commit();
            }
            true
        }
        (0xff08, 0) | (0xff08, K_SHIFT_MASK) => {
            // `{XK_BackSpace, 0}` = RevertLastEdit；Shift 变体走 `FallbackOptions::All` 回退。
            revert_last_edit(context);
            true
        }
        (0xff08, K_CONTROL_MASK) => {
            back_to_previous_syllable(context);
            true
        }
        (0xff0d, 0) => {
            // 参照 `Editor::CommitRawInput` = `ClearNonConfirmedComposition(); Commit();`：
            // 先丢弃**未确认**的末段（本实现的 `selected_candidate()` 不看 `selected` 标志，
            // 故必须显式清段），保证 Return 提交的是原始输入码而不是高亮候选。
            context.refresh_non_confirmed_composition();
            let commit_text = context.get_commit_text();
            commit_notifier(observer, context, &commit_text);
            context.commit();
            true
        }
        (0xff0d, K_CONTROL_MASK) => {
            // 参照 `{XK_Return, kControlMask}` = `CommitScriptText`（ExpressEditor 绑定）：
            // 提交「脚本文本」——即按当前组合原样提交（**不清未确认段**；注意 `Context::GetScriptText`
            // 的准确定义尚未从参照源码核对，此处按「脚本文本 = 组合文本」实现，待复核见 docs/refactor.md §8）。
            let commit_text = context.get_commit_text();
            commit_notifier(observer, context, &commit_text);
            context.commit();
            true
        }
        // 注意：模式里的 `|` 是**或模式**而非按位或，故组合修饰键必须用 match guard。
        (0xff0d, modifier) if modifier == K_CONTROL_MASK | K_SHIFT_MASK => {
            // 参照 `{XK_Return, kControlMask | kShiftMask}` = `CommitComment`：
            // 提交高亮候选的注释（如反查候选的虎码）并清空组合。
            let comment = context
                .composition
                .back()
                .and_then(|segment| segment.selected_candidate())
                .map(|candidate| candidate.comment.clone())
                .unwrap_or_default();
            context.clear();
            context.direct_commit(&comment);
            true
        }
        (0xffff, 0) | (0xffff, K_SHIFT_MASK) => {
            // `{XK_Delete, 0}` = DeleteChar；Shift 变体走回退。
            context.delete_input(1);
            true
        }
        (0xffff, K_CONTROL_MASK) => true, // DeleteCandidate：设置选中并通知（本实现无删除通道）
        (0xff1b, 0) => {
            cancel_composition(context);
            true
        }
        _ => false,
    };
    if consumed {
        return HostResult::Consumed;
    }
    // 参照 `Editor::ProcessKeyEvent` 的 char_handler（ExpressEditor = `DirectCommit`）：
    // 可打印字符（>0x20 且 <0x7f，无 Ctrl/Alt/Super）先提交组合，再交宿主。
    // 宿主层（fcitx5 addon）会据此消费该键并以 `forwardKey` 重发，保证「提交 → 按键」送达顺序。
    if !key_event.ctrl()
        && !key_event.alt()
        && !key_event.super_modifier()
        && key_event.keycode > 0x20
        && key_event.keycode < 0x7f
    {
        let commit_text = context.get_commit_text();
        commit_notifier(observer, context, &commit_text);
        context.commit();
    }
    HostResult::Forward
}

/// 参照 `Context::ConfirmCurrentSelection`。
fn confirm(context: &mut Context) -> bool {
    context.confirm_current_selection()
}

/// 参照 `Editor::RevertLastEdit`：`ReopenPreviousSelection() || (PopInput() && ReopenPreviousSegment())`。
fn revert_last_edit(context: &mut Context) {
    if reopen_previous_selection(context) {
        return;
    }
    if context.pop_input(1) {
        reopen_previous_segment(context);
    }
}

/// 参照 `Editor::BackToPreviousSyllable`：无词组 spans 时等价于退格。
fn back_to_previous_syllable(context: &mut Context) {
    if reopen_previous_selection(context) {
        return;
    }
    if context.pop_input(1) {
        reopen_previous_segment(context);
    }
}

/// 参照 `Context::ReopenPreviousSelection`：末尾已选段回退为未选。
///
/// 与参照的结构差异（有意）：参照另有两道护栏——`seg->status > kSelected` 与
/// `seg->tags.count("selected_before_editing")`，本模型下**不可达**：
/// 已确认段会被移出组合（进 committed/locks 状态），故不存在 `kConfirmed` 段；
/// 本 crate 也无 `BeginEditing` 等价物（无该标签的写入方）。
/// 若将来引入「编辑态」（`BeginEditing`）或组合内的确认段，须同步补这两道判据。
fn reopen_previous_selection(context: &mut Context) -> bool {
    let mut index = context.composition.segments.len();
    while index > 0 {
        index -= 1;
        if !context.composition.segments[index].selected {
            continue;
        }
        let caret = context.caret();
        context.composition.segments.truncate(index + 1);
        let segment = &mut context.composition.segments[index];
        reopen_segment(segment, caret);
        return true;
    }
    false
}

/// 参照 `Context::ReopenPreviousSegment`：`composition.Trim()` 后回退末尾已选段。
fn reopen_previous_segment(context: &mut Context) -> bool {
    if !context.composition.trim() {
        return false;
    }
    let caret = context.caret();
    if let Some(segment) = context.composition.back_mut()
        && segment.selected
    {
        reopen_segment(segment, caret);
    }
    true
}

/// 参照 `Segment::Reopen`：清掉选中状态（同位置保留候选与高亮）。
fn reopen_segment(segment: &mut crate::session::Segment, caret: usize) {
    segment.selected = false;
    if segment.end != caret {
        segment.translated = false;
        segment.candidates.clear();
        segment.selected_index = 0;
    }
}

/// 参照 `Editor::CancelComposition`：`ClearPreviousSegment() || Clear()`。
fn cancel_composition(context: &mut Context) {
    if !clear_previous_segment(context) {
        context.clear();
    }
}

/// 参照 `Context::ClearPreviousSegment`：输入截到末段起点。
fn clear_previous_segment(context: &mut Context) -> bool {
    let Some(segment) = context.composition.back() else {
        return false;
    };
    let where_ = segment.start;
    if where_ >= context.input().len() {
        return false;
    }
    let head = context.input()[..where_].to_vec();
    context.set_input(&head);
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::{Candidate, Segment};

    fn context_with_menu(texts: &[&str], highlight: usize) -> Context {
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

    fn process(
        context: &mut Context,
        repr: &str,
        punct: Option<&PunctTable>,
        options: &HostOptions,
    ) -> HostResult {
        let key = KeyEvent::from_repr(repr).expect("key repr");
        process_key(&key, context, punct, options, None)
    }

    fn press(context: &mut Context, repr: &str) -> HostResult {
        press_with(context, repr, &HostOptions::default())
    }

    fn press_with(context: &mut Context, repr: &str, options: &HostOptions) -> HostResult {
        process(context, repr, None, options)
    }

    fn selected(context: &Context) -> usize {
        context.composition.back().unwrap().selected_index
    }

    fn custom_page_options(page_size: usize) -> HostOptions {
        HostOptions {
            page_size,
            page_up_keys: vec![KeyEvent::from_repr("comma").expect("key")],
            page_down_keys: vec![KeyEvent::from_repr("period").expect("key")],
            page_cycle: false,
        }
    }

    #[test]
    fn editor_commits_composition_on_uppercase_then_passes() {
        // 组合中收到大写字母：先提交组合（保证上屏顺序），按键交宿主。
        let mut context = context_with_menu(&["甲", "乙"], 0);
        assert_eq!(press(&mut context, "A"), HostResult::Forward);
        assert_eq!(context.last_commit_text(), "甲");
        assert!(context.input().is_empty());
    }

    #[test]
    fn editor_passes_uppercase_when_idle() {
        let mut context = Context::new();
        assert_eq!(press(&mut context, "A"), HostResult::Forward);
        assert_eq!(context.last_commit_text(), "");
    }

    fn punct_table() -> PunctTable {
        PunctTable::parse(
            "punctuator:\n  half_shape:\n    \",\": { commit: ， }\n    \"'\": { pair: [ \"‘\", \"’\" ] }\n",
        )
        .expect("punct table")
    }

    #[test]
    fn punctuator_commits_standalone_punct() {
        let table = punct_table();
        let mut context = Context::new();
        assert_eq!(
            process(&mut context, "comma", Some(&table), &HostOptions::default()),
            HostResult::Consumed
        );
        assert_eq!(context.last_commit_text(), "，");
    }

    #[test]
    fn punctuator_appends_to_composition_text() {
        let table = punct_table();
        let mut context = context_with_menu(&["甲", "乙"], 0);
        assert_eq!(
            process(&mut context, "comma", Some(&table), &HostOptions::default()),
            HostResult::Consumed
        );
        assert_eq!(context.last_commit_text(), "甲，");
        assert!(context.input().is_empty());
    }

    #[test]
    fn punctuator_pair_alternates() {
        let table = punct_table();
        let mut context = Context::new();
        for text in ["‘", "’"] {
            assert_eq!(
                process(
                    &mut context,
                    "apostrophe",
                    Some(&table),
                    &HostOptions::default()
                ),
                HostResult::Consumed
            );
            assert_eq!(context.last_commit_text(), text);
        }
    }

    #[test]
    fn punctuator_passes_unmapped_key() {
        let table = punct_table();
        let mut context = Context::new();
        assert_eq!(
            process(&mut context, "space", Some(&table), &HostOptions::default()),
            HostResult::Forward
        );
    }

    #[test]
    fn punctuator_without_table_passes() {
        let mut context = Context::new();
        assert_eq!(
            process(&mut context, "comma", None, &HostOptions::default()),
            HostResult::Forward
        );
    }

    #[test]
    fn selector_moves_highlight_without_wrapping() {
        let mut context = context_with_menu(&["甲", "乙"], 0);
        assert_eq!(press(&mut context, "Up"), HostResult::Consumed);
        assert_eq!(selected(&context), 0);
        assert_eq!(press(&mut context, "Down"), HostResult::Consumed);
        assert_eq!(selected(&context), 1);
        // 末项：吞键不再前进（无环绕）
        assert_eq!(press(&mut context, "Down"), HostResult::Consumed);
        assert_eq!(selected(&context), 1);
    }

    #[test]
    fn selector_end_at_tail_returns_to_first() {
        let mut context = context_with_menu(&["甲", "乙"], 1);
        assert_eq!(press(&mut context, "End"), HostResult::Consumed);
        assert_eq!(selected(&context), 0);
    }

    #[test]
    fn selector_pages_by_default_page_size() {
        let mut context = context_with_menu(&["a", "b", "c", "d", "e", "f"], 0);
        assert_eq!(press(&mut context, "Page_Down"), HostResult::Consumed);
        assert_eq!(selected(&context), 5);
        assert_eq!(press(&mut context, "Page_Up"), HostResult::Consumed);
        assert_eq!(selected(&context), 0);
    }

    #[test]
    fn selector_page_down_is_noop_on_single_page() {
        let mut small = context_with_menu(&["甲", "乙"], 0);
        assert_eq!(press(&mut small, "Page_Down"), HostResult::Consumed);
        assert_eq!(selected(&small), 0);
    }

    /// 翻页循环（`page_cycle`）：末页再下回首页、首页向上翻到末页。
    #[test]
    fn selector_page_cycle_wraps_at_ends() {
        let mut options = custom_page_options(2);
        options.page_cycle = true;
        let mut context = context_with_menu(&["a", "b", "c", "d", "e"], 0);
        assert_eq!(
            press_with(&mut context, "period", &options),
            HostResult::Consumed
        );
        assert_eq!(selected(&context), 2);
        assert_eq!(
            press_with(&mut context, "period", &options),
            HostResult::Consumed
        );
        assert_eq!(selected(&context), 4);
        // 末页再下 → 首页。
        assert_eq!(
            press_with(&mut context, "period", &options),
            HostResult::Consumed
        );
        assert_eq!(selected(&context), 0);
        // 首页再上 → 末页起点。
        assert_eq!(
            press_with(&mut context, "comma", &options),
            HostResult::Consumed
        );
        assert_eq!(selected(&context), 4);
    }

    /// 默认不循环：末页/首页翻页只吞键、不动。
    #[test]
    fn selector_page_does_not_wrap_by_default() {
        let options = custom_page_options(2);
        let mut context = context_with_menu(&["a", "b", "c", "d", "e"], 0);
        for expected in [2, 4, 4] {
            assert_eq!(
                press_with(&mut context, "period", &options),
                HostResult::Consumed
            );
            assert_eq!(selected(&context), expected);
        }
        for expected in [2, 0, 0] {
            assert_eq!(
                press_with(&mut context, "comma", &options),
                HostResult::Consumed
            );
            assert_eq!(selected(&context), expected);
        }
    }

    #[test]
    fn selector_uses_configured_page_keys() {
        let options = custom_page_options(2);
        let mut context = context_with_menu(&["a", "b", "c", "d", "e", "f"], 0);
        assert_eq!(
            press_with(&mut context, "period", &options),
            HostResult::Consumed
        );
        assert_eq!(selected(&context), 2);
        assert_eq!(
            press_with(&mut context, "comma", &options),
            HostResult::Consumed
        );
        assert_eq!(selected(&context), 0);
    }

    #[test]
    fn selector_ignores_unconfigured_page_key() {
        let options = custom_page_options(2);
        let mut context = context_with_menu(&["a", "b", "c", "d", "e", "f"], 0);
        assert_eq!(
            press_with(&mut context, "equal", &options),
            HostResult::Forward
        );
    }

    #[test]
    fn selector_uses_configured_page_size_for_navigation() {
        let mut context = context_with_menu(&["a", "b", "c", "d", "e", "f"], 0);
        assert_eq!(
            press_with(&mut context, "Page_Down", &custom_page_options(2)),
            HostResult::Consumed
        );
        assert_eq!(selected(&context), 2);
        let mut single = context_with_menu(&["a", "b", "c", "d", "e", "f"], 0);
        assert_eq!(
            press_with(&mut single, "Page_Down", &custom_page_options(1)),
            HostResult::Consumed
        );
        assert_eq!(selected(&single), 1);
    }

    #[test]
    fn selector_page_up_requires_paging_tag() {
        // 参照 `key_binder`：上翻页键的绑定条件是 `when: paging`。
        // 未翻页时不消费——该键继续下落（无标点表时由 `editor` 的可打印字符路径
        // 提交组合，有标点表时落作标点），与参照一致。
        let mut fresh = context_with_menu(&["a", "b", "c", "d", "e", "f"], 0);
        assert_eq!(
            press(&mut fresh, "minus"),
            HostResult::Forward,
            "未翻页时上翻页键不消费"
        );

        // 独立上下文：先下翻一页（`=`，`when: has_menu`）写入 `paging` 标签，再按上翻页键。
        let mut paged = context_with_menu(&["a", "b", "c", "d", "e", "f"], 0);
        assert_eq!(press(&mut paged, "equal"), HostResult::Consumed);
        assert!(
            paged
                .composition
                .back()
                .is_some_and(|segment| segment.has_tag("paging")),
            "翻页应写入 paging 标签"
        );
        assert_eq!(
            press(&mut paged, "minus"),
            HostResult::Consumed,
            "翻过页后上翻页键生效"
        );

        // 无菜单：不消费（交宿主）。
        let mut idle = Context::new();
        assert_eq!(press(&mut idle, "minus"), HostResult::Forward);
    }

    #[test]
    fn selector_accepts_multiple_page_keys() {
        let options = HostOptions {
            page_size: 2,
            page_up_keys: vec![
                KeyEvent::from_repr("comma").expect("key"),
                KeyEvent::from_repr("bracketleft").expect("key"),
            ],
            page_down_keys: vec![
                KeyEvent::from_repr("period").expect("key"),
                KeyEvent::from_repr("bracketright").expect("key"),
            ],
            page_cycle: false,
        };
        let mut context = context_with_menu(&["a", "b", "c", "d", "e", "f"], 0);
        assert_eq!(
            press_with(&mut context, "period", &options),
            HostResult::Consumed
        );
        assert_eq!(selected(&context), 2);
        // 第二绑定：`]` 同样下翻一页
        assert_eq!(
            press_with(&mut context, "bracketright", &options),
            HostResult::Consumed
        );
        assert_eq!(selected(&context), 4);
        // 第二绑定：`[` 上翻一页
        assert_eq!(
            press_with(&mut context, "bracketleft", &options),
            HostResult::Consumed
        );
        assert_eq!(selected(&context), 2);
    }

    #[test]
    fn selector_requires_translated_segment() {
        let mut context = context_with_menu(&["甲"], 0);
        context.composition.segments[0].translated = false;
        assert_eq!(press(&mut context, "Down"), HostResult::Forward);
    }

    #[test]
    fn selector_skips_raw_segment() {
        let mut raw = context_with_menu(&["甲"], 0);
        raw.composition.segments[0].tags = vec!["raw".to_string()];
        assert_eq!(press(&mut raw, "Down"), HostResult::Forward);
    }

    #[test]
    fn navigator_moves_caret_by_char() {
        let mut context = context_with_menu(&["甲", "乙"], 0);
        assert_eq!(press(&mut context, "Left"), HostResult::Consumed);
        assert_eq!(context.caret(), 1);
        assert_eq!(press(&mut context, "Right"), HostResult::Consumed);
        assert_eq!(context.caret(), 2);
    }

    #[test]
    fn navigator_home_end_move_to_edges() {
        let mut context = context_with_menu(&["甲", "乙"], 0);
        assert_eq!(press(&mut context, "Home"), HostResult::Consumed);
        assert_eq!(context.caret(), 0);
        assert_eq!(press(&mut context, "End"), HostResult::Consumed);
        assert_eq!(context.caret(), 2);
    }

    #[test]
    fn navigator_ctrl_shift_arrows_jump_edges() {
        let mut context = context_with_menu(&["甲", "乙"], 0);
        // Ctrl/Shift+Left|Right：按单段跳到首/尾
        assert_eq!(press(&mut context, "Control+Left"), HostResult::Consumed);
        assert_eq!(context.caret(), 0);
        assert_eq!(press(&mut context, "Shift+Right"), HostResult::Consumed);
        assert_eq!(context.caret(), 2);
    }

    #[test]
    fn navigator_consumes_without_menu() {
        let mut context = Context::new();
        context.set_input(b"xx");
        context.composition.segments.push(Segment {
            start: 0,
            end: 2,
            tags: vec!["abc".to_string()],
            translated: true,
            ..Segment::default()
        });
        assert_eq!(press(&mut context, "Left"), HostResult::Consumed);
        assert_eq!(context.caret(), 1);
        assert_eq!(press(&mut context, "Home"), HostResult::Consumed);
        assert_eq!(context.caret(), 0);
    }

    #[test]
    fn editor_backspace_removes_char_before_caret() {
        let mut context = context_with_menu(&["甲", "乙"], 0);
        assert_eq!(press(&mut context, "BackSpace"), HostResult::Consumed);
        assert_eq!(context.input(), b"a");
        assert_eq!(context.caret(), 1);
    }

    #[test]
    fn editor_delete_removes_char_at_caret() {
        let mut context = context_with_menu(&["甲", "乙"], 0);
        // 光标在末尾：Delete 不删除（librime `DeleteInput` 越界返回 false）
        assert_eq!(press(&mut context, "Delete"), HostResult::Consumed);
        assert_eq!(context.input(), b"ab");
        assert_eq!(press(&mut context, "Home"), HostResult::Consumed);
        assert_eq!(press(&mut context, "Delete"), HostResult::Consumed);
        assert_eq!(context.input(), b"b");
    }

    #[test]
    fn editor_passes_idle_editing_keys() {
        let mut context = Context::new();
        assert_eq!(press(&mut context, "BackSpace"), HostResult::Forward);
        assert_eq!(press(&mut context, "Delete"), HostResult::Forward);
        assert_eq!(press(&mut context, "Left"), HostResult::Forward);
    }

    #[test]
    fn key_binder_tab_navigates_candidates() {
        let mut context = context_with_menu(&["甲", "乙"], 0);
        assert_eq!(press(&mut context, "Tab"), HostResult::Consumed);
        assert_eq!(selected(&context), 1);
        assert_eq!(press(&mut context, "Shift+Tab"), HostResult::Consumed);
        assert_eq!(selected(&context), 0);
    }

    #[test]
    fn key_binder_tab_passes_without_menu() {
        let mut empty = Context::new();
        empty.set_input(b"x");
        assert_eq!(press(&mut empty, "Tab"), HostResult::Forward);
    }

    /// 直接构造键事件（绑定测试不依赖 repr 解析）。
    fn press_raw(context: &mut Context, keycode: i32, modifier: i32) -> HostResult {
        process_key(
            &KeyEvent::new(keycode, modifier),
            context,
            None,
            &HostOptions::default(),
            None,
        )
    }

    #[test]
    fn editor_confirm_cancel_and_syllable_bindings() {
        // 参照 `ExpressEditor`：`{XK_space,0}`=Confirm、`{XK_Escape,0}`=CancelComposition、
        // `{XK_BackSpace,kControlMask}`=BackToPreviousSyllable。
        // 这些键在真机路径上会先被方案 `processor` 消费，故金样覆盖不到宿主链，须在此钉住。
        let mut context = context_with_menu(&["甲", "乙"], 0);
        assert_eq!(press(&mut context, "space"), HostResult::Consumed);
        assert!(
            context.composition.segments[0].selected,
            "空格应确认高亮段（ConfirmCurrentSelection）"
        );

        // 参照 `CancelComposition` = `ClearPreviousSegment() || Clear()`：
        // 有段时截到末段起点（可能仍有输入），无段时整体清空。
        let mut context = context_with_menu(&["甲", "乙"], 0);
        assert_eq!(press(&mut context, "Escape"), HostResult::Consumed);
        assert_eq!(context.last_commit_text(), "", "取消不上屏");

        let mut raw_only = Context::new();
        raw_only.push_input(b"ab");
        assert!(raw_only.is_composing());
        assert_eq!(press(&mut raw_only, "Escape"), HostResult::Consumed);
        assert!(!raw_only.is_composing(), "无段时取消应整体清空");

        let mut context = context_with_menu(&["甲", "乙"], 0);
        assert_eq!(
            press_raw(&mut context, 0xff08, K_CONTROL_MASK),
            HostResult::Consumed
        );
    }

    #[test]
    fn editor_ctrl_return_commits_script_text() {
        // 参照 `{XK_Return, kControlMask}` = `CommitScriptText`：提交**脚本文本**（候选文字），
        // 与 `{XK_Return, 0}`（`CommitRawInput`，提交原始输入码）区分开。
        let mut context = context_with_menu(&["甲", "乙"], 0);
        assert_eq!(
            press_raw(&mut context, 0xff0d, K_CONTROL_MASK),
            HostResult::Consumed
        );
        assert_eq!(context.last_commit_text(), "甲", "脚本文本 = 高亮候选文字");

        let mut context = context_with_menu(&["甲", "乙"], 0);
        assert_eq!(press_raw(&mut context, 0xff0d, 0), HostResult::Consumed);
        assert_eq!(
            context.last_commit_text(),
            "ab",
            "原始输入 = 未确认段的原文"
        );
    }

    #[test]
    fn editor_ctrl_shift_return_commits_candidate_comment() {
        // 参照 `{XK_Return, kControlMask | kShiftMask}` = `CommitComment`：提交高亮候选的注释。
        let mut context = Context::new();
        context.set_input(b"ab");
        let mut segment = Segment {
            start: 0,
            end: 2,
            tags: vec!["abc".to_string()],
            translated: true,
            selected_index: 0,
            ..Segment::default()
        };
        segment
            .candidates
            .push(Candidate::new("sentence", 0, 2, "中", "d/dg/dgs"));
        context.composition.segments.push(segment);
        context.drain_events();
        assert_eq!(
            press_raw(&mut context, 0xff0d, K_CONTROL_MASK | K_SHIFT_MASK),
            HostResult::Consumed
        );
        let events = context.drain_events();
        assert!(
            events.iter().any(
                |event| matches!(event, crate::session::Event::Commit(text) if text == "d/dg/dgs")
            ),
            "应提交候选注释：{events:?}"
        );
    }

    #[test]
    fn editor_fallbacks_match_reference_keymap() {
        // 参照 `ExpressEditor` 键表 + `FallbackOptions::All`：Shift 变体回退到无修饰绑定。
        let mut context = context_with_menu(&["甲", "乙"], 0);
        assert_eq!(
            press_raw(&mut context, 0x20, K_SHIFT_MASK),
            HostResult::Consumed
        );

        let mut context = context_with_menu(&["甲", "乙"], 0);
        context.set_input(b"abc");
        assert_eq!(
            press_raw(&mut context, 0xff08, K_SHIFT_MASK),
            HostResult::Consumed
        );

        let mut context = context_with_menu(&["甲", "乙"], 0);
        context.set_input(b"abc");
        assert_eq!(
            press_raw(&mut context, 0xffff, K_SHIFT_MASK),
            HostResult::Consumed
        );
    }
}
