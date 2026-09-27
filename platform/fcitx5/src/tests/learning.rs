// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;

/// 学习库里的**坏帧**：跳过该条记录、不禁用整库，并进既有诊断。
///
/// 触发面：`<user dir>/tiger_sentence_learning_<hash>.userdb` 被损坏或被其它工具写坏
/// （值可能不是合法 UTF-8，`from_utf8_lossy` 会把 1 字节换成 3 字节 U+FFFD 使长度前缀错位），
/// 而 `LearningStore::open` 由 `hux_engine_new`（`extern "C"`）调用——
/// 旧实现用 `&value[a..b]` 按字节切片：落点不在字符边界即 panic，unwind 跨不过 C ABI ⇒ abort。
#[test]
fn learning_store_skips_undecodable_records_with_diagnostic() {
    let _guard = serial();
    let dir = temp_user_dir("bad-frame");
    let name = crate::learning_store::store_name("tiger_sentence");
    let path = dir.join(format!("{name}.userdb"));
    {
        let mut db = rusty_leveldb::DB::open(&path, rusty_leveldb::Options::default())
            .expect("open learning db");
        // 良构帧：时间 / mode / code / text / context。
        let good = hux_core::learning::frame(&[
            "1000.0".to_string(),
            "sentence-v2".to_string(),
            "ab".to_string(),
            "甲".to_string(),
            String::new(),
        ]);
        db.put(b"e/0000000001", good.as_bytes()).expect("put good");
        // 坏帧：长度前缀 1 落在 `é`（2 字节）中间——旧实现的 panic 点。
        db.put(b"e/0000000002", "1:é".as_bytes()).expect("put bad");
        db.flush().expect("flush");
    }
    let engine = TestEngine::new(host(), fixture_dirs(), None, Some(dir.clone()));
    let store = &engine.learning;
    assert!(store.store_ready(), "坏帧不得让整库不可用");
    assert_eq!(store.events.len(), 1, "坏帧跳过、良帧照常装载");
    assert_eq!(store.count, 2, "计数仍按库中记录数（上限判定不受影响）");
    let error = store.error.clone().unwrap_or_default();
    assert!(
        error.contains("skipped 1 undecodable record"),
        "坏帧必须计入既有诊断：{error}"
    );
    let status = engine
        .engine
        .diagnostics
        .status
        .to_str()
        .unwrap_or("")
        .to_string();
    assert!(
        status.contains("skipped 1 undecodable record"),
        "诊断随状态串对用户可见：{status}"
    );
}

/// 宿主自发提交接学习：Tab 选字后由宿主链提交（组合中大写字母），事件应落库。
///
/// 输入取 `abab`（两条 2 码边 ⇒ composed-only）：`c69c1a8` 起差异学习只对
/// composed-only 的基线与选中项成对，整串直出（Direct）的确认不再产生事件。
#[test]
fn host_commit_records_learning_on_tab() {
    let _guard = serial();
    let dir = temp_user_dir("host-learning");
    COMMITS.lock().unwrap().clear();
    UPDATES.lock().unwrap().clear();
    let mut engine = TestEngine::new(host(), fixture_dirs(), None, Some(dir.clone()));
    for code in *b"abab" {
        engine.key(u32::from(code), 0, false);
    }
    assert!(engine.key(0xff09, 0, false), "Tab 应被消费");
    let before = engine.learning.index_version();
    // 大写 A（0x41）：core 交宿主链 `char_handler`，先提交组合再交应用。
    engine.key(0x41, 0, false);
    // 第 2 个可见候选：`甲乙`/`乙甲` 同分时按文本字节序（`乙` < `甲`）取后者。
    assert_eq!(COMMITS.lock().unwrap().last().unwrap(), "乙甲");
    assert_ne!(
        engine.learning.index_version(),
        before,
        "宿主提交应写入学习库"
    );
    std::fs::remove_dir_all(&dir).ok();
}

/// 整串直出（Direct）的 Tab 确认不再是纠错证据（参照 `78bfaf3` 的探针期望：
/// 「Direct→Direct 学习计数不变」）。`ab` 只有一条整串边 ⇒ 两个候选都是 Direct。
#[test]
fn host_commit_direct_choice_records_no_learning() {
    let _guard = serial();
    let dir = temp_user_dir("host-learning-direct");
    COMMITS.lock().unwrap().clear();
    UPDATES.lock().unwrap().clear();
    let mut engine = TestEngine::new(host(), fixture_dirs(), None, Some(dir.clone()));
    for code in *b"ab" {
        engine.key(u32::from(code), 0, false);
    }
    assert!(engine.key(0xff09, 0, false), "Tab 应被消费");
    let before = engine.learning.index_version();
    engine.key(0x41, 0, false);
    assert_eq!(COMMITS.lock().unwrap().last().unwrap(), "乙");
    assert_eq!(
        engine.learning.index_version(),
        before,
        "Direct → Direct 不产生学习事件"
    );
    std::fs::remove_dir_all(&dir).ok();
}

/// **运行期**学习库写入失败须进状态串。
///
/// 此前 `learning.error` 只在构造期读一次：打开失败可见，`confirm` 里的 `db.put` 失败
/// （磁盘满 / 库被改成只读 / 锁异常）则完全静默，用户只看到「学习不生效」。
/// LevelDB 的写失败无法在测试里稳定构造，故直接注入错误值再走一次按键路径——
/// 守护的是 `finish → observe_learning_error → refresh_status` 这条接线：
/// 去掉那次调用，本用例即失败。
#[test]
fn learning_write_failure_reaches_the_status_string() {
    let _guard = serial();
    let dir = temp_user_dir("learning-error");
    let mut engine = TestEngine::new(host(), fixture_dirs(), None, Some(dir.clone()));
    engine.key(u32::from(b'a'), 0, false);
    let baseline = engine
        .engine
        .diagnostics
        .status
        .to_str()
        .unwrap_or("")
        .to_string();
    assert!(
        !baseline.contains("learning database write failed"),
        "初始状态串不含写入错误：{baseline}"
    );
    engine.engine.learning.error = Some("learning database write failed".to_string());
    engine.key(u32::from(b'b'), 0, false);
    let status = engine
        .engine
        .diagnostics
        .status
        .to_str()
        .unwrap_or("")
        .to_string();
    assert!(
        status.contains("learning: learning database write failed"),
        "运行期落库失败应进状态串：{status}"
    );
    std::fs::remove_dir_all(&dir).ok();
}
