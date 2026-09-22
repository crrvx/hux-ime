// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! 键序列金样（2c）重放：真 librime 探针记录 vs Rust 会话逐步比对。
//!
//! 比对字段：`consumed`、输入、光标、提交、候选（按页：页码/数量/文本/注释/高亮）。
//! `preedit`（预编辑串）属宿主层职责，金样保留但不比对；
//! `page_no`（`fields[9]`）纳入比对（此前只以页切片间接约束）。
//! 处理器链：方案 `processor` 未消费（Forward）的键交 core `host` 模块（librime
//! `key_binder`/`selector`/`navigator`/`express_editor` 等价物）后比对 `consumed`；
//! 组合重建用 core `CompositionBuilder`（参照 `ConcreteEngine::Compose`），
//! 之后执行 update 通知器等价物（`interaction::update_notifier`）。
//!
//! 金样与数据：`goldens/key_sequence.tsv.gz`、`goldens/key_sequence/`（合成小码表）；
//! 音反查：`goldens/sound_to_char_shape.tsv.gz`、`goldens/sound_to_char_shape/`（小 PY_c + 音反查索引夹具）。
//! 再生成：`tools/generators/gen_key_sequence_golden.sh`、`tools/generators/gen_sound_to_char_shape_golden.sh`
//! （依赖系统 librime + librime-lua）。
//!
//! # 偏离守护（可证伪）
//!
//! 金样记录的是**上游行为**，本仓有若干处**已登记**的差异（见 [`DEVIATIONS`]）。
//! 登记项不是「跳过名单」而是**期望值表**，逐条满足：
//!
//! 1. **逐位断言**：该用例每一步都必须等于登记表里的**本仓期望行**（`consumed`/输入/
//!    光标/提交/高亮/候选数/候选/注释，与金样同字段）；
//! 2. **确有差异**：`期望行 != 金样行` 的步集合必须与 `实测 != 金样行` 的步集合**完全一致**，
//!    且非空 —— 实现若回退成上游行为，或某天上游 pin/金样追平使差异消失，都会**失败**
//!    （而不是静默通过）；登记表里写一个「实际并不偏离」的用例同样会失败；
//! 3. **不得静默跳过**：实际跳过的用例集合必须恰好等于登记集合（[`split_cases`]），
//!    登记名必须真实存在、唯一、步数吻合（`registry_is_falsifiable`）。
//!
//! 负面路径（把实现改回上游行为 / 塞入一个不偏离的用例）必须让本测试变红。

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
    CompositionBuilder, K_SOUND_TO_CHAR_SHAPE_KEY, LiveLearning, OPTION_DIGIT_SELECT,
    OPTION_EARLY_COMMIT, OPTION_EARLY_COMMIT_TO_PREEDIT, ProcessorEnv, ProcessorResult,
    SentenceState, processor, update_notifier,
};
use hux_scheme_tiger::lexicon::{Lexicon, Supplement};

// ---------------------------------------------------------------- 偏离登记表

/// 偏离的**种类**（决定差异来源、影响面与回归做法）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum DeviationKind {
    /// 上游缺陷：`abad411` 起方案处理器在菜单可见时把**所有**可打印 ASCII 标点先
    /// 「暂存学习 + 确认组合」再交标点表，宿主 `key_binder` 的翻页绑定被永久遮蔽。
    /// 本仓判定为缺陷并**有意修复**：
    /// 标点分支先问宿主判据，判为翻页的键让给宿主链。
    ///
    /// **含用户决定的语义强化（2026-09）**：上翻页键不再要求参照 `when: paging` 的
    /// 末段标签——菜单可见即拦截（与下翻页同前置），故 `punct_menu_minus`（首屏 `-`）
    /// 也从「与上游逐位一致」转为偏离项。**代价**：菜单可见时 `-`/`=`/`[`/`]`
    /// 不再能作为标点打出（已接受该代价）。
    UpstreamDefectFix,
    /// addon 扩展：`tiger_sentence_digit_select`（出厂缺省 **true**）在上游方案核心里
    /// 不存在（上游把数字当作编码字符入串）。金样按上游行为记录，重放按出厂缺省开启。
    AddonExtension,
    /// pin 差异（上游分支自身的后续提交）：本仓分段常量 `SEGMENTATION_DELIMITER`
    /// 追踪反查分支尖端 `92a0b54` 的 schema（`speller/delimiter: " '"`），而本金样的
    /// 主干 pin `abad411` 是 `" "` ⇒ `'` 之后的 `1`/`;` 在本仓切成「abc 段 + raw 段」，
    /// 上游主干保持单段。同 pin 的探针实测（`PIN=92a0b54` 重跑同一探针）与**本仓行完全
    /// 相同**，即该差异是上游自己后续提交带来的，不是本仓发明。
    BranchPinDelimiter,
}

/// 一个登记项：金样中一个与本仓行为**确有差异**的用例 + 本仓的逐步期望值。
struct Deviation {
    /// 金样文件名。
    golden: &'static str,
    /// 用例名（必须在该金样中真实存在且唯一）。
    case: &'static str,
    kind: DeviationKind,
    /// 本仓期望的逐步记录，与金样 `step` 行**同字段**（去掉 `step`/用例名/序号三列）：
    /// `repr \t consumed \t input \t caret \t commit \t highlight \t count \t candidates \t comments`
    /// （`input`/`commit`/候选/注释为 hex；`count == 0` 时候选与注释写 `-`）。
    steps: &'static [&'static str],
}

use DeviationKind::{AddonExtension, BranchPinDelimiter, UpstreamDefectFix};

/// 登记表（金样字节保持原样，不重生成）。**每一项都必须确有差异**，否则
/// `deviated_cases_match_their_registered_expectations` 会报「偏离已消失」。
const DEVIATIONS: &[Deviation] = &[
    // ---- 上游缺陷修复（翻页放行）------------------------------------------------
    Deviation {
        golden: "key_sequence.tsv.gz",
        case: "punct_menu_equal",
        kind: UpstreamDefectFix,
        // `j a equal`：`=` 的 `when: has_menu` 成立 ⇒ 本仓翻页（不提交）。
        steps: &[
            "j\t1\t6a\t1\t-\t0\t0\t0\t-\t-",
            "a\t1\t6a61\t2\t-\t0\t0\t5\te4b880,e4b881,e4b882,e4b883,e4b884\t-,-,-,-,-",
            "equal\t1\t6a61\t2\t-\t1\t0\t5\te4b885,e4b886,e4b887,e4b888,e4b889\t-,-,-,-,-",
        ],
    },
    Deviation {
        golden: "key_sequence.tsv.gz",
        case: "punct_menu_minus",
        kind: UpstreamDefectFix,
        // `j a minus`：上翻页键在**菜单可见**时即判翻页（本仓语义强化：不要求参照
        // `when: paging` 的末段标签）⇒ 本仓上翻页（首屏归零高亮、不提交）。
        // **代价（用户已接受）**：菜单可见时 `-` 不再能作为标点打出。
        steps: &[
            "j\t1\t6a\t1\t-\t0\t0\t0\t-\t-",
            "a\t1\t6a61\t2\t-\t0\t0\t5\te4b880,e4b881,e4b882,e4b883,e4b884\t-,-,-,-,-",
            "minus\t1\t6a61\t2\t-\t0\t0\t5\te4b880,e4b881,e4b882,e4b883,e4b884\t-,-,-,-,-",
        ],
    },
    Deviation {
        golden: "key_sequence.tsv.gz",
        case: "nav_page_home_minus",
        kind: UpstreamDefectFix,
        // `a b Page_Up minus`：`-` 在菜单可见时判上翻页（高亮仍在首页，归零）⇒
        // 不提交、输入不变；上游在确认组合时清空组合 ⇒ 落标点提交「乙-」。
        steps: &[
            "a\t1\t61\t1\t-\t0\t0\t0\t-\t-",
            "b\t1\t6162\t2\t-\t0\t0\t2\te794b2,e4b999\t-,-",
            "Page_Up\t1\t6162\t2\t-\t0\t0\t2\te794b2,e4b999\t-,-",
            "minus\t1\t6162\t2\t-\t0\t0\t2\te794b2,e4b999\t-,-",
        ],
    },
    Deviation {
        golden: "sound_to_char_shape.tsv.gz",
        case: "nav-page-equal",
        kind: UpstreamDefectFix,
        // `` ` z = = ``：`=` 本仓下翻一页（第 2 页；fixture 共 2 页，第二次 `=`
        // 已在末页 ⇒ 原地不动、不循环）；上游两次「上屏组合 + 落 `=`」。
        steps: &[
            "`\t1\t60\t1\t-\t0\t0\t1\t60\te38094e58d8ae8a792e38095",
            "z\t1\t607a\t2\t-\t0\t0\t5\te4b8ad,e591a8,e9878d,e7a78d,e59ca8\t2064202f206467202f20646773,207670202f20767071,206c646c,20786467202f2078646773,206e202f206e67202f206e676774",
            "=\t1\t607a\t2\t-\t1\t0\t5\te8bf99,e79c9f,e9929f,e8bdb4,e59e9a\t207675202f20767563,206e71202f206e716862,207a6467202f207a646773,2076707a,-",
            "=\t1\t607a\t2\t-\t1\t0\t5\te8bf99,e79c9f,e9929f,e8bdb4,e59e9a\t207675202f20767563,206e71202f206e716862,207a6467202f207a646773,2076707a,-",
        ],
    },
    Deviation {
        golden: "sound_to_char_shape.tsv.gz",
        case: "nav-page-minus",
        kind: UpstreamDefectFix,
        // `` ` z = = - ``：`=` 翻页后 `paging` 标签置位，`-`（`when: paging`）在本仓上翻一页。
        steps: &[
            "`\t1\t60\t1\t-\t0\t0\t1\t60\te38094e58d8ae8a792e38095",
            "z\t1\t607a\t2\t-\t0\t0\t5\te4b8ad,e591a8,e9878d,e7a78d,e59ca8\t2064202f206467202f20646773,207670202f20767071,206c646c,20786467202f2078646773,206e202f206e67202f206e676774",
            "=\t1\t607a\t2\t-\t1\t0\t5\te8bf99,e79c9f,e9929f,e8bdb4,e59e9a\t207675202f20767563,206e71202f206e716862,207a6467202f207a646773,2076707a,-",
            "=\t1\t607a\t2\t-\t1\t0\t5\te8bf99,e79c9f,e9929f,e8bdb4,e59e9a\t207675202f20767563,206e71202f206e716862,207a6467202f207a646773,2076707a,-",
            "-\t1\t607a\t2\t-\t0\t0\t5\te4b8ad,e591a8,e9878d,e7a78d,e59ca8\t2064202f206467202f20646773,207670202f20767071,206c646c,20786467202f2078646773,206e202f206e67202f206e676774",
        ],
    },
    Deviation {
        golden: "sound_to_char_shape.tsv.gz",
        case: "nav-page-zho",
        kind: UpstreamDefectFix,
        // `` ` z h o = - ``：同上前半段，`-` 由标点变为上翻页。
        steps: &[
            "`\t1\t60\t1\t-\t0\t0\t1\t60\te38094e58d8ae8a792e38095",
            "z\t1\t607a\t2\t-\t0\t0\t5\te4b8ad,e591a8,e9878d,e7a78d,e59ca8\t2064202f206467202f20646773,207670202f20767071,206c646c,20786467202f2078646773,206e202f206e67202f206e676774",
            "h\t1\t607a68\t3\t-\t0\t0\t5\te4b8ad,e591a8,e9878d,e7a78d,e8bf99\t2064202f206467202f20646773,207670202f20767071,206c646c,20786467202f2078646773,207675202f20767563",
            "o\t1\t607a686f\t4\t-\t0\t0\t5\te4b8ade593a6,e4b8ade9be98,e4b8ade6aca7,e689bee593a6,e58586e6aca7\t20e4b8ad3a642f64672f64677320e593a63a6474752f64747570,20e4b8ad3a642f64672f64677320e9be983a3f,20e4b8ad3a642f64672f64677320e6aca73a6e62652f6e626571,20e689be3a75702f75706720e593a63a6474752f64747570,20e585863a7077772f7077776220e6aca73a6e62652f6e626571",
            "=\t1\t607a686f\t4\t-\t1\t0\t1\te689bee6aca7\t20e689be3a75702f75706720e6aca73a6e62652f6e626571",
            "-\t1\t607a686f\t4\t-\t0\t0\t5\te4b8ade593a6,e4b8ade9be98,e4b8ade6aca7,e689bee593a6,e58586e6aca7\t20e4b8ad3a642f64672f64677320e593a63a6474752f64747570,20e4b8ad3a642f64672f64677320e9be983a3f,20e4b8ad3a642f64672f64677320e6aca73a6e62652f6e626571,20e689be3a75702f75706720e593a63a6474752f64747570,20e585863a7077772f7077776220e6aca73a6e62652f6e626571",
        ],
    },
    // ---- addon 扩展：数字直选（出厂缺省 true）-----------------------------------
    Deviation {
        golden: "key_sequence.tsv.gz",
        case: "digit_menu_select",
        kind: AddonExtension,
        // `a b 1`：本仓按出厂缺省直选当前页第 1 个候选并上屏；上游把 `1` 并入编码。
        steps: &[
            "a\t1\t61\t1\t-\t0\t0\t0\t-\t-",
            "b\t1\t6162\t2\t-\t0\t0\t2\te794b2,e4b999\t-,-",
            "1\t1\t-\t0\te794b2\t0\t0\t0\t-\t-",
        ],
    },
    // ---- pin 差异：分段常量追踪反查分支尖端 `92a0b54`（见 DeviationKind 注释）--------
    Deviation {
        golden: "key_sequence.tsv.gz",
        case: "apostrophe_digit_page",
        kind: BranchPinDelimiter,
        // `a b ' 1 Page_Down`：`'`+`1` 在本仓切成 abc 段 `ab'` + raw 段 `1`
        // （末段是 raw 段 ⇒ 无菜单），`Page_Down` 因此**不被消费**；上游主干单段
        // `ab'1` 是 abc 段 ⇒ 消费。`PIN=92a0b54` 的探针实测与本表逐位相同。
        steps: &[
            "a\t1\t61\t1\t-\t0\t0\t0\t-\t-",
            "b\t1\t6162\t2\t-\t0\t0\t2\te794b2,e4b999\t-,-",
            "apostrophe\t1\t616227\t3\t-\t0\t0\t0\t-\t-",
            "1\t1\t61622731\t4\t-\t0\t0\t0\t-\t-",
            "Page_Down\t0\t61622731\t4\t-\t0\t0\t0\t-\t-",
        ],
    },
    Deviation {
        golden: "key_sequence.tsv.gz",
        case: "apostrophe_semicolon_page",
        kind: BranchPinDelimiter,
        // `a b ' ; Page_Down`：同上是 `;` 变体（两 pin 的差异同样落在末段类型上）。
        // 注：反查分支尖端另有 `punct_segmentor` 把 `;` 直接落成全角「；」——那属本仓
        // 未移植的分段器范围（标点由宿主表处理），与本项 delimiter 差异无关。
        steps: &[
            "a\t1\t61\t1\t-\t0\t0\t0\t-\t-",
            "b\t1\t6162\t2\t-\t0\t0\t2\te794b2,e4b999\t-,-",
            "apostrophe\t1\t616227\t3\t-\t0\t0\t0\t-\t-",
            "semicolon\t1\t6162273b\t4\t-\t0\t0\t0\t-\t-",
            "Page_Down\t0\t6162273b\t4\t-\t0\t0\t0\t-\t-",
        ],
    },
];

// ---------------------------------------------------------------- 金样装载

struct Step {
    repr: String,
    consumed: bool,
    input: String,
    caret: usize,
    commit: String,
    /// 参照 `RimeMenu::page_no`（0 基；参与比对）。
    page_no: usize,
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
                cases
                    .last_mut()
                    .expect("case record before step")
                    .steps
                    .push(step_from_fields(&fields));
            }
            other => panic!("unknown golden record: {other}"),
        }
    }
    cases
}

/// 金样 `step` 行 → [`Step`]（比对字段 + 计数语义：`count == 0` 时候选/注释为空）。
fn step_from_fields(fields: &[&str]) -> Step {
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
    Step {
        repr: fields[3].to_string(),
        consumed: fields[4] == "1",
        input: fields[5].to_string(),
        caret: fields[6].parse().expect("caret"),
        commit: fields[7].to_string(),
        page_no: fields[9].parse().expect("page_no"),
        highlight: fields[10].parse().expect("highlight"),
        count,
        candidates,
        comments,
    }
}

/// 登记表的紧凑期望行 → [`RowView`]（字段序与 [`step_from_fields`] 的比对字段一致）。
fn expected_row(raw: &str) -> RowView {
    let fields: Vec<&str> = raw.split('\t').collect();
    assert_eq!(
        fields.len(),
        10,
        "登记表期望行必须 10 列（repr/consumed/input/caret/commit/page_no/highlight/count/candidates/comments）：{raw:?}"
    );
    let count: usize = fields[7].parse().expect("count");
    let split = |value: &str| -> Vec<String> {
        if count == 0 {
            Vec::new()
        } else {
            value.split(',').map(str::to_string).collect()
        }
    };
    RowView {
        consumed: fields[1] == "1",
        input: fields[2].to_string(),
        caret: fields[3].parse().expect("caret"),
        commit: fields[4].to_string(),
        page_no: fields[5].parse().expect("page_no"),
        highlight: fields[6].parse().expect("highlight"),
        count,
        candidates: split(fields[8]),
        comments: split(fields[9]),
    }
}

// ---------------------------------------------------------------- 比对视图

/// 一步的比对视图（金样行 / 本仓期望行 / 实测行共用）。
#[derive(PartialEq, Eq, Debug)]
struct RowView {
    consumed: bool,
    input: String,
    caret: usize,
    commit: String,
    page_no: usize,
    highlight: usize,
    count: usize,
    candidates: Vec<String>,
    comments: Vec<String>,
}

impl RowView {
    fn of_golden(step: &Step) -> Self {
        Self {
            consumed: step.consumed,
            input: step.input.clone(),
            caret: step.caret,
            commit: step.commit.clone(),
            page_no: step.page_no,
            highlight: step.highlight,
            count: step.count,
            candidates: step.candidates.clone(),
            comments: step.comments.clone(),
        }
    }
}

/// 实测的一步（重放产物）。
struct Observed {
    repr: String,
    row: RowView,
}

/// 逐字段比对（失败消息与既有实现一致，另附期望/实测的紧凑行）。
fn compare(label: &str, expected: &RowView, observed: &RowView, failures: &mut Vec<String>) {
    if observed.consumed != expected.consumed {
        failures.push(format!(
            "{label}: consumed 期望 {} 实际 {}",
            expected.consumed, observed.consumed
        ));
    }
    if observed.input != expected.input {
        failures.push(format!(
            "{label}: input 期望 {} 实际 {}",
            expected.input, observed.input
        ));
    }
    if observed.caret != expected.caret {
        failures.push(format!(
            "{label}: caret 期望 {} 实际 {}",
            expected.caret, observed.caret
        ));
    }
    if observed.commit != expected.commit {
        failures.push(format!(
            "{label}: commit 期望 {} 实际 {}",
            expected.commit, observed.commit
        ));
    }
    if observed.page_no != expected.page_no {
        failures.push(format!(
            "{label}: page_no 期望 {} 实际 {}",
            expected.page_no, observed.page_no
        ));
    }
    if observed.highlight != expected.highlight {
        failures.push(format!(
            "{label}: highlight 期望 {} 实际 {}",
            expected.highlight, observed.highlight
        ));
    }
    if observed.count != expected.count {
        failures.push(format!(
            "{label}: candidate count 期望 {} 实际 {}",
            expected.count, observed.count
        ));
    }
    if observed.candidates != expected.candidates {
        failures.push(format!(
            "{label}: candidates 期望 {:?} 实际 {:?}",
            expected.candidates, observed.candidates
        ));
    }
    if observed.comments != expected.comments {
        failures.push(format!(
            "{label}: comments 期望 {:?} 实际 {:?}",
            expected.comments, observed.comments
        ));
    }
}

/// 两个视图是否逐字段相同（用于「确有差异」判定）。
fn rows_differ(left: &RowView, right: &RowView) -> bool {
    left != right
}

// ---------------------------------------------------------------- 登记表自校验

/// 登记项在本金样内的用例（不存在即 panic：登记名写错会静默失去守护）。
fn registered_case<'a>(cases: &'a [Case], deviation: &Deviation, golden: &str) -> &'a Case {
    assert_eq!(deviation.golden, golden, "登记项属于其它金样");
    let matches: Vec<&Case> = cases
        .iter()
        .filter(|case| case.name == deviation.case)
        .collect();
    assert_eq!(
        matches.len(),
        1,
        "{golden}: 登记用例 `{}` 必须唯一存在",
        deviation.case
    );
    matches[0]
}

/// 剔除登记在册的用例，返回（待重放, 登记项）。
///
/// 自校验：本金样中**实际跳过**的用例集合必须恰好等于登记表中属于本金样者 ——
/// 任何未登记却被跳过的用例都会让断言失败（不得静默跳过其它用例）。
fn split_cases<'a>(
    cases: &'a [Case],
    golden: &str,
) -> (Vec<&'a Case>, Vec<(&'a Deviation, &'a Case)>) {
    let mut kept = Vec::new();
    let mut deviated: Vec<(&Deviation, &Case)> = Vec::new();
    let mut skipped: Vec<&str> = Vec::new();
    for case in cases {
        match DEVIATIONS
            .iter()
            .find(|deviation| deviation.golden == golden && deviation.case == case.name)
        {
            Some(deviation) => {
                skipped.push(case.name.as_str());
                deviated.push((deviation, case));
            }
            None => kept.push(case),
        }
    }
    let mut expected: Vec<&str> = DEVIATIONS
        .iter()
        .filter(|deviation| deviation.golden == golden)
        .filter(|deviation| cases.iter().any(|case| case.name == deviation.case))
        .map(|deviation| deviation.case)
        .collect();
    expected.sort_unstable();
    skipped.sort_unstable();
    assert_eq!(
        skipped, expected,
        "{golden}: 实际跳过的用例与 DEVIATIONS 在本金样内的登记不一致\
         （不得跳过未登记的用例，也不得漏跳已登记的用例）"
    );
    for (deviation, case) in &deviated {
        assert_eq!(
            deviation.steps.len(),
            case.steps.len(),
            "{golden}: 登记用例 `{}` 的期望值步数（{}）与金样（{}）不一致",
            deviation.case,
            deviation.steps.len(),
            case.steps.len()
        );
    }
    (kept, deviated)
}

// ---------------------------------------------------------------- 重放

/// 重放一个用例；返回（逐步观测, [`store_ready`] 基线捕获次数）。
///
/// `store_ready` 对应参照的 `learned.store and learned.store.db`（金样夹具的
/// `tiger_sentence/tab_learning`）：`goldens/key_sequence.tsv.gz` 的夹具是 `false`，
/// `goldens/key_sequence_tab.tsv.gz` 的夹具是 `true`——**两者必须一起改**。
fn replay(
    case: &Case,
    data_dir: &Path,
    page_size: usize,
    lookup_key: Option<&str>,
    store_ready: bool,
) -> (Vec<Observed>, usize) {
    let dirs = [data_dir.to_path_buf()];
    let lexicon = Lexicon::load(&dirs, 0);
    let supplement = Supplement::load_default(Some(data_dir));
    let mut decoder = Decoder::new(lexicon, supplement, None);
    let mut context = Context::new();
    let mut state = SentenceState::fresh(1);
    let mut live = LiveLearning {
        store_ready,
        ..Default::default()
    };
    let mut baseline_captures = 0usize;
    let mut dot_armed = false;
    context.set_option("ascii_mode", false);
    context.set_option("_auto_commit", true);
    context.set_option(OPTION_EARLY_COMMIT, true);
    context.set_option(OPTION_EARLY_COMMIT_TO_PREEDIT, false);
    // **出厂缺省口径**：`tiger_sentence_digit_select` 在
    // `hux-cfg`/addon/`TigerScheme` 三处都是 `true`，差分层按同一口径重放
    // （addon 扩展，金样记录的是上游行为 ⇒ 数字直选用例登记在 `DEVIATIONS`）。
    context.set_option(OPTION_DIGIT_SELECT, true);
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
    let mut observed = Vec::with_capacity(case.steps.len());
    for step in case.steps.iter() {
        let key = KeyEvent::from_repr(&step.repr).expect("key repr");
        // 可证伪断言：夹具 `tab_learning: true`（⇒ `store_ready`）时，
        // 每次「未处于 tab_pending 的 Tab」都必须走参照的基线捕获分支并留下 baseline。
        let expects_baseline = store_ready
            && !state.tab_pending
            && matches!(step.repr.as_str(), "Tab" | "ISO_Left_Tab" | "Shift+Tab");
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
        if expects_baseline {
            assert!(
                live.baseline.is_some(),
                "{}[{}] {}: store_ready 时首次 Tab 必须捕获学习基线（参照 \
                 `if not state.tab_pending and learned.store and learned.store.db`）",
                case.name,
                observed.len(),
                step.repr
            );
            baseline_captures += 1;
        }
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
        let input = context.input().to_vec();
        let segment = context.composition.back();
        // 参照 `RimeGetContext`：按当前页上报候选、页码与页内高亮（夹具 `menu/page_size`）。
        let highlight = segment
            .map(|segment| segment.selected_index % page_size)
            .unwrap_or(0);
        // `menu.page_no` 同样按选中项所在页上报；无段（无菜单）时为 0
        // （参照的 `RimeMenu` 零初始化）。
        let page_no = segment
            .map(|segment| segment.selected_index / page_size)
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
        let candidates: Vec<String> = page
            .iter()
            .map(|candidate| hex(candidate.text.as_bytes()))
            .collect();
        let comments: Vec<String> = page
            .iter()
            .map(|candidate| hex(candidate.comment.as_bytes()))
            .collect();
        observed.push(Observed {
            repr: step.repr.clone(),
            row: RowView {
                consumed,
                input: hex(&input),
                caret: context.caret(),
                commit: hex(committed.as_bytes()),
                page_no,
                highlight,
                count: candidates.len(),
                candidates,
                comments,
            },
        });
    }
    (observed, baseline_captures)
}

/// 非登记用例：与金样逐位比对。
fn compare_with_golden(
    case: &Case,
    data_dir: &Path,
    page_size: usize,
    lookup_key: Option<&str>,
    store_ready: bool,
    failures: &mut Vec<String>,
) -> usize {
    let (observed, baseline_captures) = replay(case, data_dir, page_size, lookup_key, store_ready);
    assert_eq!(
        observed.len(),
        case.steps.len(),
        "{}: 重放步数与金样不一致",
        case.name
    );
    for (index, (step, actual)) in case.steps.iter().zip(&observed).enumerate() {
        let label = format!("{}[{}] {}", case.name, index, actual.repr);
        compare(&label, &RowView::of_golden(step), &actual.row, failures);
    }
    baseline_captures
}

/// 登记用例：逐位断言**本仓期望值**，并要求「与金样的差异集合」实测与登记一致且非空。
fn compare_with_registered_expectations(
    deviation: &Deviation,
    case: &Case,
    data_dir: &Path,
    page_size: usize,
    lookup_key: Option<&str>,
    store_ready: bool,
    failures: &mut Vec<String>,
) -> usize {
    let (observed, baseline_captures) = replay(case, data_dir, page_size, lookup_key, store_ready);
    assert_eq!(
        observed.len(),
        case.steps.len(),
        "{}: 重放步数与金样不一致",
        deviation.case
    );
    let mut differing_registered = 0usize;
    let mut differing_observed = 0usize;
    for (index, ((raw, step), actual)) in deviation
        .steps
        .iter()
        .zip(&case.steps)
        .zip(&observed)
        .enumerate()
    {
        let label = format!("{}[{}] {}", deviation.case, index, actual.repr);
        let expected = expected_row(raw);
        let golden = RowView::of_golden(step);
        // ① 本仓期望值：逐位断言。
        compare(&label, &expected, &actual.row, failures);
        // ② 偏离形状：登记表与实测「相对金样有差异的步」集合必须一致。
        if rows_differ(&expected, &golden) {
            differing_registered += 1;
        }
        if rows_differ(&actual.row, &golden) {
            differing_observed += 1;
        }
        if rows_differ(&expected, &golden) && !rows_differ(&actual.row, &golden) {
            failures.push(format!(
                "{label}: 登记的偏离（{:?}）已消失 —— 实测与上游金样一致，\
                 实现可能已回退成上游行为；期望 {} 实测 {}",
                deviation.kind,
                describe(&expected),
                describe(&actual.row)
            ));
        }
        if !rows_differ(&expected, &golden) && rows_differ(&actual.row, &golden) {
            failures.push(format!(
                "{label}: 实测与上游金样存在**未登记**的差异（登记种类 {:?}）：实测 {} 金样 {}",
                deviation.kind,
                describe(&actual.row),
                describe(&golden)
            ));
        }
    }
    if differing_registered == 0 {
        failures.push(format!(
            "{}: 登记表里该用例（{:?}）的期望值与上游金样**完全相同**（并不偏离）—— \
             登记一个不偏离的用例会静默取消其全部步的比对；请从 DEVIATIONS 删除",
            deviation.case, deviation.kind
        ));
    }
    if differing_registered != differing_observed {
        failures.push(format!(
            "{}: 与金样有差异的步数 实测 {} != 登记 {}（{:?}）",
            deviation.case, differing_observed, differing_registered, deviation.kind
        ));
    }
    baseline_captures
}

/// 紧凑行（失败消息用；与登记表期望行同格式）。
fn describe(row: &RowView) -> String {
    let join = |values: &[String]| -> String {
        if values.is_empty() {
            "-".to_string()
        } else {
            values.join(",")
        }
    };
    format!(
        "consumed={} input={} caret={} commit={} highlight={} count={} candidates={} comments={}",
        row.consumed as u8,
        row.input,
        row.caret,
        row.commit,
        row.highlight,
        row.count,
        join(&row.candidates),
        join(&row.comments)
    )
}

// ---------------------------------------------------------------- 用例

#[test]
fn key_sequence_matches_reference() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..");
    let cases = load_cases(&root.join("goldens/key_sequence.tsv.gz"));
    // 覆盖下限：金样被截断 / 少解析若干 case 时必须失败，不得静默变少。
    let golden_steps: usize = cases.iter().map(|case| case.steps.len()).sum();
    assert!(
        cases.len() >= 68,
        "key_sequence 金样用例数不足：{} < 68（金样被截断？）",
        cases.len()
    );
    assert!(
        golden_steps >= 285,
        "key_sequence 金样步数不足：{golden_steps} < 285（金样被截断？）"
    );
    // 上游金样原样保留：其中「上游缺陷」用例见 DEVIATIONS（登记 + 本仓期望值，不重生成）。
    let (kept, deviated) = split_cases(&cases, "key_sequence.tsv.gz");
    let data_dir = root.join("goldens/key_sequence");
    let mut failures = Vec::new();
    let mut steps = 0usize;
    for case in &kept {
        steps += case.steps.len();
        compare_with_golden(
            case,
            &data_dir,
            hux_core::host::DEFAULT_PAGE_SIZE,
            None,
            // 夹具 `tab_learning: false` ⇒ 参照学习库不就绪（同探针口径）。
            false,
            &mut failures,
        );
    }
    for (deviation, case) in &deviated {
        compare_with_registered_expectations(
            deviation,
            case,
            &data_dir,
            hux_core::host::DEFAULT_PAGE_SIZE,
            None,
            false,
            &mut failures,
        );
    }
    // 重放面下限（同上）：非登记用例与步数不得变少（登记项由 `DEVIATIONS` 单独钉住）。
    // 68 例 / 285 步 − 6 个登记项（`punct_menu_equal` 3 + `punct_menu_minus` 3 +
    // `nav_page_home_minus` 4 + `digit_menu_select` 3 + `apostrophe_*_page` 各 5）= 62 例 / 262 步。
    assert!(
        kept.len() >= 62 && steps >= 262,
        "key_sequence 重放覆盖不足：{} 例 / {} 步（下限 62 例 / 262 步）",
        kept.len(),
        steps
    );
    assert!(
        failures.is_empty(),
        "键序列不一致 {} 处（{} 例 / {} 步，另 {} 例登记偏离）：\n{}",
        failures.len(),
        kept.len(),
        steps,
        deviated.len(),
        failures.join("\n")
    );
}

/// Tab 锁路径的**真机探针**金样。
///
/// 主金样 `key_sequence.tsv.gz` 的夹具写 `tiger_sentence/tab_learning: false` ⇒ 参照的
/// `learned.store` 为 nil，处理器里 `if not state.tab_pending and learned.store and
/// learned.store.db then … learned.baseline = first end` 这条分支**永不进入**；
/// 本夹具（`goldens/key_sequence_tab/tiger_sentence.custom.yaml`）写 `true` ⇒ 学习库就绪，
/// 分支进入。重放侧 `store_ready = true` 与之对应（两者必须一起改）。
///
/// 断言：①逐位比对真机记录；②**基线捕获确实发生**（`captures == 8`，即 8 个用例各一次
/// 首次 Tab）——把 `store_ready` 写回 false（或处理器不再捕获基线）即失败；
/// ③夹具条件本身入库（`custom.yaml`）且被本用例复核，防止「金样说 true、重放说 false」
/// 这类**两边一致地错**的漂移。
///
/// 实测事实（记录以免误读覆盖面）：本夹具的这些用例在 `tab_learning: true/false` 下的
/// **比对字段逐位相同**（Tab 的可见行为不因学习库就绪而变；真机探针两侧各跑一次实测），
/// 故本金样的增量是「参照分支真的走进去了 + 重放侧真的接上了 `learning_selection`」，
/// 而不是新的可见行为。
#[test]
fn key_sequence_tab_matches_reference() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..");
    let data_dir = root.join("goldens/key_sequence_tab");
    // 夹具条件与重放口径必须一起改（`tools/generators/gen_key_sequence_tab_golden.sh` 同断言）。
    let fixture = std::fs::read_to_string(data_dir.join("tiger_sentence.custom.yaml"))
        .expect("读取 Tab 夹具 custom.yaml");
    assert!(
        fixture.contains("tiger_sentence/tab_learning: true")
            && !fixture.contains("tab_learning: false"),
        "Tab 金样夹具必须开启 tab_learning（否则本金样失去意义）"
    );
    let cases = load_cases(&root.join("goldens/key_sequence_tab.tsv.gz"));
    let golden_steps: usize = cases.iter().map(|case| case.steps.len()).sum();
    assert!(
        cases.len() >= 8,
        "key_sequence_tab 金样用例数不足：{} < 8（金样被截断？）",
        cases.len()
    );
    assert!(
        golden_steps >= 44,
        "key_sequence_tab 金样步数不足：{golden_steps} < 44（金样被截断？）"
    );
    let (kept, deviated) = split_cases(&cases, "key_sequence_tab.tsv.gz");
    assert!(
        deviated.is_empty(),
        "Tab 金样是上游行为原样记录，不应有偏离登记"
    );
    let mut failures = Vec::new();
    let mut captures = 0usize;
    for case in &kept {
        captures += compare_with_golden(
            case,
            &data_dir,
            hux_core::host::DEFAULT_PAGE_SIZE,
            None,
            true,
            &mut failures,
        );
    }
    assert!(
        failures.is_empty(),
        "Tab 键序列不一致 {} 处（{} 例）：\n{}",
        failures.len(),
        kept.len(),
        failures.join("\n")
    );
    assert_eq!(
        captures, 8,
        "夹具 tab_learning: true ⇒ 每个用例首次 Tab 都必须捕获学习基线（共 8 例）；\
         captures=0 说明重放侧没接上 store_ready，等价于回到「Tab 路径只有 processor 级用例」"
    );
}

/// 音反查金样：夹具页大小 5（与引擎一致，翻页用例覆盖后续页）；音反查前缀 `` ` ``。
#[test]
fn sound_to_char_shape_matches_reference() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..");
    let cases = load_cases(&root.join("goldens/sound_to_char_shape.tsv.gz"));
    // 覆盖下限：同上。
    let golden_steps: usize = cases.iter().map(|case| case.steps.len()).sum();
    assert!(
        cases.len() >= 31,
        "sound_to_char_shape 金样用例数不足：{} < 31（金样被截断？）",
        cases.len()
    );
    assert!(
        golden_steps >= 164,
        "sound_to_char_shape 金样步数不足：{golden_steps} < 164（金样被截断？）"
    );
    // 同上：`nav-page-*` 记录的是上游「`=`/`-` 被标点分支遮蔽」的行为，见 DEVIATIONS。
    let (kept, deviated) = split_cases(&cases, "sound_to_char_shape.tsv.gz");
    let data_dir = root.join("goldens/sound_to_char_shape");
    let mut failures = Vec::new();
    let mut steps = 0usize;
    for case in &kept {
        steps += case.steps.len();
        compare_with_golden(
            case,
            &data_dir,
            hux_core::host::DEFAULT_PAGE_SIZE,
            Some("grave"),
            // 音反查夹具同样 `tab_learning: false`（与主方案夹具同源）。
            false,
            &mut failures,
        );
    }
    for (deviation, case) in &deviated {
        compare_with_registered_expectations(
            deviation,
            case,
            &data_dir,
            hux_core::host::DEFAULT_PAGE_SIZE,
            Some("grave"),
            false,
            &mut failures,
        );
    }
    // 重放面下限（同上）：28 例 / 149 步（登记偏离 3 例 15 步另计）。
    assert!(
        kept.len() >= 28 && steps >= 149,
        "sound_to_char_shape 重放覆盖不足：{} 例 / {} 步（下限 28 例 / 149 步）",
        kept.len(),
        steps
    );
    assert!(
        failures.is_empty(),
        "音反查不一致 {} 处（{} 例 / {} 步，另 {} 例登记偏离）：\n{}",
        failures.len(),
        kept.len(),
        steps,
        deviated.len(),
        failures.join("\n")
    );
}

/// 登记表自校验（无需重放）：每个登记名必须在**指定金样**中真实存在且唯一；
/// 期望值步数与金样步数一致；登记项**确有差异**（否则报「偏离已消失/未登记」）；
/// 同一用例不得同时出现在两份金样。
#[test]
fn registry_is_falsifiable() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..");
    let goldens = ["key_sequence.tsv.gz", "sound_to_char_shape.tsv.gz"];
    let loaded: Vec<Vec<Case>> = goldens
        .iter()
        .map(|golden| load_cases(&root.join("goldens").join(golden)))
        .collect();
    let mut seen: Vec<(&str, &str)> = Vec::new();
    for deviation in DEVIATIONS {
        assert!(
            goldens.contains(&deviation.golden),
            "未知金样：{}",
            deviation.golden
        );
        assert!(
            !seen.iter().any(|(_, case)| *case == deviation.case),
            "登录用例名 `{}` 重名，登记表无法唯一定位",
            deviation.case
        );
        seen.push((deviation.golden, deviation.case));
        let cases = &loaded[goldens
            .iter()
            .position(|golden| *golden == deviation.golden)
            .expect("金样下标")];
        let case = registered_case(cases, deviation, deviation.golden);
        assert_eq!(
            deviation.steps.len(),
            case.steps.len(),
            "{}: 登记期望值步数与金样不一致",
            deviation.case
        );
        let differing = deviation
            .steps
            .iter()
            .zip(&case.steps)
            .filter(|(raw, step)| rows_differ(&expected_row(raw), &RowView::of_golden(step)))
            .count();
        assert!(
            differing > 0,
            "{}: 登记表期望值与上游金样完全相同（并不偏离）—— \
             把「名字存在但不偏离」的用例写进登记表即可静默取消其全部步的比对，故必须失败",
            deviation.case
        );
    }
}

/// 维护工具（`#[ignore]`，不随 `cargo test` 运行）：打印用例的实测行
/// （与 [`Deviation::steps`] 同格式的 10 列，可直接粘进 `DEVIATIONS`）。
///
/// - 缺省打印 `DEVIATIONS` 里全部登记用例（复核/更新期望值时用）；
/// - 另可用 `HUX_DUMP_CASES="key_sequence.tsv.gz/case-a,sound_to_char_shape.tsv.gz/case-b"`
///   打印任意用例（**新增偏离项**时先跑它，再核对「期望 ≠ 金样」的步集合）。
/// - 与 `DEVIATIONS` 同口径重放（出厂缺省 + 金样夹具），故输出即当前实现的行为。
#[test]
#[ignore]
fn dump_registered_expectations() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..");
    let mut targets: Vec<(String, String)> = DEVIATIONS
        .iter()
        .map(|deviation| (deviation.golden.to_string(), deviation.case.to_string()))
        .collect();
    if let Ok(extra) = std::env::var("HUX_DUMP_CASES") {
        targets.extend(
            extra
                .split(',')
                .filter(|item| !item.is_empty())
                .map(|item| {
                    let (golden, case) = item
                        .split_once('/')
                        .expect("HUX_DUMP_CASES 形如 <金样>/<用例>");
                    (golden.to_string(), case.to_string())
                }),
        );
    }
    for (golden, case_name) in targets {
        let cases = load_cases(&root.join("goldens").join(&golden));
        let case = cases
            .iter()
            .find(|case| case.name == case_name)
            .expect("case");
        let (data_dir, lookup) = if golden == "key_sequence.tsv.gz" {
            (root.join("goldens/key_sequence"), None)
        } else {
            (root.join("goldens/sound_to_char_shape"), Some("grave"))
        };
        let (observed, _) = replay(
            case,
            &data_dir,
            hux_core::host::DEFAULT_PAGE_SIZE,
            lookup,
            false,
        );
        println!("--- {golden} / {case_name}");
        for (step, actual) in case.steps.iter().zip(&observed) {
            let row = &actual.row;
            let join = |values: &[String]| -> String {
                if values.is_empty() {
                    "-".to_string()
                } else {
                    values.join(",")
                }
            };
            let golden_row = RowView::of_golden(step);
            println!(
                "        \"{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\",{}",
                actual.repr,
                row.consumed as u8,
                row.input,
                row.caret,
                row.commit,
                row.page_no,
                row.highlight,
                row.count,
                join(&row.candidates),
                join(&row.comments),
                if rows_differ(row, &golden_row) {
                    "  // 偏离金样"
                } else {
                    ""
                }
            );
        }
    }
}
