// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! 锁定重放用例：前缀重建、全输入锁定、部分锁定、失配拒绝与不透明前缀。

use super::*;

/// 锁定前缀代表已上屏内容：重建出的每个候选都必须以它开头。
#[test]
fn locked_decode_rebuilds_confirmed_prefix() {
    let dir = hux_test_support::repo_path("goldens/lexicon");
    let lexicon = Lexicon::load(std::slice::from_ref(&dir), 1500);
    let supplement = Supplement::load_default(Some(&dir));
    let mut decoder = Decoder::new(lexicon, supplement, None);
    let unlocked = decoder.decode_with("ab", false, "").expect("decode");
    let top = unlocked.items.first().expect("candidates").clone();
    // 路径链 root..top；取第一个非根节点作为局部锁边界。
    let mut chain = Vec::new();
    let mut current = Some(top.path);
    while let Some(index) = current {
        chain.push(index);
        current = decoder.arena[index].previous;
    }
    chain.reverse();
    assert!(chain.len() >= 2, "期望多节点路径");
    let node = chain[1];
    let (raw_length, text_length) = (
        decoder.arena[node].raw_length,
        decoder.arena[node].text_length,
    );
    let locked_text = decoder.arena[node].text.clone();
    let locked_raw = "ab"[..raw_length].to_string();
    let boundaries = format!("{raw_length},{text_length};");
    let lock = DecodeLock {
        raw: &locked_raw,
        text: &locked_text,
        boundaries: &boundaries,
    };
    let locked = decoder
        .decode_with_lock("ab", false, "", Some(lock))
        .expect("locked decode");
    assert!(!locked.items.is_empty(), "锁住前缀后仍必须给出候选");
    for item in &locked.items {
        assert!(item.text.starts_with(&locked_text), "{}", item.text);
    }
}

/// 边界覆盖整段输入的全量锁必须原样复现顶层候选文本。
#[test]
fn locked_decode_honors_full_input_lock() {
    let dir = hux_test_support::repo_path("goldens/lexicon");
    let lexicon = Lexicon::load(std::slice::from_ref(&dir), 1500);
    let supplement = Supplement::load_default(Some(&dir));
    let mut decoder = Decoder::new(lexicon, supplement, None);
    let unlocked = decoder.decode_with("ab", false, "").expect("decode");
    let top = unlocked.items.first().expect("candidates").clone();
    let mut chain = Vec::new();
    let mut current = Some(top.path);
    while let Some(index) = current {
        chain.push(index);
        current = decoder.arena[index].previous;
    }
    chain.reverse();
    // 全量锁：以顶层候选路径的全部边界重建，前缀即整段输入。
    let boundaries: String = chain[1..]
        .iter()
        .map(|&index| {
            format!(
                "{},{};",
                decoder.arena[index].raw_length, decoder.arena[index].text_length
            )
        })
        .collect();
    let lock = DecodeLock {
        raw: "ab",
        text: &top.text,
        boundaries: &boundaries,
    };
    let locked = decoder
        .decode_with_lock("ab", false, "", Some(lock))
        .expect("locked decode");
    assert!(!locked.items.is_empty(), "全量锁下候选不得为空");
    assert!(
        locked.items.iter().all(|item| item.text == top.text),
        "{:?}",
        locked
            .items
            .iter()
            .map(|item| &item.text)
            .collect::<Vec<_>>()
    );
}

/// 锁只钉住前缀，剩余输入仍要正常解码并允许扩展出多字候选。
#[test]
fn locked_decode_expands_after_partial_lock() {
    let dir = hux_test_support::repo_path("goldens/lexicon");
    let lexicon = Lexicon::load(std::slice::from_ref(&dir), 1500);
    let supplement = Supplement::load_default(Some(&dir));
    let mut decoder = Decoder::new(lexicon, supplement, None);
    // 码表事实：ab → 交（rank 1）、疒（rank 2）；整段输入 >1 字节时单字节尾边被跳过，
    // 故 "abab" 唯一两段路径为 ab+ab。锁住首边后应继续解出 交交/交疒。
    let lock = DecodeLock {
        raw: "ab",
        text: "交",
        boundaries: "2,3;",
    };
    let locked = decoder
        .decode_with_lock("abab", false, "", Some(lock))
        .expect("locked decode");
    assert!(!locked.items.is_empty(), "局部锁后仍要解出剩余输入");
    assert!(
        locked.items.iter().all(|item| item.text.starts_with("交")),
        "所有候选都必须沿用锁定的前缀文本"
    );
    assert!(
        locked
            .items
            .iter()
            .any(|item| item.text.chars().count() > 1),
        "扩展应产生多字候选：{:?}",
        locked
            .items
            .iter()
            .map(|item| &item.text)
            .collect::<Vec<_>>()
    );
}

/// 失配锁用例的夹具：返回 `(解码器, raw_length, locked_text, locked_raw, boundaries)`，
/// 锁参数取自 `ab` 顶层候选的父节点。
fn locked_mismatch_fixture() -> (Decoder, usize, String, String, String) {
    let dir = hux_test_support::repo_path("goldens/lexicon");
    let lexicon = Lexicon::load(std::slice::from_ref(&dir), 1500);
    let supplement = Supplement::load_default(Some(&dir));
    let mut decoder = Decoder::new(lexicon, supplement, None);
    let unlocked = decoder.decode_with("ab", false, "").expect("decode");
    let top = unlocked.items.first().expect("candidates").clone();
    let node = decoder.arena[top.path].previous.expect("non-root path");
    let (raw_length, text_length) = (
        decoder.arena[node].raw_length,
        decoder.arena[node].text_length,
    );
    let locked_text = decoder.arena[node].text.clone();
    let locked_raw = "ab"[..raw_length].to_string();
    let boundaries = format!("{raw_length},{text_length};");
    (decoder, raw_length, locked_text, locked_raw, boundaries)
}

/// 原始串不符、空串、零边界、文本长度不符都必须整条拒绝，不得静默降级成无锁解码。
#[test]
fn locked_decode_rejects_mismatches() {
    let (mut decoder, raw_length, locked_text, locked_raw, boundaries) = locked_mismatch_fixture();
    let raw_mismatch = DecodeLock {
        raw: "xy",
        text: &locked_text,
        boundaries: &boundaries,
    };
    assert!(
        decoder
            .decode_with_lock("ab", false, "", Some(raw_mismatch))
            .unwrap()
            .items
            .is_empty(),
        "锁前缀与输入不符：整条拒绝给空候选"
    );
    let empty_raw = DecodeLock {
        raw: "",
        text: &locked_text,
        boundaries: &boundaries,
    };
    assert!(
        decoder
            .decode_with_lock("ab", false, "", Some(empty_raw))
            .unwrap()
            .items
            .is_empty(),
        "空锁前缀视为不匹配，不得当成无锁"
    );
    let short = "0,0;".to_string();
    let short_lock = DecodeLock {
        raw: &locked_raw,
        text: &locked_text,
        boundaries: &short,
    };
    assert!(
        decoder
            .decode_with_lock("ab", false, "", Some(short_lock))
            .unwrap()
            .items
            .is_empty(),
        "零边界与锁文本不符，必须拒绝"
    );
    // 边界文本长度与锁文本不一致（"a" != "ab"）
    let text_boundary = format!("{raw_length},1;");
    let text_lock = DecodeLock {
        raw: &locked_raw,
        text: "ab",
        boundaries: &text_boundary,
    };
    assert!(
        decoder
            .decode_with_lock("ab", false, "", Some(text_lock))
            .unwrap()
            .items
            .is_empty(),
        "边界文本长度与锁文本不一致也必须拒绝"
    );
}

/// 文本级退格会产生与码表边不对应的锁，此时按中立码证据重放而不是整段拒绝。
#[test]
fn locked_decode_replays_opaque_prefix_neutrally() {
    let dir = hux_test_support::repo_path("goldens/lexicon");
    let lexicon = Lexicon::load(std::slice::from_ref(&dir), 1500);
    let supplement = Supplement::load_default(Some(&dir));
    let mut decoder = Decoder::new(lexicon, supplement, None);
    // 锁文本与任何码表边都不对应（文本级退格产生的"不透明"锁）：
    // 参照 12d2ecc 起以中立码证据重放，而不是整段拒绝。
    let lock = DecodeLock {
        raw: "ab",
        text: "某某",
        boundaries: "2,6;",
    };
    let locked = decoder
        .decode_with_lock("abab", false, "", Some(lock))
        .expect("locked decode");
    assert!(!locked.items.is_empty(), "不透明锁前缀也要给出候选");
    assert!(
        locked
            .items
            .iter()
            .all(|item| item.text.starts_with("某某")),
        "{:?}",
        locked
            .items
            .iter()
            .map(|item| &item.text)
            .collect::<Vec<_>>()
    );
}
