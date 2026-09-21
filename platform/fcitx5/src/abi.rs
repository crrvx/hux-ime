// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! C ABI：C 布局类型与 `extern "C"` 导出（C++ 薄壳调用；与 `../../crates/hux-ffi/include/hux_abi.h` 一一对应）。

use std::ffi::c_char;

use crate::engine::Engine;
use hux_cfg::{CandidateLayout, PreeditMode, Settings};

use hux_core::key::{
    K_ALT_MASK, K_CONTROL_MASK, K_LOCK_MASK, K_RELEASE_MASK, K_SHIFT_MASK, K_SUPER_MASK, KeyEvent,
};

pub(crate) use hux_ffi::{
    HUX_KEY_CONSUMED, HUX_KEY_FORWARD_AFTER_COMMIT, HUX_MAX_KEYS, HostCallback, HuxKeyList,
    HuxOptions,
};

// fcitx5 `KeyState` 位（`fcitx-utils/keysym.h`）。
pub(crate) const FCITX_SHIFT: u32 = 1 << 0;
pub(crate) const FCITX_CAPS_LOCK: u32 = 1 << 1;
pub(crate) const FCITX_CTRL: u32 = 1 << 2;
pub(crate) const FCITX_ALT: u32 = 1 << 3;
pub(crate) const FCITX_SUPER: u32 = 1 << 6;

/// fcitx5 `KeyState` → core（Rime）掩码。
pub(crate) fn core_modifiers(states: u32, release: bool) -> i32 {
    let mut modifiers = 0;
    if states & FCITX_SHIFT != 0 {
        modifiers |= K_SHIFT_MASK;
    }
    if states & FCITX_CAPS_LOCK != 0 {
        modifiers |= K_LOCK_MASK;
    }
    if states & FCITX_CTRL != 0 {
        modifiers |= K_CONTROL_MASK;
    }
    if states & FCITX_ALT != 0 {
        modifiers |= K_ALT_MASK;
    }
    if states & FCITX_SUPER != 0 {
        modifiers |= K_SUPER_MASK;
    }
    if release {
        modifiers |= K_RELEASE_MASK;
    }
    modifiers
}

/// 创建引擎实例（`host` 可为空指针）。
///
/// # Safety
/// `host` 须为空或指向有效 `hux_host`（只做浅拷贝）。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn hux_engine_new(host: *const HostCallback) -> *mut Engine {
    let host = if host.is_null() {
        None
    } else {
        Some(unsafe { *host })
    };
    Box::into_raw(Box::new(Engine::new(host)))
}

/// 释放引擎实例（`engine` 可为空指针）。
///
/// # Safety
/// `engine` 须为 [`hux_engine_new`] 的返回值且尚未释放。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn hux_engine_free(engine: *mut Engine) {
    if engine.is_null() {
        return;
    }
    drop(unsafe { Box::from_raw(engine) });
}

/// 新建会话（一个输入上下文注册时调用）；返回会话 id（`0` = 失败）。
///
/// # Safety
/// 同 [`hux_engine_free`]。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn hux_engine_session_new(engine: *mut Engine) -> u64 {
    match unsafe { engine.as_mut() } {
        Some(engine) => engine.session_new(),
        None => 0,
    }
}

/// 释放会话（输入上下文销毁时调用；未知 id 忽略）。
///
/// # Safety
/// 同 [`hux_engine_free`]。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn hux_engine_session_free(engine: *mut Engine, session: u64) {
    if let Some(engine) = unsafe { engine.as_mut() } {
        engine.session_free(session);
    }
}

/// 重置会话（对应 fcitx5 `InputMethodEngine::deactivate/reset`）。
///
/// # Safety
/// 同 [`hux_engine_free`]。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn hux_engine_reset(engine: *mut Engine, session: u64) {
    if let Some(engine) = unsafe { engine.as_mut() } {
        engine.reset(session);
    }
}

/// 数据加载状态（诊断；NUL 结尾，随引擎存活）。
///
/// # Safety
/// `engine` 须有效（可为空指针）。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn hux_engine_status(engine: *const Engine) -> *const c_char {
    match unsafe { engine.as_ref() } {
        Some(engine) => engine.status.as_ptr(),
        None => std::ptr::null(),
    }
}

/// 应用外部配置（fcitx5 配置界面 → C++ 壳 → 本入口）。返回 1 = 已应用。
///
/// # Safety
/// `engine` 须有效；`options` 须为空或指向有效 `HuxOptions`。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn hux_engine_apply_settings(
    engine: *mut Engine,
    options: *const HuxOptions,
) -> i32 {
    let Some(engine) = (unsafe { engine.as_mut() }) else {
        return 0;
    };
    let Some(options) = (unsafe { options.as_ref() }) else {
        return 0;
    };
    // fcitx5 按键列表（keysym + 状态位）→ rime 键名列表；`sym=0` 项忽略。
    let key_reprs = |list: &HuxKeyList| -> Vec<String> {
        (0..HUX_MAX_KEYS)
            .take(list.count.clamp(0, HUX_MAX_KEYS as i32) as usize)
            .filter_map(|index| {
                let sym = list.sym[index];
                (sym != 0).then(|| {
                    KeyEvent::new(sym, core_modifiers(list.states[index] as u32, false)).repr()
                })
            })
            .collect()
    };
    engine.apply_settings(Settings {
        early_commit: options.early_commit != 0,
        early_commit_to_preedit: options.early_commit_to_preedit != 0,
        allow_duplicate_single: options.allow_duplicate_single != 0,
        full_shape: options.full_shape != 0,
        ascii_punct: options.ascii_punct != 0,
        learning_on_tab: options.learning_on_tab != 0,
        high_freq_limit: options.high_freq_limit.max(0) as usize,
        reverse_lookup_pronunciation_keys: key_reprs(&options.reverse_lookup_pronunciation),
        reverse_lookup_character_keys: key_reprs(&options.reverse_lookup_character),
        page_size: options.page_size.max(1) as usize,
        page_up_keys: key_reprs(&options.page_up),
        page_down_keys: key_reprs(&options.page_down),
        digit_select: options.digit_select != 0,
        candidate_layout: match options.candidate_layout {
            1 => CandidateLayout::Horizontal,
            2 => CandidateLayout::Vertical,
            _ => CandidateLayout::FollowGlobal,
        },
        preedit_mode: match options.preedit_mode {
            1 => PreeditMode::RawInput,
            2 => PreeditMode::Hidden,
            _ => PreeditMode::CandidateCode,
        },
        page_cycle: options.page_cycle != 0,
        min_retained_input_length: options
            .min_retained_input_length
            .clamp(0, hux_cfg::MAX_MIN_RETAINED_INPUT_LENGTH as i32)
            as usize,
    });
    1
}

/// 读取运行时开关（状态菜单）：返回 1/0；未知选项返回 -1。
///
/// # Safety
/// `engine` 须有效（可为空指针）；`name` 须为空或指向 NUL 结尾字符串。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn hux_engine_option_value(engine: *mut Engine, name: *const c_char) -> i32 {
    let Some(engine) = (unsafe { engine.as_ref() }) else {
        return -1;
    };
    if name.is_null() {
        return -1;
    }
    let name = unsafe { std::ffi::CStr::from_ptr(name) }.to_string_lossy();
    match engine.option_value(&name) {
        Some(true) => 1,
        Some(false) => 0,
        None => -1,
    }
}

/// 设置运行时开关（状态菜单）：返回 1 = 已应用；未知选项返回 0。
///
/// # Safety
/// `engine` 须有效（可为空指针）；`name` 须为空或指向 NUL 结尾字符串。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn hux_engine_set_option(
    engine: *mut Engine,
    name: *const c_char,
    value: i32,
) -> i32 {
    let Some(engine) = (unsafe { engine.as_mut() }) else {
        return 0;
    };
    if name.is_null() {
        return 0;
    }
    let name = unsafe { std::ffi::CStr::from_ptr(name) }.to_string_lossy();
    i32::from(engine.set_option_value(&name, value != 0))
}

/// 送入应用侧周边文本（字符制光标；`valid=0` 表示不可用/应用不支持）。返回 1 = 已受理。
///
/// # Safety
/// `engine` 须有效（可为空指针）；`text` 须为空或指向 NUL 结尾字符串。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn hux_engine_set_surrounding(
    engine: *mut Engine,
    session: u64,
    text: *const c_char,
    cursor_chars: i32,
    valid: i32,
) -> i32 {
    let Some(engine) = (unsafe { engine.as_mut() }) else {
        return 0;
    };
    if valid == 0 || text.is_null() {
        engine.set_surrounding(session, None, 0);
    } else {
        let text = unsafe { std::ffi::CStr::from_ptr(text) };
        engine.set_surrounding(
            session,
            Some(text.to_string_lossy().as_ref()),
            cursor_chars.max(0) as usize,
        );
    }
    1
}

/// 处理一次按键：返回位掩码 [`HUX_KEY_CONSUMED`] / [`HUX_KEY_FORWARD_AFTER_COMMIT`]。
///
/// # Safety
/// `engine` 须有效（可为空指针）。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn hux_engine_key(
    engine: *mut Engine,
    session: u64,
    keysym: u32,
    states: u32,
    release: i32,
) -> i32 {
    let Some(engine) = (unsafe { engine.as_mut() }) else {
        return 0;
    };
    let consumed = engine.key(session, keysym, states, release != 0);
    let mut disposition = 0;
    if engine.forward_after_commit {
        disposition |= HUX_KEY_FORWARD_AFTER_COMMIT;
    }
    if consumed {
        disposition |= HUX_KEY_CONSUMED;
    }
    disposition
}

/// 引擎选项**角色**对应的选项键（NUL 结尾；角色越界或引擎为空返回 NULL）。
///
/// 角色顺序与 `include/hux_abi.h` 的 `HUX_OPTION_*` 一致，即 `hux_cfg::roles::RUNTIME_OPTION_ROLES`：
/// 0=提前上屏、1=提前上屏至预编辑、2=单字重码组句、3=全角标点（rime 标准名）、4=数字直选。
/// 宿主据此构造状态菜单与面板序号，**不得**在宿主侧硬编码方案选项名。
///
/// 方案未声明的角色返回 NULL（宿主跳过该项；装配缺陷已在状态串报错）。
///
/// # Safety
/// `engine` 须有效（可为空指针）；返回指针在引擎存活期内有效。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn hux_engine_option_key(engine: *const Engine, role: i32) -> *const c_char {
    let Some(engine) = (unsafe { engine.as_ref() }) else {
        return std::ptr::null();
    };
    if role < 0 {
        return std::ptr::null();
    }
    engine
        .option_keys
        .get(role as usize)
        .and_then(|key| key.as_ref())
        .map_or(std::ptr::null(), |key| key.as_ptr())
}

/// 引擎选项角色总数（状态菜单项数）：与角色序同源，不各写一份。
#[unsafe(no_mangle)]
pub extern "C" fn hux_engine_option_role_count() -> i32 {
    hux_cfg::roles::RUNTIME_OPTION_ROLES.len() as i32
}

/// 候选点击（面板候选 `CandidateWord::select`）：按全局索引选中并上屏。
/// 返回 1 = 已处理；0 = 忽略（索引越界 / 无可选段 / 引擎不可用）。
///
/// # Safety
/// `engine` 须有效（可为空指针）。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn hux_engine_select_candidate(
    engine: *mut Engine,
    session: u64,
    index: i32,
) -> i32 {
    let Some(engine) = (unsafe { engine.as_mut() }) else {
        return 0;
    };
    if index < 0 {
        return 0;
    }
    i32::from(engine.select_candidate(session, index as usize))
}
