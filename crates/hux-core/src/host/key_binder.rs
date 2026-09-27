// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! 按键绑定与翻页判据：镜像 librime `KeyBinder`（参照 schema 的 `Tab`/`Shift+Tab` 绑定；翻页键见 [`paging_action`]）。

use super::selector::{SelectorAction, selector_action};
use super::{HostOptions, HostResult};
use crate::key::{K_SHIFT_MASK, KeyEvent};
use crate::session::Context;

/// 翻页方向（[`paging_action`] 的结果；`Up` = Page_Up 等价动作，`Down` = Page_Down）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PagingDir {
    /// 上翻页（[`SelectorAction::PreviousPage`]）。
    Up,
    /// 下翻页（[`SelectorAction::NextPage`]）。
    Down,
}

/// `key_binder` 的翻页判据：该键是否会被判为翻页。
///
/// 两侧**同一前置条件**（[`menu_available`] = `!ascii_mode && has_menu`）：
/// - 命中 `page_up_keys` → [`PagingDir::Up`]；
/// - 命中 `page_down_keys` → [`PagingDir::Down`]；
/// - 其余（菜单不可用 / 未命中绑定）→ `None`。
///
/// 宿主绑定与方案处理器共用本判据（避免两处条件漂移）。方案侧在「菜单可见 + 可打印 ASCII 标点」
/// 分支入口先问一次：被宿主判为翻页的键（如缺省 `=`/`-`，以及 schema 绑到翻页的 `[`/`]`）
/// 不由该分支消费，让出被其遮蔽的翻页绑定——**本仓有意偏离上游**。
///
/// **上翻页与下翻页同前置，是本仓的语义强化**：参照的 `-` 绑定带
/// `when: paging`（`key_binder.cc:248-266` 的 `kWhenPaging` **只看末段 `paging` 标签**，
/// 先翻过页才吃该键），本仓改为与下翻页同前置「菜单可见即拦截」，不再看标签。
/// **已知并接受的代价**：菜单可见时 `-`/`=`/`[`/`]` 不再能作为标点打出（被判为翻页而消费）。
/// 偏离登记见 `crates/hux-scheme/tiger/tests/key_sequence_differential.rs` 的 `DEVIATIONS`。
///
/// `ascii_mode` 前置**两侧都保留**：`ascii_mode` 打开时翻页键一律不拦截，仍落标点/原路径。
pub fn paging_action(
    context: &Context,
    options: &HostOptions,
    key_event: &KeyEvent,
) -> Option<PagingDir> {
    // 参照 `key_binder.cc` 的绑定查表（`map<KeyEvent,…>::find(key_event)`）是**精确**的
    // `(keycode, modifier)` 比较，而非 `repr()` 字符串比较：后者每次按键多一次分配，
    // 且 `K_MODIFIER_MASK` 内的**无名位**（16-20/24/25）会让不同修饰状态的键在字符串上碰撞。
    let bound = |keys: &[KeyEvent]| {
        keys.iter()
            .any(|key| key.keycode == key_event.keycode && key.modifier == key_event.modifier)
    };
    if !menu_available(context) {
        return None;
    }
    if bound(&options.page_up_keys) {
        return Some(PagingDir::Up);
    }
    if bound(&options.page_down_keys) {
        return Some(PagingDir::Down);
    }
    None
}

/// 参照 `KeyBinder` 的前置条件：非 `ascii_mode` 且 `has_menu`
/// （上/下翻页判据与 [`key_binder`] 的 Tab/Ctrl 绑定共用）。
fn menu_available(context: &Context) -> bool {
    !context.get_option("ascii_mode") && context.has_menu()
}

/// 参照 `KeyBinder::ProcessKeyEvent`：Tab/Shift+Tab 固定，翻页键取 [`HostOptions`]。
/// 翻页判据统一走 [`paging_action`]（菜单不可用时翻页键**不消费**——
/// 交后续处理器落作标点/输入）。
pub(super) fn key_binder(
    key_event: &KeyEvent,
    context: &mut Context,
    options: &HostOptions,
) -> HostResult {
    if let Some(dir) = paging_action(context, options, key_event) {
        return match dir {
            PagingDir::Up => selector_action(SelectorAction::PreviousPage, context, options),
            PagingDir::Down => selector_action(SelectorAction::NextPage, context, options),
        };
    }
    if !menu_available(context) {
        return HostResult::Forward;
    }
    // 固定绑定表同样是精确的 `(keycode, modifier)` 比较：`{Tab,0}` → 下一候选、
    // `{Tab,Shift}` → 上一候选。注意 X11 的 `ISO_Left_Tab`（0xfe20）**不在**该表里
    // （与参照一致）：它由方案处理器/Tab 循环处理，落到宿主链时照旧 Forward。
    if key_event.keycode == 0xff09 && key_event.modifier == 0 {
        return selector_action(SelectorAction::NextCandidate, context, options);
    }
    if key_event.keycode == 0xff09 && key_event.modifier == K_SHIFT_MASK {
        return selector_action(SelectorAction::PreviousCandidate, context, options);
    }
    HostResult::Forward
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::test_support::*;
    use crate::punct::PunctTable;

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

    /// 回归场景：`Page_Up` 停在首页后，紧随的上翻页键
    /// `-` 必须**翻页**（消费、不提交），不得落标点分支把组合提前上屏。
    ///
    /// 判据来源是「菜单可见」而非「翻页写入 `paging` 标签」
    /// （[`paging_action`]），故用例在按 `Page_Up` **之前**就断言 `Some(Up)`——
    /// 若有人把标签判据加回来（而标签已无人写），本用例立刻失败。
    #[test]
    fn page_up_key_holds_at_the_first_page() {
        let options = HostOptions::default();
        let mut context = context_with_menu(&["a", "b", "c", "d", "e", "f"], 0);
        assert_eq!(
            paging_action(&context, &options, &key_of("minus")),
            Some(PagingDir::Up),
            "菜单可见即判上翻页（不要求 `paging` 标签）"
        );
        assert_eq!(press(&mut context, "Page_Up"), HostResult::Consumed);
        assert_eq!(
            paging_action(&context, &options, &key_of("minus")),
            Some(PagingDir::Up),
            "首页上翻后判据不变（高亮仍 0，菜单仍在）"
        );
        // 宿主链：`-` 走翻页（消费、不提交、输入不变、高亮留在首页）。
        assert_eq!(press(&mut context, "minus"), HostResult::Consumed);
        assert_eq!(context.last_commit_text(), "");
        assert_eq!(context.input(), b"ab");
        assert_eq!(selected(&context), 0);
    }

    /// 上翻页键**只要菜单可见**就判为翻页并消费，不要求
    /// 「已翻过页」（参照的 `when: paging` 标签在本仓已删）；菜单不可用（无菜单 / `ascii_mode`）时
    /// 不消费——该键继续下落（无标点表时由 `editor` 的可打印字符路径提交组合，
    /// 有标点表时落作标点）。
    #[test]
    fn selector_page_up_requires_a_visible_menu() {
        let mut fresh = context_with_menu(&["a", "b", "c", "d", "e", "f"], 0);
        assert_eq!(
            press(&mut fresh, "minus"),
            HostResult::Consumed,
            "菜单可见时上翻页键生效（首屏亦然）"
        );
        assert_eq!(selected(&fresh), 0, "已在首页 ⇒ 归零高亮（不循环）");
        assert_eq!(fresh.last_commit_text(), "");
        assert_eq!(fresh.input(), b"ab", "翻页不改动输入");

        // 无菜单：不消费（交宿主）。
        let mut idle = Context::new();
        assert_eq!(press(&mut idle, "minus"), HostResult::Forward);

        // `ascii_mode`：两侧翻页键都不拦截（`menu_available` 前置）——键落回后续处理器，
        // 而不是被 `key_binder` 吞成翻页（证据：组合被后续处理器提交，翻页从不提交）。
        let options = HostOptions::default();
        let mut ascii = context_with_menu(&["甲", "乙"], 0);
        ascii.set_option("ascii_mode", true);
        assert_eq!(paging_action(&ascii, &options, &key_of("minus")), None);
        assert_eq!(paging_action(&ascii, &options, &key_of("equal")), None);
        assert_eq!(
            press(&mut ascii, "minus"),
            HostResult::Forward,
            "键交宿主的可打印字符路径（翻页会返回 Consumed）"
        );
        assert_eq!(
            ascii.last_commit_text(),
            "甲",
            "键落回后续处理器（editor 提交组合）；翻页路径不提交"
        );
        assert!(ascii.input().is_empty());
    }

    /// 方案侧「菜单可见 + 标点」分支与宿主 `key_binder` 共用此判据：
    /// 缺省绑定 `=` → Down、`-` → Up，**两侧同前置**（菜单可见；`ascii_mode` 关闭两侧）；
    /// 判据不看 `paging` 标签（本仓语义强化，见函数文档）。
    #[test]
    fn paging_action_is_the_shared_key_binder_predicate() {
        let options = HostOptions::default();
        let mut menu = context_with_menu(&["a", "b", "c", "d", "e", "f"], 0);
        assert_eq!(
            paging_action(&menu, &options, &key_of("equal")),
            Some(PagingDir::Down)
        );
        assert_eq!(
            paging_action(&menu, &options, &key_of("minus")),
            Some(PagingDir::Up),
            "菜单可见即判上翻页（不要求先翻过页）"
        );
        assert_eq!(press(&mut menu, "equal"), HostResult::Consumed);
        assert_eq!(
            paging_action(&menu, &options, &key_of("minus")),
            Some(PagingDir::Up),
            "翻页后判据不变"
        );

        // 无菜单 / `ascii_mode`：两侧一律不成立。
        let idle = Context::new();
        assert_eq!(paging_action(&idle, &options, &key_of("equal")), None);
        assert_eq!(paging_action(&idle, &options, &key_of("minus")), None);
        let mut ascii = context_with_menu(&["a", "b"], 0);
        ascii.set_option("ascii_mode", true);
        assert_eq!(paging_action(&ascii, &options, &key_of("equal")), None);
        assert_eq!(paging_action(&ascii, &options, &key_of("minus")), None);
        // 标签不再参与判据：即便人为置位（本仓已无写入方），`ascii_mode` 下仍不判翻页。
        ascii
            .composition
            .back_mut()
            .expect("段")
            .tags
            .push("paging".to_string());
        assert_eq!(
            paging_action(&ascii, &options, &key_of("minus")),
            None,
            "`paging` 标签不是判据（判据是菜单可见）"
        );

        // schema 绑定的其它翻页键按 options 生效（`[`/`]`），未绑定的键不判翻页。
        let custom = HostOptions {
            page_size: 2,
            page_up_keys: vec![key_of("bracketleft")],
            page_down_keys: vec![key_of("bracketright")],
            page_cycle: false,
        };
        let mut menu = context_with_menu(&["a", "b", "c", "d", "e", "f"], 0);
        assert_eq!(
            paging_action(&menu, &custom, &key_of("bracketright")),
            Some(PagingDir::Down)
        );
        assert_eq!(
            paging_action(&menu, &custom, &key_of("bracketleft")),
            Some(PagingDir::Up),
            "`[` 与 `-` 同前置（菜单可见即翻页）"
        );
        assert_eq!(
            paging_action(&menu, &custom, &key_of("equal")),
            None,
            "未绑定为翻页键的 `=` 不判翻页（该配置下落标点）"
        );
        assert_eq!(
            press_with(&mut menu, "bracketright", &custom),
            HostResult::Consumed
        );
        assert_eq!(
            paging_action(&menu, &custom, &key_of("bracketleft")),
            Some(PagingDir::Up)
        );
    }

    /// **负向对照（用户决定 B 的另一半）**：`ascii_mode` 打开时翻页键**不**被拦截，
    /// 仍落标点路径（`punctuator` 消费并提交「组合 + 标点」）；同一上下文关掉 `ascii_mode`
    /// 则判为翻页（消费、不提交、输入不变）。
    #[test]
    fn ascii_mode_paging_keys_fall_through_to_punctuation() {
        let table = PunctTable::parse(
            "punctuator:\n  half_shape:\n    \"-\": { commit: － }\n    \"=\": { commit: ＝ }\n",
        )
        .expect("punct table");
        let options = HostOptions::default();
        // `ascii_mode` 打开：`-`/`=` 都不判翻页。
        let mut ascii = context_with_menu(&["甲", "乙"], 0);
        ascii.set_option("ascii_mode", true);
        assert_eq!(
            paging_action(&ascii, &options, &key_of("minus")),
            None,
            "`ascii_mode` 下上翻页键不拦截"
        );
        assert_eq!(
            process(&mut ascii, "minus", Some(&table), &options),
            HostResult::Consumed,
            "`-` 落标点分支"
        );
        assert_eq!(ascii.last_commit_text(), "甲－", "确认组合后落标点");
        assert!(ascii.input().is_empty());
        // 同一形态的上下文关掉 `ascii_mode`：判为翻页（不提交、输入不变）。
        let mut menu = context_with_menu(&["甲", "乙"], 0);
        assert_eq!(
            process(&mut menu, "minus", Some(&table), &options),
            HostResult::Consumed
        );
        assert_eq!(menu.last_commit_text(), "", "翻页不提交");
        assert_eq!(menu.input(), b"ab", "翻页不改动输入");
    }
}
