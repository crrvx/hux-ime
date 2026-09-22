// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! 五阶模型读取的单元测试。
//!
//! 夹具由上游 builder（`tools/build_tcs_knm03.cpp`）从固定 ARPA 生成；期望值由参照实现
//! （Lua 5.4 跑 `lua/tiger_sentence_fivegram.lua`）对同一夹具逐位实测，因此全部用
//! `assert_eq!` 比较完整精度，并另比 `f64` 位模式（防止十进制字面量抄写走样）。
//!
//! - `fivegram_fixture.bin`：上游 `tools/test_tcs_knm03.py` 的 ARPA，41,603 B；
//! - `fivegram_fixture_paged.bin`：自造 ARPA，43,798 B，用于覆盖上游小夹具到不了的路径。
//!   构造规则（可复现）：词表 `</s> <s> <unk> P Q T V W Y Z s00..s09 t00..t09 x00 x005 x01..x64`；
//!   `\2-grams` 有 `P x00`（**无回退列** ⇒ 3-gram 上下文 `(P,x00)` 的 `bow_q == 0`）、
//!   `P x01 -0.25`、`W V -0.35`（无 3-gram 后继 ⇒ 空 block）、`V T -0.2`、`Y T -0.3`、
//!   `T </s> 0`、`x00 Y -0.2`、`s<k> t<k> -0.5`；`\3-grams` 有 `<s> P x00 -0.05` 与
//!   `P x<i> Y`（i = 0..64，共 65 个 block）；`\4-grams` 有 `<s> P x00 Y -0.04`；
//!   `\5-grams` 有 `<s> P x00 Y T`。

// 断言里的十进制字面量逐字取自参照实现的 `%.17g` 实测输出（便于与 Lua 侧文本对照），
// 每条另用 `to_bits()` 断言位模式，故不做「最短往返表示」改写。
#![allow(clippy::excessive_precision)]

use super::*;
use std::path::{Path, PathBuf};

/// 一条硬期望：`(lm1, lm2, lm3, lm4, count, target, 期望分数, 期望位模式)`。
type Expectation = (u16, u16, u16, u16, u8, &'static str, f64, u64);

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../goldens")
        .join(name)
}

/// 上游小夹具（每桶 1 个索引点）。
fn reference() -> FivegramModel {
    FivegramModel::load(fixture("fivegram_fixture.bin"), None).expect("装载五阶模型夹具")
}

/// 自造分页夹具（order-3 桶 3 有 65 个 block ⇒ 2 个索引点 + 页边界）。
fn paged() -> FivegramModel {
    FivegramModel::load(fixture("fivegram_fixture_paged.bin"), None).expect("装载分页五阶夹具")
}

/// 规格列出的硬期望：9 条调用的分数必须逐位吻合。
#[test]
fn reference_step_scores_are_bit_exact() {
    let mut model = reference();
    assert_eq!(model.bytes(), 41_603);
    assert_eq!(
        (model.bos_id(), model.eos_id(), model.unknown_id()),
        (1, 0, 2)
    );
    // 规格里 `step(1,0,0,2,"好")` 一类简写省略了 `count`，此处按 `count = lm4` 补全。
    let cases: [Expectation; 9] = [
        (
            1,
            0,
            0,
            0,
            1,
            "你",
            -0.46051703942569144,
            0xbfdd791c75e561b0,
        ),
        (1, 0, 0, 2, 2, "好", -1.381551062738722, 0xbff61ad54983783d),
        (1, 0, 0, 3, 3, "吗", -2.3025851550233853, 0xc0026bb1c408a4ed),
        (1, 0, 0, 4, 4, EOS, -1.6118096224486218, 0xbff9c9f8e2fcb811),
        (1, 0, 0, 0, 1, EOS, -1.6118096224486218, 0xbff9c9f8e2fcb811),
        (0, 0, 0, 0, 0, "你", -1.6118095604192817, 0xbff9c9f8d2561860),
        (0, 0, 0, 0, 0, EOS, -1.3815510495610273, 0xbff61ad545f9e7c8),
        (1, 0, 0, 0, 1, "不", -2.5328436658816407, 0xc00443438a36bd3b),
        (
            3,
            999,
            999,
            999,
            1,
            "你",
            -2.0723301349831429,
            0xc0009421d2622496,
        ),
    ];
    for (lm1, lm2, lm3, lm4, count, target, expected, bits) in cases {
        let outcome = model.step(lm1, lm2, lm3, lm4, count, target).expect("打分");
        assert_eq!(
            outcome.score, expected,
            "step({lm1},{lm2},{lm3},{lm4},{count},{target:?}) 的完整精度"
        );
        assert_eq!(
            outcome.score.to_bits(),
            bits,
            "step({lm1},{lm2},{lm3},{lm4},{count},{target:?}) 的 f64 位模式"
        );
    }
}

/// `step` 的六元返回：`id` 是目标 ID，`lm1..lm3` 是**旧的**槽位值（调用方写入 `lm2..lm4`），
/// `count` 增长到 4 为止。
#[test]
fn step_returns_shifted_slots_and_caps_count() {
    let mut model = reference();
    let outcome = model.step(1, 0, 0, 0, 1, "你").expect("打分");
    assert_eq!(
        (
            outcome.id,
            outcome.lm1,
            outcome.lm2,
            outcome.lm3,
            outcome.count
        ),
        (3, 1, 0, 0, 2)
    );
    // `count` 超过 4 时 history 仍只有 4 个槽位，回传的 `count` 恒为 4。
    let four = model.step(3, 5, 4, 1, 4, "好").expect("打分");
    let five = model.step(3, 5, 4, 1, 5, "好").expect("打分");
    let nine = model.step(3, 5, 4, 1, 9, "好").expect("打分");
    assert_eq!(four.score, -0.69078608581269607);
    assert_eq!(five.score, four.score);
    assert_eq!(nine.score, four.score);
    assert_eq!((five.count, nine.count), (4, 4));
}

/// `count` 较小时 `lm2..lm4` 的脏值被忽略；`count == 0` 合法且仍取 `lm1` 作 history。
#[test]
fn dirty_slots_are_ignored_and_count_zero_uses_lm1() {
    let mut model = reference();
    let dirty = model.step(3, 999, 999, 999, 1, "你").expect("打分");
    let clean = model.step(3, 0, 0, 0, 1, "你").expect("打分");
    assert_eq!(dirty.score, clean.score);
    assert_eq!(dirty.score, -2.0723301349831429);
    // 脏值只被原样回显（槽位平移），不参与打分。
    assert_eq!((dirty.lm1, dirty.lm2, dirty.lm3), (3, 999, 999));
    let clean_dirty = model.step(3, 5, 4, 0, 2, "好").expect("打分");
    assert_eq!(clean_dirty.score, -0.69078608581269607);
    // `count == 0` 与 `count == 1` 同分（`count <= 1` 分支取 `lm1`）。
    let zero = model.step(1, 0, 0, 0, 0, "你").expect("打分");
    let one = model.step(1, 0, 0, 0, 1, "你").expect("打分");
    assert_eq!(zero.score, one.score);
    assert_eq!(zero.score, -0.46051703942569144);
    assert_eq!((zero.lm1, zero.count), (1, 1));
    // `lm1` 为 `</s>`（ID 0）时 history 非空：order-2 桶 0 为空桶 ⇒ 直接落 unigram。
    let eos_history = model.step(0, 0, 0, 0, 0, "你").expect("打分");
    assert_eq!(eos_history.score, -1.6118095604192817);
    assert_eq!(eos_history.score.to_bits(), 0xbff9c9f8d2561860);
}

/// 词表外 token 走 `unknown_id` 兜底；`token_id` 对词表外返回 `None`（两条口径不同）。
#[test]
fn unknown_targets_fall_back_to_unknown_id() {
    let mut model = reference();
    for target in ["不", "不存在", "", "\u{4}"] {
        let outcome = model.step(1, 0, 0, 0, 1, target).expect("打分");
        assert_eq!(outcome.id, 2, "target {target:?}");
        assert_eq!(outcome.score, -2.5328436658816407, "target {target:?}");
        assert_eq!(
            outcome.score.to_bits(),
            0xc00443438a36bd3b,
            "target {target:?}"
        );
    }
    assert_eq!(model.token_id("不"), None);
    assert_eq!(model.token_id(""), None);
    assert_eq!(model.token_id(BOS), Some(1));
    assert_eq!(model.token_id(EOS), Some(0));
    assert_eq!(model.token_id("你"), Some(3));
    // unigram(`</s>`) = -0.6 ⇒ `count == 0`（history 为 `[</s>]`）时分数为 -0.6·ln10。
    let known = model.step(0, 0, 0, 0, 0, EOS).expect("打分");
    assert_eq!(known.score, -1.3815510495610273);
    assert_eq!(known.score.to_bits(), 0xbff61ad545f9e7c8);
    // unigram(`<unk>`) 量化无损（恰为 -1.0）⇒ 词表外 target 且 `count == 0` 时分数正好是 -LN10。
    let unknown = model.step(0, 0, 0, 0, 0, "不存在").expect("打分");
    assert_eq!(unknown.score, -LN10);
    assert_eq!(unknown.score.to_bits(), 0xc0026bb1bbb55516);
}

/// `has_observed_bigram` 只判 order 2：词表外任一侧为 `false`（不走 `unknown`）。
#[test]
fn has_observed_bigram_positive_and_negative() {
    let mut model = reference();
    // `<s>`/`</s>` 既是词表里的字面 token，也与 `BOS`/`EOS` 控制字符指向同一 ID。
    let cases = [
        ("你", "好", true),
        ("吗", "你", false),
        (BOS, "你", true),
        ("你", EOS, false),
        ("好", "吗", true),
        ("<s>", "好", true),
        ("</s>", "你", false),
        ("不", "你", false),
        ("", "好", false),
        (EOS, "你", false),
    ];
    for (previous, target, expected) in cases {
        let observed = model
            .has_observed_bigram(previous, target)
            .expect("观测查询");
        assert_eq!(observed, expected, "({previous:?},{target:?})");
    }
    assert!(
        model
            .has_observed_bigram_chars('你', '好')
            .expect("观测查询")
    );
    assert!(
        !model
            .has_observed_bigram_chars('\u{3}', '你')
            .expect("观测查询")
    );
    assert!(
        !model
            .has_observed_bigram_chars('不', '你')
            .expect("观测查询")
    );
}

/// `logp` 旧口径：`prev2` 为 BOS 时不入 history，且与 TCSKNM02 的插值口径无关。
#[test]
fn logp_keeps_bos_out_of_history() {
    let mut model = reference();
    let cases = [
        (BOS, "你", "好", -0.25328434149781531, 0xbfd035cf86d49bda),
        ("你", "好", "吗", -0.18420687352371584, 0xbfc7941740bdcb56),
        (BOS, BOS, "好", -1.381551062738722, 0xbff61ad54983783d),
        ("你", "你", "你", -2.0723301349831429, 0xc0009421d2622496),
        (BOS, "你", EOS, -2.0723301255172162, 0xc0009421d11ce56a),
    ];
    for (prev2, prev1, target, expected, bits) in cases {
        let score = model.logp(prev2, prev1, target).expect("logp");
        assert_eq!(score, expected, "logp({prev2:?},{prev1:?},{target:?})");
        assert_eq!(
            score.to_bits(),
            bits,
            "logp({prev2:?},{prev1:?},{target:?}) 位模式"
        );
    }
}

/// `cache_status` 只有 4 个字段，且随页换入变化；链式查询的分数与槽位同步校验。
#[test]
fn cache_status_tracks_pages_and_has_four_fields() {
    let mut model = reference();
    let after_load = model.cache_status();
    assert_eq!(
        after_load,
        CacheStatus {
            page_bytes: 0,
            page_limit: 8 * 1024 * 1024,
            page_entries: 0,
            index_cache_limit: 64,
        }
    );
    assert_eq!(
        after_load.canonical(),
        "page_bytes=0\tpage_limit=8388608\tpage_entries=0\tindex_cache_limit=64"
    );
    let mut history = LmHistory::begin(model.bos_id());
    let steps = [
        ("你", -0.46051703942569144),
        ("好", -0.25328434149781531),
        ("吗", -0.18420687352371584),
        (EOS, -0.069077552789821375),
    ];
    for (target, expected) in steps {
        let score = model.step_token(&mut history, target).expect("打分");
        assert_eq!(score, expected, "target {target:?}");
    }
    assert_eq!(
        (
            history.lm1,
            history.lm2,
            history.lm3,
            history.lm4,
            history.count
        ),
        (0, 4, 5, 3, 4)
    );
    // 四次查询共换入 4 张 page（桶 `<s>`/`你`/`好`/`吗`），合计 82 B。
    assert_eq!(
        model.cache_status(),
        CacheStatus {
            page_bytes: 82,
            page_limit: 8 * 1024 * 1024,
            page_entries: 4,
            index_cache_limit: 64,
        }
    );
    // 单字符入口与 token 串入口同分（单字符快路径与字节串查表等价）。
    let mut chars = LmHistory::begin(model.bos_id());
    let mut tokens = LmHistory::begin(model.bos_id());
    let by_char = model.step_char(&mut chars, '你').expect("打分");
    let by_token = model.step_token(&mut tokens, "你").expect("打分");
    assert_eq!(by_char, by_token);
    assert_eq!((chars.lm1, chars.count), (tokens.lm1, tokens.count));
}

/// `configure_cache` / `trim_caches` / `close` 都清空 page cache 并保留/更新上限。
#[test]
fn configure_cache_and_trim_reset_page_cache() {
    let mut model = reference();
    let mut history = LmHistory::begin(model.bos_id());
    for target in ["你", "好"] {
        model.step_token(&mut history, target).expect("打分");
    }
    assert!(model.cache_status().page_bytes > 0);
    model.trim_caches();
    assert_eq!(
        model.cache_status(),
        CacheStatus {
            page_bytes: 0,
            page_limit: 8 * 1024 * 1024,
            page_entries: 0,
            index_cache_limit: 64,
        }
    );
    model.configure_cache(Limits {
        page_bytes: 32,
        index_pages: 1,
    });
    assert_eq!(
        model.cache_status(),
        CacheStatus {
            page_bytes: 0,
            page_limit: 32,
            page_entries: 0,
            index_cache_limit: 1,
        }
    );
    // 上限 0 与参照实现同口径：`page_bytes` 原样接受，index cache 容量取 `max(1, 0)`。
    model.configure_cache(Limits {
        page_bytes: 0,
        index_pages: 0,
    });
    assert_eq!(
        model.cache_status(),
        CacheStatus {
            page_bytes: 0,
            page_limit: 0,
            page_entries: 0,
            index_cache_limit: 1,
        }
    );
    model.close();
    assert_eq!(
        model.cache_status(),
        CacheStatus {
            page_bytes: 0,
            page_limit: 0,
            page_entries: 0,
            index_cache_limit: 1,
        }
    );
}

/// 损坏模型按参照实现的错误类别报错（magic / 版本 / 尺寸 / 布局 / 词表 / 越界读）。
#[test]
fn corrupt_models_are_rejected() {
    let source = std::fs::read(fixture("fivegram_fixture.bin")).expect("读夹具");
    let directory = std::env::temp_dir();
    let mut cases: Vec<(&str, Vec<u8>, &str)> = Vec::new();
    let mut magic = source.clone();
    magic[..8].copy_from_slice(b"TCSKNM02");
    cases.push(("magic", magic, "not a TCSKNM03 model"));
    let mut version = source.clone();
    version[8..12].copy_from_slice(&2u32.to_le_bytes());
    cases.push(("version", version, "unsupported TCSKNM03 version"));
    let mut header_size = source.clone();
    header_size[12..16].copy_from_slice(&128u32.to_le_bytes());
    cases.push(("header_size", header_size, "unsupported TCSKNM03 version"));
    let mut size = source.clone();
    size[16..24].copy_from_slice(&(source.len() as u64 + 1).to_le_bytes());
    cases.push(("file_size", size, "TCSKNM03 size mismatch"));
    let mut order = source.clone();
    order[24..28].copy_from_slice(&4u32.to_le_bytes());
    cases.push(("order", order, "invalid TCSKNM03 layout"));
    let mut buckets = source.clone();
    buckets[32..36].copy_from_slice(&255u32.to_le_bytes());
    cases.push(("bucket_count", buckets, "invalid TCSKNM03 layout"));
    // 词表字节数多 1：解析完 6 条后仍有剩余 ⇒ 词表不吻合。
    let mut vocab_extra = source.clone();
    vocab_extra[48..56].copy_from_slice(&58u64.to_le_bytes());
    cases.push(("vocab_extra", vocab_extra, "invalid TCSKNM03 vocabulary"));
    // 词表区间越出文件末尾 ⇒ 越界读。
    let mut vocab_over = source.clone();
    vocab_over[48..56].copy_from_slice(&(1u64 << 40).to_le_bytes());
    cases.push(("vocab_overflow", vocab_over, "truncated TCSKNM03"));
    for (name, bytes, expected) in cases {
        let path = directory.join(format!("hux-fivegram-{}-{name}.bin", std::process::id()));
        std::fs::write(&path, &bytes).expect("写损坏模型");
        let Err(error) = FivegramModel::load(&path, None) else {
            panic!("{name}: 损坏模型必须报错");
        };
        assert!(error.to_string().contains(expected), "{name}: {error}");
        std::fs::remove_file(&path).ok();
    }
    // 短于 256 B：参照实现先读满头部，故报 truncated 而非 magic 不符。
    let path = directory.join(format!("hux-fivegram-tiny-{}.bin", std::process::id()));
    std::fs::write(&path, b"TCSKNM03").expect("写短文件");
    let Err(error) = FivegramModel::load(&path, None) else {
        panic!("tiny: 短文件必须报错");
    };
    assert!(error.to_string().contains("truncated TCSKNM03"), "{error}");
    std::fs::remove_file(&path).ok();
}

/// 头部 `unknown_id` 越界（损坏模型）：查询时按错误返回，而不是 panic 在 `unigram_p` 下标上。
#[test]
fn out_of_range_unknown_id_reports_error_instead_of_panicking() {
    let source = std::fs::read(fixture("fivegram_fixture.bin")).expect("读夹具");
    let mut corrupted = source.clone();
    corrupted[56..58].copy_from_slice(&60_000u16.to_le_bytes());
    let path =
        std::env::temp_dir().join(format!("hux-fivegram-unknown-{}.bin", std::process::id()));
    std::fs::write(&path, &corrupted).expect("写损坏模型");
    let mut model = FivegramModel::load(&path, None).expect("头部其余字段合法，仍可装载");
    assert_eq!(model.unknown_id(), 60_000);
    // 词表内 token 不受影响。
    assert_eq!(
        model.step(1, 0, 0, 0, 1, "你").expect("打分").score,
        -0.46051703942569144
    );
    // 词表外 token ⇒ `id = unknown_id` 越界 ⇒ 概率缺失，参照实现的 nil 语义在这里是报错。
    let error = model
        .step(1, 0, 0, 0, 1, "不")
        .expect_err("越界 unknown_id 必须报错");
    assert!(
        error.to_string().contains("invalid TCSKNM03 vocabulary"),
        "{error}"
    );
    std::fs::remove_file(&path).ok();
}

/// page cache 的三段游标语义（字节压力循环 / 覆盖当前槽位 / 写入后推进）。
#[test]
fn page_cache_put_keeps_pressure_and_cursor_semantics() {
    // 单页大于上限：压力循环把环清空后 break，该页超限驻留。
    let mut cache = PageCache::new(16, 4);
    cache.put(1, Rc::new(vec![0u8; 64]));
    assert_eq!((cache.entries(), cache.bytes), (1, 64));
    // 键已在缓存中时不进压力循环，只覆盖当前槽位并推进游标；覆盖写入仍按参照实现
    // 二次累加字节（该分支只有直接调用才可达：`lookup` 只在未命中时写入）。
    cache.put(1, Rc::new(vec![0u8; 64]));
    assert_eq!((cache.entries(), cache.bytes), (1, 128));

    // 逐个换入 8 B 页：前 4 个逐个填满槽位（总字节可暂时超限）。
    let mut cache = PageCache::new(16, 4);
    for key in 1..=4u64 {
        cache.put(key, Rc::new(vec![0u8; 8]));
    }
    assert_eq!((cache.entries(), cache.bytes, cache.next), (4, 32, 1));
    // 第 5 个：压力循环按槽位顺序牺牲 1..3 号（游标随循环体推进），
    // 再覆盖 4 号槽位，最后写入并推进游标。
    cache.put(5, Rc::new(vec![0u8; 8]));
    assert_eq!((cache.entries(), cache.bytes, cache.next), (1, 8, 1));
    assert!(cache.get(5).is_some());
    for key in 1..=4u64 {
        assert!(cache.get(key).is_none(), "键 {key} 应已被淘汰");
    }
}

/// order-3 桶 3 有 65 个 block：多索引点、页边界，以及页内三种提前返回。
#[test]
fn paged_fixture_multi_index_points_and_page_boundary() {
    let mut model = paged();
    assert_eq!(model.bytes(), 43_798);
    // page 0（block 0..63，768 B）的末块：线性扫描要走完整页。
    let last = model.step(94, 3, 0, 0, 2, "Y").expect("打分");
    assert_eq!(last.score, -0.33686825197238263);
    assert_eq!(last.score.to_bits(), 0xbfd58f3fdb520d73);
    // 第二个索引点起的 page（block 64，12 B，右端点取索引区起点）。
    let next = model.step(95, 3, 0, 0, 2, "Y").expect("打分");
    assert_eq!(next.score, -0.33709845761432833);
    assert_eq!(next.score.to_bits(), 0xbfd5930568bf13c8);
    // page 0 首块。
    let first = model.step(30, 3, 0, 0, 2, "Y").expect("打分");
    assert_eq!(first.score, -0.29933577989360727);
    assert_eq!(first.score.to_bits(), 0xbfd32851424a9aca);
    // 查询上下文排在 block 0 与 block 1 之间 ⇒ `compared > 0` 提前返回，不累加任何回退。
    let between = model.step(31, 3, 0, 0, 2, "Y").expect("打分");
    assert_eq!(between.score, -2.9933558244048832);
    assert_eq!(between.score.to_bits(), 0xc007f26489d8e178);
    // 查询上下文排在首个索引点之前 ⇒ `low == 0` 提前返回（不进 page）。
    let before = model.step(9, 3, 0, 0, 2, "Y").expect("打分");
    assert_eq!(before.score, -3.5690090676705748);
    assert_eq!(before.score.to_bits(), 0xc00c8d54a0462e39);
    // 以上查询共换入 3 张 page：(3,3,0)=768 B、(3,3,1)=12 B、(2,9,0)=6 B。
    assert_eq!(
        model.cache_status(),
        CacheStatus {
            page_bytes: 786,
            page_limit: 8 * 1024 * 1024,
            page_entries: 3,
            index_cache_limit: 64,
        }
    );
}

/// `bow_q == 0` 的块、`count == 0` 的空 block，以及空桶短路。
#[test]
fn paged_fixture_zero_bow_and_empty_blocks() {
    let mut model = paged();
    // 上下文 `(P,x00)` 的 `bow_q == 0`（有后继但无回退）⇒ 命中而 target 缺失时回退 0.0，
    // 分数由下一级（order-2 的 `x00` 块回退 -0.55）与 unigram 决定。
    let zero_bow = model.step(30, 3, 0, 0, 2, "Z").expect("打分");
    assert_eq!(zero_bow.score, -4.4900073320351339);
    assert_eq!(zero_bow.score.to_bits(), 0xc011f5c47b679080);
    // order-3 空 block（`count == 0`、`bow_q != 0`）：`(W,V)` 只有回退、无后继。
    let empty_three = model.step(6, 7, 0, 0, 2, "T").expect("打分");
    assert_eq!(empty_three.score, -1.8420593326217207);
    assert_eq!(empty_three.score.to_bits(), 0xbffd791334ee6e16);
    // order-2 空 block：`Q` 只有 unigram 回退（-0.4）、无后继。
    let empty_two = model.step(4, 0, 0, 0, 1, "Z").expect("打分");
    assert_eq!(empty_two.score, -4.1446230530946169);
    assert_eq!(empty_two.score.to_bits(), 0xc010941810cd27fd);
    // 词表外 target 走 `<unk>`：上下文 `P` 命中但无该后继 ⇒ 回退 -0.2 + unigram(`<unk>`) -1.0。
    let unknown = model.step(3, 0, 0, 0, 1, "zzz").expect("打分");
    assert_eq!(unknown.id, 2);
    assert_eq!(unknown.score, -2.7631049540052079);
    assert_eq!(unknown.score.to_bits(), 0xc0061ad6c526f191);
    // 空桶短路：`count == 0` 时 history 为 `[</s>]`，order-2 桶 0 为空 ⇒ 不加载任何 page。
    let mut fresh = paged();
    let zero_count = fresh.step(0, 0, 0, 0, 0, "Y").expect("打分");
    assert_eq!(zero_count.score, -2.9933558244048832);
    assert_eq!(fresh.cache_status().page_bytes, 0);
    assert_eq!(fresh.cache_status().page_entries, 0);
    let unknown_zero = fresh.step(0, 0, 0, 0, 0, "不存在").expect("打分");
    assert_eq!(unknown_zero.score, -2.3025915156314869);
}

/// 链式 `BOS → P → x00 → Y → T → </s>`：逐级命中 2/3/4/5-gram，随后回到 order 2 与空 block。
#[test]
fn paged_fixture_chain_hits_five_gram_and_backs_off() {
    let mut model = paged();
    let mut history = LmHistory::begin(model.bos_id());
    let steps = [
        ("P", -0.46051706804682419, 0xbfdd791c94a0b2db),
        ("x00", -0.25328442014260916, 0xbfd035cfdb46533b),
        ("Y", -0.18420680743952367, 0xbfc79416b2d3a01c),
        ("T", -0.069077552789821375, 0xbfb1af11061eb815),
        (EOS, -1.9572096647370174, 0xbfff50bb14d75ead),
    ];
    for (index, (target, expected, bits)) in steps.into_iter().enumerate() {
        let score = model.step_token(&mut history, target).expect("打分");
        assert_eq!(score, expected, "第 {} 步 target {target:?}", index + 1);
        assert_eq!(
            score.to_bits(),
            bits,
            "第 {} 步 target {target:?} 位模式",
            index + 1
        );
        // 第三步后 history 已满 4 个槽位：BOS 被顶到 `lm4` 并参与 5-gram 上下文。
        if index == 2 {
            assert_eq!(
                (
                    history.lm1,
                    history.lm2,
                    history.lm3,
                    history.lm4,
                    history.count
                ),
                (8, 30, 3, 1, 4)
            );
        }
    }
    assert_eq!(
        (
            history.lm1,
            history.lm2,
            history.lm3,
            history.lm4,
            history.count
        ),
        (0, 5, 8, 30, 4)
    );
    // 单字符入口与 token 串入口同分。
    let mut chars = LmHistory::begin(model.bos_id());
    assert_eq!(
        model.step_char(&mut chars, 'P').expect("打分"),
        -0.46051706804682419
    );
    assert_eq!(chars.lm1, 3);
}

/// 小 `page_bytes` + 1 个索引页：FIFO 环的淘汰与回访（含单页超限驻留）。
#[test]
fn paged_fixture_small_page_cache_evicts_and_revisits() {
    let mut model = paged();
    model.configure_cache(Limits {
        page_bytes: 64,
        index_pages: 1,
    });
    assert_eq!(
        model.cache_status(),
        CacheStatus {
            page_bytes: 0,
            page_limit: 64,
            page_entries: 0,
            index_cache_limit: 1,
        }
    );
    // 每次查询换一个 order-2 桶 ⇒ 每次换入一张 10 B 的 page；槽位 8 个、上限 64 B。
    let expected = [
        (10u64, 1usize),
        (20, 2),
        (30, 3),
        (40, 4),
        (50, 5),
        (60, 6),
        (70, 7),
        (80, 8),
        (50, 5),
        (50, 5),
    ];
    for (index, (bytes, entries)) in expected.into_iter().enumerate() {
        let source = 10 + index as u16;
        let target = format!("t{index:02}");
        let score = model.step(source, 0, 0, 0, 1, &target).expect("打分");
        assert!(score.score.is_finite(), "第 {index} 次查询");
        let status = model.cache_status();
        assert_eq!(
            (status.page_bytes, status.page_entries),
            (bytes, entries),
            "第 {index} 次查询后的 page 计数"
        );
    }
    // 回访仍在缓存里的 page：换入顺序改变（淘汰 s05、写入 s00 的槽位），计数不变。
    model.step(10, 0, 0, 0, 1, "t00").expect("打分");
    assert_eq!(
        (
            model.cache_status().page_bytes,
            model.cache_status().page_entries
        ),
        (50, 5)
    );
    // 单页 768 B > 上限 64 B：该页超限驻留（压力循环腾空两个槽位后 break）。
    model.step(94, 3, 0, 0, 2, "Y").expect("打分");
    assert_eq!(
        (
            model.cache_status().page_bytes,
            model.cache_status().page_entries
        ),
        (798, 4)
    );
    model.step(95, 3, 0, 0, 2, "Y").expect("打分");
    assert_eq!(
        (
            model.cache_status().page_bytes,
            model.cache_status().page_entries
        ),
        (810, 5)
    );
    // 仍在缓存里的 page：回访不换入（计数不变）。
    model.step(94, 3, 0, 0, 2, "Y").expect("打分");
    assert_eq!(
        (
            model.cache_status().page_bytes,
            model.cache_status().page_entries
        ),
        (810, 5)
    );
    // 已被大页淘汰的 page：回访重新换入。
    model.step(16, 0, 0, 0, 1, "t06").expect("打分");
    assert_eq!(
        (
            model.cache_status().page_bytes,
            model.cache_status().page_entries
        ),
        (820, 6)
    );
    model.step(11, 0, 0, 0, 1, "t01").expect("打分");
    assert_eq!(
        (
            model.cache_status().page_bytes,
            model.cache_status().page_entries
        ),
        (800, 4)
    );
    model.trim_caches();
    assert_eq!(
        model.cache_status(),
        CacheStatus {
            page_bytes: 0,
            page_limit: 64,
            page_entries: 0,
            index_cache_limit: 1,
        }
    );
    model.configure_cache(Limits {
        page_bytes: 32,
        index_pages: 2,
    });
    assert_eq!(
        model.cache_status(),
        CacheStatus {
            page_bytes: 0,
            page_limit: 32,
            page_entries: 0,
            index_cache_limit: 2,
        }
    );
    model.close();
    assert_eq!(model.cache_status().page_entries, 0);
}
