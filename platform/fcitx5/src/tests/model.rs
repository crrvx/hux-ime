// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! 模型解析与重新部署：菜单短名 / 路径三态、按来源解析、重装模型与数据文件。
//!
//! 重新部署同时核对选项存储与学习库句柄；夹具与 `serial()` 串行约定见父模块 `tests.rs`。

use super::*;

/// 模型**菜单短名**（`hux_engine_model_info`）的三种状态 + 空指针：已装载（格式名）/
/// 无模型 / `<格式名>（装载失败）`；短名由方案侧结构化产出，平台只搬运不解析，
/// 文件名与失败原因走状态串的 `model:` 行（只落日志）。
#[test]
fn model_info_reports_file_format_and_state() {
    let _guard = serial();
    let goldens = hux_test_support::repo_path("goldens");
    let read = |engine: *const Engine| -> String {
        let info = unsafe { hux_engine_model_info(engine) };
        assert!(!info.is_null(), "引擎存活期内摘要指针不应为空");
        unsafe { std::ffi::CStr::from_ptr(info) }
            .to_string_lossy()
            .into_owned()
    };

    // 已装载：菜单显示**格式名**（按文件头 magic 检出，不是文件名；文件名在状态串里）。
    let engine = Box::into_raw(Box::new(Engine::new_with_dirs(
        host(),
        fixture_dirs(),
        Some(goldens.join("ngram_fixture.bin")),
        Some(temp_user_dir("model-info-loaded")),
    )));
    assert_eq!(read(engine), "三阶 TCSKNM02");
    unsafe { hux_engine_free(engine) };

    // 未找到：数据目录里没有模型资产。
    let empty_dir = hux_test_support::temp_dir("model-info-empty");
    let engine = Box::into_raw(Box::new(Engine::new_with_dirs(
        host(),
        vec![empty_dir.clone()],
        None,
        Some(temp_user_dir("model-info-none")),
    )));
    assert_eq!(read(engine), "无模型");
    unsafe { hux_engine_free(engine) };
    std::fs::remove_dir_all(&empty_dir).ok();

    // 装载失败：错误原文来自装载器（平台不拼、不解析）。
    let engine = Box::into_raw(Box::new(Engine::new_with_dirs(
        host(),
        fixture_dirs(),
        Some(goldens.join("lexicon/tiger_sentence.codes.txt")),
        Some(temp_user_dir("model-info-failed")),
    )));
    let failed = read(engine);
    assert!(failed.ends_with("（装载失败）"), "{failed}");
    // 失败原因（含期望格式）不进菜单，改走状态串的 `model:` 行。
    let status = unsafe { hux_engine_status(engine) };
    assert!(!status.is_null(), "引擎存活期内状态串指针不应为空");
    let status = unsafe { std::ffi::CStr::from_ptr(status) }
        .to_string_lossy()
        .into_owned();
    assert!(
        status.contains("model: tiger_sentence.codes.txt — 装载失败：")
            && status.contains("TCSKNM02"),
        "文件名与失败原因（含期望格式）应进状态串的 model: 行：{status}"
    );
    unsafe { hux_engine_free(engine) };

    // 空指针 ⇒ NULL（宿主据此早退）。
    assert!(unsafe { hux_engine_model_info(std::ptr::null()) }.is_null());
}

/// 模型文件路径（`hux_engine_model_path`，宿主首项「打开模型目录」入口）：
/// 已装载 / 装载失败 ⇒ 该文件本身；未找到 ⇒ 默认查找路径（**文件可以不存在**，其父目录即
/// 「模型该放的地方」）；重新部署后随新解析结果刷新；空引擎 ⇒ NULL。
#[test]
fn model_path_points_at_the_file_or_the_place_to_put_it() {
    let _guard = serial();
    let goldens = hux_test_support::repo_path("goldens");
    let read = |engine: *const Engine| -> Option<String> {
        let path = unsafe { hux_engine_model_path(engine) };
        if path.is_null() {
            return None;
        }
        Some(
            unsafe { std::ffi::CStr::from_ptr(path) }
                .to_string_lossy()
                .into_owned(),
        )
    };

    // 已装载：路径就是装载的那个文件。
    let loaded = goldens.join("ngram_fixture.bin");
    let engine = Box::into_raw(Box::new(Engine::new_with_dirs(
        host(),
        fixture_dirs(),
        Some(loaded.clone()),
        Some(temp_user_dir("model-path-loaded")),
    )));
    assert_eq!(read(engine), Some(loaded.display().to_string()));
    unsafe { hux_engine_free(engine) };

    // 未找到：给「该放的位置」（首个数据目录下的方案资产路径），文件**不存在**也算数——
    // 菜单据此把用户送到正确目录（其父目录）。
    let first = hux_test_support::temp_dir("model-path-first");
    let second = hux_test_support::temp_dir("model-path-second");
    let intended = first.join("models/sentence-ngram-mobile.bin");
    assert!(!intended.exists(), "夹具前提：该位置还没有模型文件");
    let engine = Box::into_raw(Box::new(Engine::new_with_dirs(
        host(),
        vec![first.clone(), second.clone()],
        None,
        Some(temp_user_dir("model-path-none")),
    )));
    assert_eq!(read(engine), Some(intended.display().to_string()));

    // 重新部署：模型装进**第二个**目录后按新解析结果刷新（不再是「该放的位置」）。
    let found = second.join("models/sentence-ngram-mobile.bin");
    std::fs::create_dir_all(found.parent().expect("parent")).expect("mkdir");
    std::fs::copy(goldens.join("ngram_fixture.bin"), &found).expect("copy");
    assert_eq!(unsafe { hux_engine_redeploy(engine) }, 1);
    assert_eq!(read(engine), Some(found.display().to_string()));
    unsafe { hux_engine_free(engine) };
    std::fs::remove_dir_all(&first).ok();
    std::fs::remove_dir_all(&second).ok();

    // 空引擎 ⇒ NULL（宿主据此直接返回，不去开目录）。
    assert_eq!(read(std::ptr::null()), None);
}

/// 模型路径来源：默认查找（`Auto`）按数据目录解析，「重新部署」据此拿到新装入的模型；
/// 显式路径（`Fixed`）不受目录内容影响。
#[test]
fn model_source_resolves_by_source() {
    let dir = hux_test_support::temp_dir("model-source-auto");
    let auto = crate::engine::ModelSource::Auto;
    assert_eq!(
        auto.resolve(std::slice::from_ref(&dir)),
        None,
        "空目录里没有模型资产"
    );
    let model = dir.join("models/sentence-ngram-mobile.bin");
    std::fs::create_dir_all(model.parent().expect("parent")).expect("mkdir");
    std::fs::write(&model, b"TCSKNM02").expect("write");
    assert_eq!(
        auto.resolve(std::slice::from_ref(&dir)),
        Some(model.clone())
    );
    let fixed = crate::engine::ModelSource::Fixed(dir.join("fixed.bin"));
    assert_eq!(
        fixed.resolve(std::slice::from_ref(&dir)),
        Some(dir.join("fixed.bin"))
    );
    std::fs::remove_dir_all(&dir).ok();
}

/// 重新部署（`hux_engine_redeploy`）：返回 1、既有会话 id 继续可用但状态被重置、
/// 模型摘要随重新装载刷新；引擎为空指针返回 0。
#[test]
fn redeploy_refreshes_model_info_and_resets_sessions() {
    let _guard = serial();
    let goldens = hux_test_support::repo_path("goldens");
    let dir = hux_test_support::temp_dir("redeploy-model");
    std::fs::create_dir_all(&dir).expect("mkdir");
    // 指向一个尚不存在的模型：先「装载失败」，装入文件后再重新部署应变成「已加载」。
    let model = dir.join("sentence-ngram-mobile.bin");
    let engine = Box::into_raw(Box::new(Engine::new_with_dirs(
        host(),
        fixture_dirs(),
        Some(model.clone()),
        Some(temp_user_dir("redeploy-user")),
    )));
    let read = || {
        let info = unsafe { hux_engine_model_info(engine) };
        assert!(!info.is_null());
        unsafe { std::ffi::CStr::from_ptr(info) }
            .to_string_lossy()
            .into_owned()
    };
    let failed = read();
    assert!(failed.ends_with("（装载失败）"), "{failed}");

    // 建一个会话并留下组合状态：重新部署后 id 必须仍然有效、组合必须被清空。
    let session = unsafe { hux_engine_session_new(engine) };
    assert!(session > 0);
    assert_ne!(
        unsafe { hux_engine_key(engine, session, u32::from(b'a'), 0, 0) } & HUX_KEY_CONSUMED,
        0,
        "夹具码表里 a 应被消费（组合已开始）"
    );
    assert_eq!(unsafe { &*engine }.sessions[&session].context.input(), b"a");

    // 「装好数据再重新部署」：模型文件就位 → 菜单短名刷新成**格式名**（文件名只进状态串）。
    std::fs::copy(goldens.join("ngram_fixture.bin"), &model).expect("copy");
    assert_eq!(unsafe { hux_engine_redeploy(engine) }, 1);
    assert_eq!(read(), "三阶 TCSKNM02");

    // 会话 id 仍可用（重置而非释放）；未知 id 仍被忽略。
    let state = unsafe { &*engine };
    assert!(state.sessions.contains_key(&session));
    assert_eq!(
        state.sessions[&session].context.input(),
        b"",
        "重新部署应清空组合"
    );
    assert_ne!(
        unsafe { hux_engine_key(engine, session, u32::from(b'a'), 0, 0) } & HUX_KEY_CONSUMED,
        0
    );
    assert_eq!(
        unsafe { hux_engine_key(engine, session + 100, u32::from(b'a'), 0, 0) },
        0
    );
    unsafe { hux_engine_session_free(engine, session) };
    unsafe { hux_engine_free(engine) };
    std::fs::remove_dir_all(&dir).ok();

    // 空指针：返回 0（宿主据此报错，而不是假装成功）。
    assert_eq!(unsafe { hux_engine_redeploy(std::ptr::null_mut()) }, 0);
}

/// 重新部署 = **重走一遍构造期的读取**：手改 `options.yaml` 后（进程仍在跑）重新部署即生效。
/// 会话 id 不变、旧句柄继续可用（宿主的输入上下文不需要重建）。
#[test]
fn redeploy_rereads_the_option_store() {
    let _guard = serial();
    let dir = temp_user_dir("redeploy-options");
    std::fs::write(dir.join(OPTIONS_FILE), "options:\n  full_shape: false\n").expect("write");
    let mut engine = TestEngine::new(host(), fixture_dirs(), None, Some(dir.clone()));
    assert!(!engine.session().context.get_option("full_shape"));
    // 手改存储：重新部署前引擎看不到（只有重启才会重读）。
    std::fs::write(dir.join(OPTIONS_FILE), "options:\n  full_shape: true\n").expect("write");
    assert!(
        !engine.session().context.get_option("full_shape"),
        "重读前不变"
    );
    assert!(engine.redeploy());
    let session = engine.session;
    assert!(
        engine.engine.sessions.contains_key(&session),
        "重新部署后会话 id 应继续有效"
    );
    assert!(
        engine.session().context.get_option("full_shape"),
        "重读后的存储值应生效"
    );
    assert_eq!(engine.option_value("full_shape"), Some(true));
    // 旧句柄仍可用：还能开始组合。
    assert!(engine.key(u32::from(b'a'), 0, false));
    assert_eq!(engine.session().context.input(), b"a");
    std::fs::remove_dir_all(&dir).ok();
}

/// 重新部署重开**学习库**：库目录被外部改动（此处整库删除）后，重新部署读到的是新状态，
/// 而不是构造期那个仍被持有的旧句柄。
#[test]
fn redeploy_reopens_the_learning_store() {
    let _guard = serial();
    let dir = temp_user_dir("redeploy-learning");
    let name = learning_store::store_name(hux_scheme_tiger::scheme::SCHEME_ID);
    // 先播一条事件（构造前，故引擎读到的是这条）。
    {
        let mut seed = crate::learning_store::LearningStore::open(&dir, &name, 0.0);
        assert!(seed.confirm(&[hux_core::learning::Event {
            time: 1.0,
            mode: "m".to_string(),
            code: "ab".to_string(),
            text: "甲".to_string(),
            context: String::new(),
        }]));
    }
    let mut engine = TestEngine::new(host(), fixture_dirs(), None, Some(dir.clone()));
    assert_eq!(engine.engine.learning.count, 1, "构造期读到外部事件");
    // 外部改动：整库删除（引擎仍持有旧句柄）。
    std::fs::remove_dir_all(dir.join(format!("{name}.userdb"))).expect("remove library");
    assert!(engine.redeploy());
    assert!(engine.engine.learning.store_ready(), "重新部署应重开学习库");
    assert_eq!(
        engine.engine.learning.count, 0,
        "重新部署应重读学习库（旧句柄看不到删除后的状态）"
    );
    assert_eq!(engine.engine.learning.name, name);
    std::fs::remove_dir_all(&dir).ok();
}

/// 重新部署重读**数据文件**：同一目录里替换码表（新增/覆盖数据文件）后新码表立即生效。
///
/// 判据用宿主可见的候选列表（码表内容 → 候选文本），并复用同一会话 id 复核旧句柄可用。
#[test]
fn redeploy_rereads_data_files() {
    let _guard = serial();
    UPDATES.lock().unwrap().clear();
    let data = hux_test_support::temp_dir("redeploy-data");
    let user = temp_user_dir("redeploy-data-user");
    let codes = data.join("tiger_sentence.codes.txt");
    std::fs::copy(
        hux_test_support::repo_path("goldens/key_sequence/tiger_sentence.codes.txt"),
        &codes,
    )
    .expect("copy fixture");
    let mut engine = TestEngine::new(host(), vec![data.clone()], None, Some(user.clone()));
    let type_code = |engine: &mut TestEngine, code: &[u8]| {
        for key in code {
            engine.key(u32::from(*key), 0, false);
        }
        let candidates = last_update().2;
        engine.reset();
        candidates
    };
    assert_eq!(
        type_code(&mut engine, b"ab"),
        vec!["甲".to_string(), "乙".to_string()]
    );
    // 替换码表内容（同一路径）：重新部署后应读到新码表。
    std::fs::write(&codes, "丙\tab\n丁\tab\n").expect("write");
    assert_eq!(
        type_code(&mut engine, b"ab"),
        vec!["甲".to_string(), "乙".to_string()],
        "重新部署前仍用旧码表"
    );
    assert!(engine.redeploy());
    let session = engine.session;
    assert!(engine.engine.sessions.contains_key(&session));
    assert_eq!(
        type_code(&mut engine, b"ab"),
        vec!["丙".to_string(), "丁".to_string()],
        "重新部署应重读数据文件"
    );
    // 旧句柄可用且组合已重置。
    assert!(engine.key(u32::from(b'a'), 0, false));
    assert_eq!(engine.session().context.input(), b"a");
    std::fs::remove_dir_all(&data).ok();
    std::fs::remove_dir_all(&user).ok();
}
