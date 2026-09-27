// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;

/// `hux_engine_reset`（宿主 `deactivate`/`reset`）：清空组合与字反查态，会话 id 继续可用。
#[test]
fn engine_reset_via_abi_clears_the_session() {
    let _guard = serial();
    let dir = temp_user_dir("abi-reset");
    let engine = Box::into_raw(Box::new(Engine::new_with_dirs(
        host(),
        fixture_dirs(),
        None,
        Some(dir.clone()),
    )));
    let session = unsafe { hux_engine_session_new(engine) };
    assert!(session > 0, "会话应创建成功");
    assert_ne!(
        unsafe { hux_engine_key(engine, session, u32::from(b'a'), 0, 0) } & HUX_KEY_CONSUMED,
        0,
        "夹具码表里 a 应被消费（组合已开始）"
    );
    assert_eq!(unsafe { &*engine }.sessions[&session].context.input(), b"a");

    unsafe { hux_engine_reset(engine, session) };
    let state = unsafe { &*engine };
    assert!(
        state.sessions.contains_key(&session),
        "重置不释放会话（宿主不必重建输入上下文）"
    );
    assert_eq!(
        state.sessions[&session].context.input(),
        b"",
        "重置应清空组合"
    );
    assert!(
        !state.sessions[&session].reverse_lookup.valid,
        "重置应清掉字反查态"
    );
    // 同一 id 还能重新开始组合。
    assert_ne!(
        unsafe { hux_engine_key(engine, session, u32::from(b'a'), 0, 0) } & HUX_KEY_CONSUMED,
        0
    );
    unsafe { hux_engine_free(engine) };
    std::fs::remove_dir_all(&dir).ok();
}

/// `hux_engine_reset` 的边界：空指针与未知会话 id 都是 no-op（不崩、不建会话）。
#[test]
fn engine_reset_via_abi_tolerates_null_and_unknown_session() {
    let _guard = serial();
    unsafe { hux_engine_reset(std::ptr::null_mut(), 1) };

    let dir = temp_user_dir("abi-reset-boundary");
    let engine = Box::into_raw(Box::new(Engine::new_with_dirs(
        host(),
        fixture_dirs(),
        None,
        Some(dir.clone()),
    )));
    let session = unsafe { hux_engine_session_new(engine) };
    let before = unsafe { &*engine }.sessions.len();
    unsafe { hux_engine_reset(engine, session + 100) };
    assert_eq!(
        unsafe { &*engine }.sessions.len(),
        before,
        "未知会话 id 不应被创建"
    );
    unsafe { hux_engine_free(engine) };
    std::fs::remove_dir_all(&dir).ok();
}

/// `hux_engine_select_candidate`（面板候选点击）：按全局索引选中并上屏，返回 1。
#[test]
fn candidate_click_via_abi_commits_selected_candidate() {
    let _guard = serial();
    COMMITS.lock().unwrap().clear();
    UPDATES.lock().unwrap().clear();
    let dir = temp_user_dir("abi-select-candidate");
    let engine = Box::into_raw(Box::new(Engine::new_with_dirs(
        host(),
        fixture_dirs(),
        None,
        Some(dir.clone()),
    )));
    let session = unsafe { hux_engine_session_new(engine) };
    for code in *b"ab" {
        unsafe { hux_engine_key(engine, session, u32::from(code), 0, 0) };
    }
    let candidates = last_update().2;
    assert!(candidates.len() >= 2, "夹具 ab 应有至少 2 个候选");
    let second = candidates[1].clone();
    assert_eq!(
        unsafe { hux_engine_select_candidate(engine, session, 1) },
        1,
        "页内候选应被处理"
    );
    assert_eq!(COMMITS.lock().unwrap().last().unwrap(), &second);
    assert!(
        unsafe { &*engine }.sessions[&session]
            .context
            .input()
            .is_empty(),
        "上屏后组合应清空"
    );
    unsafe { hux_engine_free(engine) };
    std::fs::remove_dir_all(&dir).ok();
}

/// `hux_engine_select_candidate` 的边界：空引擎 / 无可选段 / 负索引 / 越界索引 / 未知会话
/// 都返回 0（忽略点击），且不产生提交、组合不变。
#[test]
fn candidate_click_via_abi_ignores_out_of_range_index() {
    let _guard = serial();
    COMMITS.lock().unwrap().clear();
    assert_eq!(
        unsafe { hux_engine_select_candidate(std::ptr::null_mut(), 1, 0) },
        0,
        "空引擎返回 0"
    );

    let dir = temp_user_dir("abi-select-boundary");
    let engine = Box::into_raw(Box::new(Engine::new_with_dirs(
        host(),
        fixture_dirs(),
        None,
        Some(dir.clone()),
    )));
    let session = unsafe { hux_engine_session_new(engine) };
    assert_eq!(
        unsafe { hux_engine_select_candidate(engine, session, 0) },
        0,
        "没有可选段时点击被忽略"
    );
    for code in *b"ab" {
        unsafe { hux_engine_key(engine, session, u32::from(code), 0, 0) };
    }
    assert_eq!(
        unsafe { hux_engine_select_candidate(engine, session, -1) },
        0,
        "负索引返回 0"
    );
    assert_eq!(
        unsafe { hux_engine_select_candidate(engine, session, 99) },
        0,
        "越界索引返回 0"
    );
    assert_eq!(
        unsafe { hux_engine_select_candidate(engine, session + 100, 0) },
        0,
        "未知会话 id 返回 0"
    );
    assert!(COMMITS.lock().unwrap().is_empty(), "忽略的点击不得产生提交");
    assert_eq!(
        unsafe { &*engine }.sessions[&session].context.input(),
        b"ab"
    );
    unsafe { hux_engine_free(engine) };
    std::fs::remove_dir_all(&dir).ok();
}

/// `hux_engine_set_surrounding`：文本与字符制光标进会话的字反查态，返回 1。
#[test]
fn surrounding_via_abi_records_text_and_cursor() {
    let _guard = serial();
    let dir = temp_user_dir("abi-surrounding");
    let engine = Box::into_raw(Box::new(Engine::new_with_dirs(
        host(),
        fixture_dirs(),
        None,
        Some(dir.clone()),
    )));
    let session = unsafe { hux_engine_session_new(engine) };
    let text = CString::new("中欧中兴").expect("无 NUL 字节");
    assert_eq!(
        unsafe { hux_engine_set_surrounding(engine, session, text.as_ptr(), 2, 1) },
        1,
        "可用周边文本应被受理"
    );
    let state = unsafe { &*engine };
    assert!(
        state.sessions[&session].reverse_lookup.valid,
        "受理后字反查态应有效"
    );
    assert_eq!(state.sessions[&session].reverse_lookup.text, "中欧中兴");
    assert_eq!(
        state.sessions[&session].reverse_lookup.cursor, 2,
        "字符制光标"
    );
    // 光标超出文本长度：按文本长度夹住（与内核的字符制光标约定一致）。
    assert_eq!(
        unsafe { hux_engine_set_surrounding(engine, session, text.as_ptr(), 99, 1) },
        1
    );
    assert_eq!(
        unsafe { &*engine }.sessions[&session].reverse_lookup.cursor,
        4
    );
    unsafe { hux_engine_free(engine) };
    std::fs::remove_dir_all(&dir).ok();
}

/// `hux_engine_set_surrounding` 的边界：空引擎返回 0；`valid = 0` / 文本指针为空 ⇒ 按
/// 「应用不可用」清空会话态；负光标夹到 0。均不崩。
#[test]
fn surrounding_via_abi_tolerates_null_and_invalid() {
    let _guard = serial();
    assert_eq!(
        unsafe { hux_engine_set_surrounding(std::ptr::null_mut(), 1, std::ptr::null(), 0, 0) },
        0,
        "空引擎返回 0"
    );

    let dir = temp_user_dir("abi-surrounding-boundary");
    let engine = Box::into_raw(Box::new(Engine::new_with_dirs(
        host(),
        fixture_dirs(),
        None,
        Some(dir.clone()),
    )));
    let session = unsafe { hux_engine_session_new(engine) };
    let text = CString::new("中欧中兴").expect("无 NUL 字节");
    assert_eq!(
        unsafe { hux_engine_set_surrounding(engine, session, text.as_ptr(), 2, 1) },
        1
    );

    // 应用不支持周边文本（valid = 0）：清空。
    assert_eq!(
        unsafe { hux_engine_set_surrounding(engine, session, std::ptr::null(), 0, 0) },
        1,
        "不可用也算受理（宿主不必区分）"
    );
    let state = unsafe { &*engine };
    assert!(
        !state.sessions[&session].reverse_lookup.valid
            && state.sessions[&session].reverse_lookup.text.is_empty()
            && state.sessions[&session].reverse_lookup.cursor == 0,
        "不可用应清空字反查态"
    );

    // 文本指针为空（valid 仍为 1）：同「不可用」。
    assert_eq!(
        unsafe { hux_engine_set_surrounding(engine, session, std::ptr::null(), 0, 1) },
        1
    );
    assert!(
        !(unsafe { &*engine }).sessions[&session]
            .reverse_lookup
            .valid,
        "空文本指针应清空字反查态"
    );

    // 负光标：夹到 0（字符制光标不可能为负）。
    assert_eq!(
        unsafe { hux_engine_set_surrounding(engine, session, text.as_ptr(), -1, 1) },
        1
    );
    let state = unsafe { &*engine };
    assert!(
        state.sessions[&session].reverse_lookup.valid
            && state.sessions[&session].reverse_lookup.cursor == 0,
        "负光标夹到 0"
    );
    unsafe { hux_engine_free(engine) };
    std::fs::remove_dir_all(&dir).ok();
}
