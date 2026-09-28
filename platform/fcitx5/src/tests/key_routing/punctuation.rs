// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! 标点：空闲直接提交、组合中先确认组合再独立提交标点、成对交替、未映射空格放行。

use super::*;

#[test]
fn punctuation_commits_when_idle() {
    let _guard = serial();
    COMMITS.lock().unwrap().clear();
    let mut engine = TestEngine::new(host(), fixture_dirs(), None, None);
    // symbols.yaml half_shape："." → 。
    assert!(engine.key(0x2e, 0, false), "period 应被消费");
    assert_eq!(COMMITS.lock().unwrap().last().unwrap(), "。");
}

/// 组合中按标点：**先确认组合**（提交当前候选「甲」），标点随后**独立提交**（「，」）——
/// 标点并不追加进组合（原名 `punctuation_appends_to_composition` 与断言相反）。
#[test]
fn punctuation_confirms_composition_then_commits() {
    let _guard = serial();
    COMMITS.lock().unwrap().clear();
    let mut engine = TestEngine::new(host(), fixture_dirs(), None, None);
    assert!(engine.key(u32::from(b'a'), 0, false));
    assert!(engine.key(u32::from(b'b'), 0, false));
    assert!(engine.key(0x2c, 0, false), "comma 应被消费");
    // 参照 `abad411`：菜单可见时处理器先确认组合（librime `ConcreteEngine::OnSelect`
    // 在 `_auto_commit` 下同步 `Commit()`），标点随后独立落字；提交文本合计不变。
    assert_eq!(
        COMMITS.lock().unwrap().clone(),
        vec!["甲".to_string(), "，".to_string()]
    );
    assert!(engine.session().context.input().is_empty());
}

#[test]
fn punctuation_pair_alternates() {
    let _guard = serial();
    COMMITS.lock().unwrap().clear();
    let mut engine = TestEngine::new(host(), fixture_dirs(), None, None);
    // apostrophe：'‘' / '’'
    for text in ["‘", "’"] {
        assert!(engine.key(0x27, 0, false));
        assert_eq!(COMMITS.lock().unwrap().last().unwrap(), text);
    }
}

#[test]
fn punctuation_passes_unmapped_space() {
    let _guard = serial();
    let mut engine = TestEngine::new(host(), fixture_dirs(), None, None);
    assert!(!engine.key(0x20, 0, false), "空闲空格交宿主");
}
