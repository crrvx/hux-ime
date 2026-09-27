// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! 偏离登记表：金样中的上游行为与本仓既有差异的**期望值表**。

// ---------------------------------------------------------------- 偏离登记表

/// 偏离的**种类**（决定差异来源、影响面与回归做法）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DeviationKind {
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
    ///
    /// **同一 pin 缺口的音反查面**：`sound_to_char_shape.tsv.gz` 的三个 `apostrophe-*` 与之同源——
    /// 方案 schema 要求 `'` 在反查段内作音节分隔符，而本机 librime 1.17.0 未含上游 delimiter 修复
    /// （[rime/librime#1233](https://github.com/rime/librime/pull/1233)）⇒ 探针在 `'` 之后一律无候选；
    /// 本仓按方案意图切分（`'` 透明跳过、强制断音）⇒ 三例的逐步记录确有差异。
    BranchPinDelimiter,
}

/// 一个登记项：金样中一个与本仓行为**确有差异**的用例 + 本仓的逐步期望值。
pub struct Deviation {
    /// 金样文件名。
    pub golden: &'static str,
    /// 用例名（必须在该金样中真实存在且唯一）。
    pub case: &'static str,
    pub kind: DeviationKind,
    /// 本仓期望的逐步记录：与金样 `step` 行同字段，但去掉 `step`/用例名/序号/`preedit`
    /// 四列（`preedit` 属宿主层职责，金样保留、本表不比对），依次为
    /// `repr`、`consumed`、`input`、`caret`、`commit`、`page_no`、`highlight`、
    /// `count`、`candidates`、`comments`。`repr` 只作可读性标注、不参与比对
    /// （`RowView` 只含其后 9 列）；`input`/`commit`/`candidates`/`comments` 为 hex，
    /// 空串写 `-`（`count == 0` 时候选与注释同为 `-`）。
    pub steps: &'static [&'static str],
}

use DeviationKind::{AddonExtension, BranchPinDelimiter, UpstreamDefectFix};

/// 登记表（金样字节保持原样，不重生成）。**每一项都必须确有差异**，否则
/// `deviated_cases_match_their_registered_expectations` 会报「偏离已消失」。
pub const DEVIATIONS: &[Deviation] = &[
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
    // ---- pin 缺口：音反查段内的音节分隔符（撇号，见 DeviationKind::BranchPinDelimiter）----
    //
    // 夹具索引已含 `xi`/`an`，故 `x`/`i` 两步两边都有候选（逐字段相同）；差异从 `'` 起：
    // 上游探针（librime 1.17.0 未含 delimiter 修复）在 `'` 之后一律无候选，本仓按方案
    // schema 的 `speller/delimiter: " '"` 切分（`'` 透明跳过、强制断音）⇒ 候选保留到 `xi'`，
    // `` `xi'an `` 出词「西安」（`a`/`n` 逐步补全，`space` 上屏）。
    Deviation {
        golden: "sound_to_char_shape.tsv.gz",
        case: "apostrophe-tail",
        kind: BranchPinDelimiter,
        steps: &[
            "`\t1\t60\t1\t-\t0\t0\t1\t60\te38094e58d8ae8a792e38095",
            "x\t1\t6078\t2\t-\t0\t0\t2\te8a5bf,e7b3bb\t-,-",
            "i\t1\t607869\t3\t-\t0\t0\t2\te8a5bf,e7b3bb\t-,-",
            "apostrophe\t1\t60786927\t4\t-\t0\t0\t2\te8a5bf,e7b3bb\t-,-",
        ],
    },
    Deviation {
        golden: "sound_to_char_shape.tsv.gz",
        case: "apostrophe-inner",
        kind: BranchPinDelimiter,
        // `` ` xi' an ``：`'` 断音后 `a`/`n` 逐步补全第二段 ⇒ 候选收敛到「西安」。
        steps: &[
            "`\t1\t60\t1\t-\t0\t0\t1\t60\te38094e58d8ae8a792e38095",
            "x\t1\t6078\t2\t-\t0\t0\t2\te8a5bf,e7b3bb\t-,-",
            "i\t1\t607869\t3\t-\t0\t0\t2\te8a5bf,e7b3bb\t-,-",
            "apostrophe\t1\t60786927\t4\t-\t0\t0\t2\te8a5bf,e7b3bb\t-,-",
            "a\t1\t6078692761\t5\t-\t0\t0\t1\te8a5bfe5ae89\t20e8a5bf3a3f20e5ae893a3f",
            "n\t1\t60786927616e\t6\t-\t0\t0\t1\te8a5bfe5ae89\t20e8a5bf3a3f20e5ae893a3f",
        ],
    },
    Deviation {
        golden: "sound_to_char_shape.tsv.gz",
        case: "apostrophe-commit",
        kind: BranchPinDelimiter,
        // 同上再加 `space`：本仓上屏「西安」（`commit` 非空、输入清空）；上游无候选，
        // `space` 落成空格标点（金样该步 `commit` 为空）。
        steps: &[
            "`\t1\t60\t1\t-\t0\t0\t1\t60\te38094e58d8ae8a792e38095",
            "x\t1\t6078\t2\t-\t0\t0\t2\te8a5bf,e7b3bb\t-,-",
            "i\t1\t607869\t3\t-\t0\t0\t2\te8a5bf,e7b3bb\t-,-",
            "apostrophe\t1\t60786927\t4\t-\t0\t0\t2\te8a5bf,e7b3bb\t-,-",
            "a\t1\t6078692761\t5\t-\t0\t0\t1\te8a5bfe5ae89\t20e8a5bf3a3f20e5ae893a3f",
            "n\t1\t60786927616e\t6\t-\t0\t0\t1\te8a5bfe5ae89\t20e8a5bf3a3f20e5ae893a3f",
            "space\t1\t-\t0\te8a5bfe5ae89\t0\t0\t0\t-\t-",
        ],
    },
];
