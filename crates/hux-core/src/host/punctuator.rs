// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! 标点处理器：镜像 librime `Punctuator::ProcessKeyEvent`（`digit_separators: ""`，`use_space` 缺省 false）。

use super::commit_notifier::commit_notifier;
use super::{CommitObserver, HostResult};
use crate::key::KeyEvent;
use crate::punct::PunctTable;
use crate::session::Context;

/// 参照 `Punctuator::ProcessKeyEvent`（`digit_separators: ""`，`use_space` 缺省 false）。
///
/// 命中标点表后：**在 caret 处 `PushInput(ch)`**，再按「caret 之前的前缀 + 该标点」计算提交文本
/// ——参照引擎按 `ConcreteEngine::Compose` 的 `active_input = input.substr(0, caret_pos)`
/// 分段，故光标居中时标点段就是末段，提交文本**只取到该段末尾**，其后的剩余输入随
/// `Clear()` 丢弃（实测 `x x Left comma` → 提交「x，」，`a b Left comma` → 「a，」）；
/// 光标在输入末尾时二者等价，与 `ConfirmUniquePunct`/`AutoCommitPunct`/`PairPunct`
/// 在 `_auto_commit` 下的净效果一致（候选菜单形态参照表未使用）。
pub(super) fn punctuator(
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
    // 前缀取 `PushInput` 之前的 caret：标点段起点即 caret，段末即 caret + 1。
    let caret = context.caret().min(context.input().len());
    let head = context.composition.commit_text(&context.input()[..caret]);
    context.push_input(&[keycode as u8]);
    let commit = format!("{head}{text}");
    commit_notifier(observer, context, &commit);
    context.clear();
    context.direct_commit(&commit);
    HostResult::Consumed
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::HostOptions;
    use crate::host::test_support::*;

    fn punct_table() -> PunctTable {
        PunctTable::parse(
                "punctuator:\n  half_shape:\n    \",\": { commit: ， }\n    \"'\": { pair: [ \"‘\", \"’\" ] }\n",
            )
            .expect("punct table")
    }

    /// 表驱动用例的一行：原用例名、初始上下文、按键与逐次期望（夹具与断言逐条等价）。
    struct Case {
        // 原用例名（保留可检索性）。
        name: &'static str,
        // 初始上下文：`true` ⇒ 组合中带菜单（`context_with_menu`），`false` ⇒ 空上下文。
        composing: bool,
        // 按键（`KeyEvent::from_repr` 名称）。
        key: &'static str,
        // 逐次按下的（宿主链结果；`Some` 时断言该次按键后的 `last_commit_text`）。
        steps: &'static [(HostResult, Option<&'static str>)],
        // 末态是否断言输入已清空（仅原「追加」用例断言）。
        input_empty: bool,
    }

    // 一行 = 一条原用例。
    const CASES: &[Case] = &[
        Case {
            name: "punctuator_commits_standalone_punct",
            composing: false,
            key: "comma",
            steps: &[(HostResult::Consumed, Some("，"))],
            input_empty: false,
        },
        Case {
            name: "punctuator_appends_to_composition_text",
            composing: true,
            key: "comma",
            steps: &[(HostResult::Consumed, Some("甲，"))],
            input_empty: true,
        },
        Case {
            name: "punctuator_pair_alternates",
            composing: false,
            key: "apostrophe",
            steps: &[
                (HostResult::Consumed, Some("‘")),
                (HostResult::Consumed, Some("’")),
            ],
            input_empty: false,
        },
        Case {
            name: "punctuator_passes_unmapped_key",
            composing: false,
            key: "space",
            steps: &[(HostResult::Forward, None)],
            input_empty: false,
        },
    ];

    /// 标点宿主链的表驱动用例：原四条独立用例（独立提交 / 追加到组合 / 成对交替 /
    /// 未映射键放行）合并于此——夹具与断言逐条等价；平台侧的端到端版本保留在
    /// `platform/fcitx5/src/tests.rs`（跨层重复只留平台侧）。
    ///
    /// 原用例 → 本表行对照：
    ///
    /// | 原用例 | 本表行 | 逐次断言 |
    /// | --- | --- | --- |
    /// | `punctuator_commits_standalone_punct` | 1 | 空上下文 `comma` ⇒ `Consumed` + 上屏「，」 |
    /// | `punctuator_appends_to_composition_text` | 2 | 组合中 `comma` ⇒ `Consumed` + 上屏「甲，」+ 输入清空 |
    /// | `punctuator_pair_alternates` | 3 | 连按 `apostrophe` ⇒ `Consumed`/「‘」、`Consumed`/「’」 |
    /// | `punctuator_passes_unmapped_key` | 4 | 空上下文 `space` ⇒ `Forward`（原用例不断言上屏） |
    #[test]
    fn punctuator_commits_appends_pairs_and_passes_unmapped() {
        let table = punct_table();
        for case in CASES {
            let mut context = if case.composing {
                context_with_menu(&["甲", "乙"], 0)
            } else {
                Context::new()
            };
            for (index, (result, commit)) in case.steps.iter().enumerate() {
                assert_eq!(
                    process(
                        &mut context,
                        case.key,
                        Some(&table),
                        &HostOptions::default()
                    ),
                    *result,
                    "{}：第 {} 次按键",
                    case.name,
                    index + 1
                );
                if let Some(commit) = commit {
                    assert_eq!(
                        context.last_commit_text(),
                        *commit,
                        "{}：第 {} 次按键后的上屏文本",
                        case.name,
                        index + 1
                    );
                }
            }
            if case.input_empty {
                assert!(context.input().is_empty(), "{}：输入应清空", case.name);
            }
        }
    }

    #[test]
    fn punctuator_without_table_passes() {
        let mut context = Context::new();
        assert_eq!(
            process(&mut context, "comma", None, &HostOptions::default()),
            HostResult::Forward
        );
    }

    /// 光标居中时参照在 caret 处插入标点，提交文本**只取到该段末尾**
    /// （`ConcreteEngine::Compose` 的 `active_input = input[..caret]`），标点后的剩余输入丢弃。
    #[test]
    fn punctuator_at_mid_caret_commits_only_up_to_the_punctuation() {
        let table = punct_table();
        let mut context = Context::new();
        context.set_input(b"ab");
        context.set_caret(1);
        context.drain_events();
        assert_eq!(
            process(&mut context, "comma", Some(&table), &HostOptions::default()),
            HostResult::Consumed
        );
        assert_eq!(context.last_commit_text(), "a，", "只提交 caret 之前的前缀");
        assert!(context.input().is_empty());
        assert_eq!(context.caret(), 0);
    }
}
