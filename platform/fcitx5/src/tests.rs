// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

use crate::abi::*;
use crate::engine::Engine;
use crate::learning_store;
use crate::session::Session;
use hux_cfg::{
    CandidateLayout, MAX_MIN_RETAINED_INPUT_LENGTH, OPTIONS_FILE, PreeditMode, Settings,
};
use hux_core::key::KeyEvent;
use hux_core::scheme::OptionDecl;
use std::ffi::{CString, c_char, c_void};
use std::path::PathBuf;
use std::sync::Mutex;

static COMMITS: Mutex<Vec<String>> = Mutex::new(Vec::new());
type UpdateSnapshot = (String, i32, Vec<String>, i32, String, String);
static UPDATES: Mutex<Vec<UpdateSnapshot>> = Mutex::new(Vec::new());
/// 回调记录为进程级静态：使用它们的测试串行执行，避免并发串扰。
static TEST_SEQUENCE: Mutex<()> = Mutex::new(());

fn serial() -> std::sync::MutexGuard<'static, ()> {
    TEST_SEQUENCE
        .lock()
        .unwrap_or_else(|error| error.into_inner())
}

unsafe extern "C" fn record_commit(_user: *mut c_void, text: *const c_char) {
    let text = unsafe { std::ffi::CStr::from_ptr(text) };
    COMMITS
        .lock()
        .unwrap()
        .push(text.to_string_lossy().into_owned());
}

unsafe extern "C" fn record_update(
    _user: *mut c_void,
    preedit: *const c_char,
    cursor: i32,
    texts: *const *const c_char,
    _comments: *const *const c_char,
    count: i32,
    selected: i32,
    aux_up: *const c_char,
    aux_down: *const c_char,
) {
    let preedit = unsafe { std::ffi::CStr::from_ptr(preedit) }
        .to_string_lossy()
        .into_owned();
    let mut candidates = Vec::new();
    for index in 0..count {
        let text = unsafe { *texts.add(index as usize) };
        candidates.push(
            unsafe { std::ffi::CStr::from_ptr(text) }
                .to_string_lossy()
                .into_owned(),
        );
    }
    let read = |value: *const c_char| {
        if value.is_null() {
            String::new()
        } else {
            unsafe { std::ffi::CStr::from_ptr(value) }
                .to_string_lossy()
                .into_owned()
        }
    };
    let aux_up = read(aux_up);
    let aux_down = read(aux_down);
    UPDATES
        .lock()
        .unwrap()
        .push((preedit, cursor, candidates, selected, aux_up, aux_down));
}

fn host() -> Option<HostCallback> {
    Some(HostCallback {
        user: std::ptr::null_mut(),
        commit: Some(record_commit),
        update: Some(record_update),
    })
}

fn fixture_dirs() -> Vec<PathBuf> {
    vec![
        hux_test_support::repo_path("goldens/key_sequence"),
        hux_test_support::repo_path("data"),
    ]
}

fn temp_user_dir(tag: &str) -> PathBuf {
    hux_test_support::temp_dir(&format!("user-{tag}"))
}

/// 最近一次 UI 快照（preedit、字节光标、候选、高亮）。
fn last_update() -> UpdateSnapshot {
    UPDATES.lock().unwrap().last().cloned().expect("update")
}

/// 测试用引擎包装：预建一个会话，按键/重置/点击/周边文本自动带上会话 id。
struct TestEngine {
    engine: Engine,
    session: u64,
}

impl TestEngine {
    fn new(
        host: Option<HostCallback>,
        dirs: Vec<PathBuf>,
        model_path: Option<PathBuf>,
        options_dir: Option<PathBuf>,
    ) -> Self {
        let mut engine = Engine::new_with_dirs(host, dirs, model_path, options_dir);
        let session = engine.session_new();
        Self { engine, session }
    }

    fn key(&mut self, keysym: u32, states: u32, release: bool) -> bool {
        self.engine.key(self.session, keysym, states, release)
    }

    fn reset(&mut self) {
        self.engine.reset(self.session);
    }

    fn select_candidate(&mut self, index: usize) -> bool {
        self.engine.select_candidate(self.session, index)
    }

    fn set_surrounding(&mut self, text: Option<&str>, cursor_chars: usize) {
        self.engine
            .set_surrounding(self.session, text, cursor_chars);
    }

    fn session(&self) -> &Session {
        self.engine.sessions.get(&self.session).expect("session")
    }
}

impl std::ops::Deref for TestEngine {
    type Target = Engine;
    fn deref(&self) -> &Engine {
        &self.engine
    }
}

impl std::ops::DerefMut for TestEngine {
    fn deref_mut(&mut self) -> &mut Engine {
        &mut self.engine
    }
}

fn key_list(keys: &[(i32, i32)]) -> HuxKeyList {
    let mut list = HuxKeyList::default();
    for (index, (sym, states)) in keys.iter().enumerate().take(HUX_MAX_KEYS) {
        list.sym[index] = *sym;
        list.states[index] = *states;
    }
    list.count = keys.len().min(HUX_MAX_KEYS) as i32;
    list
}

fn ffi_options() -> HuxOptions {
    HuxOptions {
        early_commit: 0,
        early_commit_to_preedit: 1,
        allow_duplicate_single: 1,
        full_shape: 1,
        ascii_punct: 1,
        learning_on_tab: 0,
        high_freq_limit: 800,
        // 音反查：`；`（无修饰）与 Shift+`；`（= `:`）。
        reverse_lookup_pronunciation: key_list(&[(0x3b, 0), (0x3a, 0)]),
        // 字反查：Shift+`（= `~`）。
        reverse_lookup_character: key_list(&[(0x60, 1)]),
        page_size: 7,
        // 翻页：`.` 与 `]`。
        page_up: key_list(&[(0x2c, 0)]),
        page_down: key_list(&[(0x2e, 0), (0x5d, 0)]),
        digit_select: 1,
        candidate_layout: 0,
        preedit_mode: 0,
        page_cycle: 0,
        min_retained_input_length: 0,
        full_charset: 1,
        filter_non_han: 1,
    }
}

/// FFI 用例的引擎指针：**显式临时用户目录**。
///
/// `hux_engine_new(std::ptr::null())` 会解析真实环境（`XDG_DATA_HOME`/`HOME`）并在
/// `~/.local/share/fcitx5/hux/` 打开（必要时创建）学习库：本机 fcitx5 正在运行时会命中
/// LevelDB 锁，且会污染/创建用户真实数据。此处用同一 `Engine`（`new_with_dirs`）显式注入
/// 临时用户目录，其余 FFI 入口（apply_settings / status / session_new / key / free）照旧覆盖。
fn ffi_engine(user_dir: PathBuf) -> *mut Engine {
    Box::into_raw(Box::new(Engine::new_with_dirs(
        host(),
        fixture_dirs(),
        None,
        Some(user_dir),
    )))
}

/// 字反查夹具目录。
fn reverse_lookup_character_dirs() -> Vec<PathBuf> {
    vec![
        hux_test_support::repo_path("goldens/sound_to_char_shape"),
        hux_test_support::repo_path("data"),
    ]
}

/// `hux_abi.h` 的 `HUX_OPTION_*` 枚举序 ↔ `hux_cfg::roles::RUNTIME_OPTION_ROLES`（顺序 / 个数 / 名字）。
///
/// 角色序在三处手工同步（角色表、头文件枚举、C++ 文案表 `kLabels[role]`）：
/// **调序**会让菜单文案与开关静默错位、`HUX_OPTION_DIGIT_SELECT` 取到别的选项键。
/// C++ 侧只能守长度（`static_assert(std::size(kLabels) == HUX_OPTION_COUNT)`，见 `shell/hux.cpp`），
/// 顺序由本用例从**头文件源码**解析后逐项比对——改名 / 加角色 / 调序都在此失败。
/// 从 `hux_abi.h` 源码解析某个具名枚举（`NAME = n, …`，含末尾计数哨兵）。
///
/// 头文件里有多个 `enum { … };`（取值枚举、角色枚举），故按**成员前缀**挑出目标枚举。
fn abi_enum_members(prefix: &str) -> Vec<(String, i32)> {
    let header = std::fs::read_to_string(hux_test_support::repo_path(
        "crates/hux-ffi/include/hux_abi.h",
    ))
    .expect("read hux_abi.h");
    for body in header.split("enum {").skip(1) {
        let body = body.split_once("};").expect("枚举结束").0;
        let members: Vec<(String, i32)> = body
            .lines()
            .filter_map(|line| {
                let (name, value) = line.trim().split_once('=')?;
                let name = name.trim();
                if !name.starts_with(&format!("{prefix}_")) {
                    return None;
                }
                Some((
                    name.to_string(),
                    value
                        .trim()
                        .trim_end_matches(',')
                        .parse::<i32>()
                        .expect("枚举下标应为整数"),
                ))
            })
            .collect();
        if !members.is_empty() {
            return members;
        }
    }
    panic!("hux_abi.h 里没有 {prefix}_* 枚举");
}

mod abi_entries;
mod digit_select;
mod ffi_mapping;
mod key_routing;
mod learning;
mod lifecycle;
mod model;
mod options;
mod preedit;
mod reverse_lookup;
mod scheme_config;
mod status;
