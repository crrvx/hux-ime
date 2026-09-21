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
    let mut punct = punct_table;
    for (index, step) in case.steps.iter().enumerate() {
        let label = format!("{}[{}] {}", case.name, index, step.repr);
        let key = KeyEvent::from_repr(&step.repr).expect("key repr");
        let mut env = ProcessorEnv {
            now: 0.0,
            dot_armed: &mut dot_armed,
            min_retained: None,
            page_size,
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
                host_process_key(
                    &key,
                    &mut context,
                    punct.as_mut(),
                    &HostOptions {
                        page_size,
                        ..HostOptions::default()
                    },
                    None,
                ) == HostResult::Consumed
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
                punct.as_mut(),
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
