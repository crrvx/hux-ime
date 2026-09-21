// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! 键序列金样（2c）重放：真 librime 探针记录 vs Rust 会话逐步比对。
//!
//! 比对字段：`consumed`、输入、光标、提交、候选（按页：数量/文本/注释/高亮）。
//! `preedit`（预编辑串）属 K3 宿主职责，金样保留但不比对。
//! 处理器链：方案 `processor` 未消费（Forward）的键交 core `host` 模块（librime
//! `key_binder`/`selector`/`navigator`/`express_editor` 等价物）后比对 `consumed`；
//! 组合重建用 core `CompositionBuilder`（参照 `ConcreteEngine::Compose`），
//! 之后执行 update 通知器等价物（`interaction::update_notifier`）。
//!
//! 金样与数据：`goldens/key_sequence.tsv.gz`、`goldens/key_sequence/`（合成小码表）；
//! 音反查：`goldens/sound_to_char_shape.tsv.gz`、`goldens/sound_to_char_shape/`（小 PY_c + 音反查索引夹具）。
//! 再生成：`tools/generators/gen_key_sequence_golden.sh`、`tools/generators/gen_sound_to_char_shape_golden.sh`
//! （依赖系统 librime + librime-lua）。

use flate2::read::GzDecoder;
use hux_test_support::hex;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

use hux_core::host::{HostOptions, HostResult, process_key as host_process_key};
use hux_core::key::KeyEvent;
use hux_core::punct::PunctTable;
use hux_core::session::{Context, Event};
use hux_scheme_tiger::decode::Decoder;
use hux_scheme_tiger::interaction::{
    CompositionBuilder, K_SOUND_TO_CHAR_SHAPE_KEY, LiveLearning, OPTION_EARLY_COMMIT,
    OPTION_EARLY_COMMIT_TO_PREEDIT, ProcessorEnv, ProcessorResult, SentenceState, processor,
    update_notifier,
};
use hux_scheme_tiger::lexicon::{Lexicon, Supplement};

/// 本仓**有意偏离上游**的金样用例登记表（金样字节保持原样，不重生成）。
///
/// **上游缺陷**（`abad411` `fix(rime): preserve punctuation learning …`）：方案处理器在
/// `context:has_menu()` 时把**所有**可打印 ASCII 标点先「暂存学习 + 确认组合」再交标点表，
/// 于是宿主 `key_binder` 的翻页绑定（缺省 `-`/`=`，以及 schema 绑到翻页的 `[`/`]`）
/// 在这条路径上被永久遮蔽 —— `Page_Up`/`Page_Down`/`Tab` 不受影响。
/// 最小复现：`j a equal` ⇒ 期望下翻页，上游提交「一=」。
/// 依据：`_tmp/批次5-追平记录.md`（探针实测）与 `lua/tiger_sentence.lua` @ `abad411` 的标点分支；
/// 用例定义见 `tools/cases/key_sequence_cases.txt`、`tools/cases/sound_to_char_shape_cases.txt`。
///
/// **本仓修法**：标点分支入口先问与宿主**同一套**判据 `hux_core::host::paging_action`，
/// 被判为翻页的键不由标点分支消费（详见 `docs/refactor.md` §8「有意偏离上游」）。
/// 下列用例的期望值是**上游行为的记录**，与本仓新语义不符，故在重放中跳过并断言
/// 「实际跳过集合恰好等于本登记表」（见 [`without_deviated`] 与
/// `deviated_cases_are_registered_with_the_expected_golden`）：
///
/// | 金样 | 用例 | 步数 | 偏离步 | 上游 vs 本仓 |
/// |---|---|---|---|---|
/// | `key_sequence.tsv.gz` | `punct_menu_equal` | 3 | 2（`equal`） | `j a` 后提交「一=」 vs **翻页**（不提交） |
/// | `sound_to_char_shape.tsv.gz` | `nav-page-equal` | 4 | 2,3（`=`） | 上屏「中=」再落「=」 vs **连翻两页** |
/// | `sound_to_char_shape.tsv.gz` | `nav-page-minus` | 5 | 2,3,4（`=`/`=`/`-`） | 同上＋落「-」 vs **翻两页后上翻一页** |
/// | `sound_to_char_shape.tsv.gz` | `nav-page-zho` | 6 | 4,5（`=`/`-`） | 上屏「中哦=」再落「-」 vs **下翻一页后上翻一页** |
///
/// **待上游修复后移除本表**（把 `paging_action` 的提前放行改回上游分支即可复现），
/// 届时全部用例无条件重放、逐位比对。
const DEVIATED_CASES: &[&str] = &[
    // `j a equal`：`=` 的 `when: has_menu` 成立 ⇒ 本仓翻页，上游提交组合 + 落 `=`。
    "punct_menu_equal",
    // `` ` z = = ``：`=` 本仓连翻两页，上游两次「上屏组合 + 落 `=`」。
    "nav-page-equal",
    // `` ` z = = - ``：`=` 翻页后 `paging` 标签置位，`-`（`when: paging`）在本仓上翻一页。
    "nav-page-minus",
    // `` ` z h o = - ``：同上前半段，`-` 由标点变为上翻页。
    "nav-page-zho",
];

struct Step {
    repr: String,
    consumed: bool,
    input: String,
    caret: usize,
    commit: String,
    highlight: usize,
    count: usize,
    candidates: Vec<String>,
    comments: Vec<String>,
}

struct Case {
    name: String,
    options: Vec<(String, bool)>,
    steps: Vec<Step>,
}

fn load_cases(path: &Path) -> Vec<Case> {
    let file = std::fs::File::open(path).expect("open key_sequence golden");
    let mut cases: Vec<Case> = Vec::new();
    for line in BufReader::new(GzDecoder::new(file)).lines() {
        let line = line.expect("golden line");
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let fields: Vec<&str> = line.split('\t').collect();
        match fields[0] {
            "case" => {
                let options = fields
                    .get(2)
                    .filter(|value| !value.is_empty())
                    .map(|value| {
                        value
                            .split(',')
                            .map(|item| {
                                let (name, value) =
                                    item.split_once('=').expect("option assignment");
                                (name.to_string(), value == "1")
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                cases.push(Case {
                    name: fields[1].to_string(),
                    options,
                    steps: Vec::new(),
                });
            }
            "step" => {
                assert_eq!(fields.len(), 14, "step fields: {line}");
                let count: usize = fields[11].parse().expect("count");
                // `-` 既表示「无候选」也表示「单候选且文本为空」，用计数区分。
                let candidates = if count == 0 {
                    Vec::new()
                } else {
                    fields[12].split(',').map(str::to_string).collect()
                };
                let comments = if count == 0 {
                    Vec::new()
                } else {
                    fields[13].split(',').map(str::to_string).collect()
                };
                cases
                    .last_mut()
                    .expect("case record before step")
                    .steps
                    .push(Step {
                        repr: fields[3].to_string(),
                        consumed: fields[4] == "1",
                        input: fields[5].to_string(),
                        caret: fields[6].parse().expect("caret"),
                        commit: fields[7].to_string(),
                        highlight: fields[10].parse().expect("highlight"),
                        count,
                        candidates,
                        comments,
                    });
            }
            other => panic!("unknown golden record: {other}"),
        }
    }
    cases
}

/// 剔除登记在册的「有意偏离」用例，返回待重放的用例。
///
/// 自校验：本金样中**实际跳过**的用例集合必须恰好等于 [`DEVIATED_CASES`] 中确实存在于
/// 本金样者 —— 任何未登记却被跳过的用例都会让断言失败（不静默跳过）；
/// 登记名是否真实存在，由 `deviated_cases_are_registered_with_the_expected_golden` 跨两份金样核对。
fn without_deviated<'a>(cases: &'a [Case], golden: &str) -> Vec<&'a Case> {
    let mut kept = Vec::new();
    let mut skipped: Vec<&str> = Vec::new();
    for case in cases {
        if DEVIATED_CASES.contains(&case.name.as_str()) {
            skipped.push(case.name.as_str());
        } else {
            kept.push(case);
        }
    }
    let mut expected: Vec<&str> = DEVIATED_CASES
        .iter()
        .copied()
        .filter(|name| cases.iter().any(|case| case.name == *name))
        .collect();
    expected.sort_unstable();
    skipped.sort_unstable();
    assert_eq!(
        skipped, expected,
        "{golden}: 实际跳过的用例与 DEVIATED_CASES 在本金样内的登记不一致\
         （不得跳过未登记的用例，也不得漏跳已登记的用例）"
    );
    kept
}

fn replay(
    case: &Case,
    data_dir: &Path,
    page_size: usize,
    lookup_key: Option<&str>,
    failures: &mut Vec<String>,
) {
    let dirs = [data_dir.to_path_buf()];
    let lexicon = Lexicon::load(&dirs, 0);
    let supplement = Supplement::load_default(Some(data_dir));
    let mut decoder = Decoder::new(lexicon, supplement, None);
    let mut context = Context::new();
    let mut state = SentenceState::fresh(1);
    let mut live = LiveLearning::default();
    let mut dot_armed = false;
    context.set_option("ascii_mode", false);
    context.set_option("_auto_commit", true);
    context.set_option(OPTION_EARLY_COMMIT, true);
    context.set_option(OPTION_EARLY_COMMIT_TO_PREEDIT, false);
    if let Some(key) = lookup_key {
        context.set_property(K_SOUND_TO_CHAR_SHAPE_KEY, key);
    }
    for (name, value) in &case.options {
        context.set_option(name, *value);
    }
    let mut builder = CompositionBuilder::default();
    let (punct_table, punct_error) = PunctTable::load_first(&[data_dir.join("symbols.yaml")]);
    assert!(
        punct_table.is_some(),
        "缺少标点表 symbols.yaml：{punct_error:?}"
    );
    let punct = punct_table;
    // 处理器与宿主链共用同一份宿主选项（翻页键绑定判据一致；见 `ProcessorEnv::host_options`）。
    let host_options = HostOptions {
        page_size,
        ..HostOptions::default()
    };
    for (index, step) in case.steps.iter().enumerate() {
        let label = format!("{}[{}] {}", case.name, index, step.repr);
        let key = KeyEvent::from_repr(&step.repr).expect("key repr");
        let mut env = ProcessorEnv {
            now: 0.0,
            dot_armed: &mut dot_armed,
            min_retained: None,
            page_size,
            host_options: &host_options,
        };
        let result = processor(
            &key,
            &mut context,
            &mut state,
            &mut decoder,
            &mut live,
            &mut env,
        )
        .expect("processor");
        // 参照链：处理器未消费的键交宿主等价物（selector/navigator/express_editor 等）。
        let consumed = match result {
            ProcessorResult::Consume => true,
            ProcessorResult::Forward => {
                host_process_key(&key, &mut context, punct.as_ref(), &host_options, None)
                    == HostResult::Consumed
            }
        };
        // 事件泵：提交与选项事件。
        let mut committed = String::new();
        let mut commit_invalidated = false;
        for _ in 0..4 {
            let events = context.drain_events();
            if events.is_empty() {
                break;
            }
            for event in events {
                match event {
                    Event::Commit(text) => {
                        commit_invalidated = true;
                        committed.push_str(&text);
                    }
                    Event::Option(_) => {}
                    Event::Update => {}
                }
            }
        }
        builder
            .rebuild(
                &mut decoder,
                &mut context,
                &state,
                commit_invalidated,
                punct.as_ref(),
            )
            .expect("rebuild");
        // 参照 update 通知器（暂存清理 / 缓冲隐藏）。
        update_notifier(&mut context, &mut state, &mut live);
        // 比对。
        let input = context.input().to_vec();
        if consumed != step.consumed {
            failures.push(format!(
                "{label}: consumed 期望 {} 实际 {}",
                step.consumed, consumed
            ));
        }
        if hex(&input) != step.input {
            failures.push(format!(
                "{label}: input 期望 {} 实际 {}",
                step.input,
                hex(&input)
            ));
        }
        if context.caret() != step.caret {
            failures.push(format!(
                "{label}: caret 期望 {} 实际 {}",
                step.caret,
                context.caret()
            ));
        }
        if hex(committed.as_bytes()) != step.commit {
            failures.push(format!(
                "{label}: commit 期望 {} 实际 {}",
                step.commit,
                hex(committed.as_bytes())
            ));
        }
        let segment = context.composition.back();
        // 参照 `RimeGetContext`：按当前页上报候选与页内高亮（夹具 `menu/page_size`）。
        let highlight = segment
            .map(|segment| segment.selected_index % page_size)
            .unwrap_or(0);
        // 参照在 `_hide_candidate` 下把菜单候选数置 0（高亮照常上报）。
        let hidden = context.get_option("_hide_candidate");
        let page: Vec<&hux_core::session::Candidate> = match segment {
            Some(segment) if !hidden => {
                let start = (segment.selected_index / page_size) * page_size;
                let end = (start + page_size).min(segment.candidates.len());
                if start < end {
                    segment.candidates[start..end].iter().collect()
                } else {
                    Vec::new()
                }
            }
            _ => Vec::new(),
        };
        let count = page.len();
        let candidates: Vec<String> = page
            .iter()
            .map(|candidate| hex(candidate.text.as_bytes()))
            .collect();
        let comments: Vec<String> = page
            .iter()
            .map(|candidate| hex(candidate.comment.as_bytes()))
            .collect();
        if highlight != step.highlight {
            failures.push(format!(
                "{label}: highlight 期望 {} 实际 {highlight}",
                step.highlight
            ));
        }
        if count != step.count {
            failures.push(format!(
                "{label}: candidate count 期望 {} 实际 {count}",
                step.count
            ));
        }
        if candidates != step.candidates {
            failures.push(format!(
                "{label}: candidates 期望 {:?} 实际 {candidates:?}",
                step.candidates
            ));
        }
        if comments != step.comments {
            failures.push(format!(
                "{label}: comments 期望 {:?} 实际 {comments:?}",
                step.comments
            ));
        }
    }
}

#[test]
fn key_sequence_matches_reference() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..");
    let cases = load_cases(&root.join("goldens/key_sequence.tsv.gz"));
    assert!(!cases.is_empty(), "empty key_sequence golden");
    // 上游金样原样保留：其中「上游缺陷」用例见 DEVIATED_CASES（有意偏离，跳过而非重生成）。
    let cases = without_deviated(&cases, "key_sequence.tsv.gz");
    let data_dir = root.join("goldens/key_sequence");
    let mut failures = Vec::new();
    let mut steps = 0usize;
    for case in &cases {
        steps += case.steps.len();
        replay(
            case,
            &data_dir,
            hux_core::host::DEFAULT_PAGE_SIZE,
            None,
            &mut failures,
        );
    }
    assert!(
        failures.is_empty(),
        "键序列不一致 {} 处（{} 例 / {} 步）：\n{}",
        failures.len(),
        cases.len(),
        steps,
        failures.join("\n")
    );
}

/// 音反查金样：夹具页大小 5（与引擎一致，翻页用例覆盖后续页）；音反查前缀 `` ` ``。
#[test]
fn sound_to_char_shape_matches_reference() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..");
    let cases = load_cases(&root.join("goldens/sound_to_char_shape.tsv.gz"));
    assert!(!cases.is_empty(), "empty pinyin lookup golden");
    // 同上：`nav-page-*` 记录的是上游「`=`/`-` 被标点分支遮蔽」的行为，见 DEVIATED_CASES。
    let cases = without_deviated(&cases, "sound_to_char_shape.tsv.gz");
    let data_dir = root.join("goldens/sound_to_char_shape");
    let mut failures = Vec::new();
    let mut steps = 0usize;
    for case in &cases {
        steps += case.steps.len();
        replay(
            case,
            &data_dir,
            hux_core::host::DEFAULT_PAGE_SIZE,
            Some("grave"),
            &mut failures,
        );
    }
    assert!(
        failures.is_empty(),
        "音反查不一致 {} 处（{} 例 / {} 步）：\n{}",
        failures.len(),
        cases.len(),
        steps,
        failures.join("\n")
    );
}

/// 偏离登记表自校验：每个登记名必须**真实存在**于两份金样之一（且不重名），
/// 两份金样实际跳过的用例总数 == 登记数 —— 登记与跳过精确对齐，不多不少。
#[test]
fn deviated_cases_are_registered_with_the_expected_golden() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..");
    let mut matched: Vec<String> = Vec::new();
    for golden in ["key_sequence.tsv.gz", "sound_to_char_shape.tsv.gz"] {
        let cases = load_cases(&root.join("goldens").join(golden));
        for case in &cases {
            if DEVIATED_CASES.contains(&case.name.as_str()) {
                assert!(
                    !matched.iter().any(|name| name == &case.name),
                    "偏离用例名 `{}` 在两份金样中重名，登记表无法唯一定位",
                    case.name
                );
                matched.push(case.name.clone());
            }
        }
    }
    let mut expected: Vec<String> = DEVIATED_CASES
        .iter()
        .map(|name| (*name).to_string())
        .collect();
    expected.sort_unstable();
    matched.sort_unstable();
    assert_eq!(
        matched, expected,
        "DEVIATED_CASES 中每个用例名都必须在金样中真实存在（不存在者会静默失去守护）"
    );
    assert_eq!(
        matched.len(),
        DEVIATED_CASES.len(),
        "被跳过的用例数必须等于登记数"
    );
}
