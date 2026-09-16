//! 键序列金样（2c）重放：真 librime 探针记录 vs Rust 会话逐步比对。
//!
//! 比对字段：`consumed`、输入、光标、提交、候选（按页：数量/文本/高亮）。
//! `preedit`（预编辑串）属 K3 宿主职责，金样保留但不比对。
//! 处理器链：core `processor` 未消费（Forward）的键交 `host` 模块（librime
//! `key_binder`/`selector`/`navigator`/`express_editor` 等价物）后比对 `consumed`；
//! 组合重建用 core `CompositionBuilder`（参照 `ConcreteEngine::Compose`），
//! 之后执行 update 通知器等价物（`interaction::update_notifier`）。
//!
//! 金样与数据：`goldens/key_sequence.tsv.gz`、`goldens/key_sequence/`（合成小码表）。
//! 再生成：`tools/gen_key_sequence_golden.sh`（依赖系统 librime + librime-lua）。

mod common;

use common::hex;
use flate2::read::GzDecoder;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

use tigerclaw_core::ascii::{AsciiComposer, AsciiResult};
use tigerclaw_core::decode::Decoder;
use tigerclaw_core::host::{HostResult, process_key as host_process_key};
use tigerclaw_core::interaction::{
    CompositionBuilder, LearningCommit, LiveLearning, OPTION_EARLY_COMMIT,
    OPTION_EARLY_COMMIT_TO_PREEDIT, ProcessorEnv, ProcessorResult, SentenceState,
    ascii_mode_option_confirm, processor, update_notifier,
};
use tigerclaw_core::key::KeyEvent;
use tigerclaw_core::lexicon::{Lexicon, Supplement};
use tigerclaw_core::session::{Context, Event};

struct Step {
    repr: String,
    consumed: bool,
    input: String,
    caret: usize,
    commit: String,
    highlight: usize,
    count: usize,
    candidates: Vec<String>,
}

struct Case {
    name: String,
    options: Vec<(String, bool)>,
    steps: Vec<Step>,
}

fn load_cases() -> Vec<Case> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../goldens/key_sequence.tsv.gz");
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
                assert_eq!(fields.len(), 13, "step fields: {line}");
                let count: usize = fields[11].parse().expect("count");
                // `-` 既表示「无候选」也表示「单候选且文本为空」，用计数区分。
                let candidates = if count == 0 {
                    Vec::new()
                } else {
                    fields[12].split(',').map(str::to_string).collect()
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
                    });
            }
            other => panic!("unknown golden record: {other}"),
        }
    }
    cases
}

fn replay(case: &Case, data_dir: &Path, failures: &mut Vec<String>) {
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
    for (name, value) in &case.options {
        context.set_option(name, *value);
    }
    let mut builder = CompositionBuilder::default();
    let mut ascii = AsciiComposer::reference();
    for (index, step) in case.steps.iter().enumerate() {
        let label = format!("{}[{}] {}", case.name, index, step.repr);
        let key = KeyEvent::from_repr(&step.repr).expect("key repr");
        let mut consumed = false;
        let mut skip_processors = false;
        // 参照链首：ascii_composer（Accepted 吞键 / Rejected 交宿主并停止链 / Noop 继续）。
        match ascii.process_key(&key, &mut context, index as f64 * 0.01) {
            AsciiResult::Accepted => {
                consumed = true;
                skip_processors = true;
            }
            AsciiResult::Rejected => {
                skip_processors = true;
            }
            AsciiResult::Noop => {}
        }
        if !skip_processors {
            let mut env = ProcessorEnv {
                now: 0.0,
                dot_armed: &mut dot_armed,
                min_retained: None,
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
            consumed = match result {
                ProcessorResult::Consume => true,
                ProcessorResult::Forward => {
                    host_process_key(&key, &mut context) == HostResult::Consumed
                }
            };
        }
        // 事件泵：提交与选项事件（ascii_mode 确认可能再产生提交）。
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
                    Event::Option(name) => {
                        ascii_mode_option_confirm(
                            &name,
                            &mut context,
                            &mut state,
                            Some(&mut LearningCommit {
                                decoder: &mut decoder,
                                live: &mut live,
                                now: 0.0,
                            }),
                        );
                    }
                    Event::Update => {}
                }
            }
        }
        builder
            .rebuild(&mut decoder, &mut context, &state, commit_invalidated)
            .expect("rebuild");
        // 参照 update 通知器（暂存清理 / 缓冲隐藏）。
        update_notifier(&mut context, &mut state, &mut live);
        // 参照 `AsciiComposer::OnContextUpdate`：临时 ascii 随组合结束退出。
        ascii.on_context_update(&mut context);
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
        // 参照 `RimeGetContext`：按当前页上报候选与页内高亮（`menu/page_size: 5`）。
        let page_size = tigerclaw_core::host::DEFAULT_PAGE_SIZE;
        let highlight = segment
            .map(|segment| segment.selected_index % page_size)
            .unwrap_or(0);
        // 参照在 `_hide_candidate` 下把菜单候选数置 0（高亮照常上报）。
        let hidden = context.get_option("_hide_candidate");
        let page: Vec<&tigerclaw_core::session::Candidate> = match segment {
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
    }
}

#[test]
fn key_sequence_matches_reference() {
    let cases = load_cases();
    assert!(!cases.is_empty(), "empty key_sequence golden");
    let data_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../goldens/key_sequence");
    let mut failures = Vec::new();
    let mut steps = 0usize;
    for case in &cases {
        steps += case.steps.len();
        replay(case, &data_dir, &mut failures);
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
