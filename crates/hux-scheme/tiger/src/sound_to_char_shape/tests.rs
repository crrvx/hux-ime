// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! 索引格式、图翻译与标点注释的单元测试（原名内联于门面文件）。

use super::index::{MAGIC, TYPE_ABBREV, TYPE_NORMAL};
use super::*;
use crate::lexicon::Lexicon;

fn fixture_index() -> SoundToCharShapeIndex {
    let path = hux_test_support::repo_path("goldens/sound_to_char_shape/tiger_sentence.pinyin.bin");
    SoundToCharShapeIndex::load(&path).expect("fixture index")
}

#[test]
fn pattern_matches_reference_recognizer() {
    // 参照 `tiger_sentence.schema.yaml` @92a0b54：`^`[a-z']*$`；
    // 上游 `tools/test_reverse_lookup.lua` 同一批断言。
    assert!(matches_pattern(b"`", '`'));
    assert!(matches_pattern(b"`zhong", '`'));
    assert!(matches_pattern(b"`xi'", '`'));
    assert!(matches_pattern(b"`xi'a", '`'));
    assert!(matches_pattern(b"`xi'an", '`'));
    assert!(matches_pattern(b"`xi'an'", '`'));
    assert!(!matches_pattern(b"`Z", '`'));
    assert!(!matches_pattern(b"a`", '`'));
    assert!(!matches_pattern(b"``", '`'));
    assert!(!matches_pattern(b"`1", '`'));
    assert!(!matches_pattern(b"`ni2", '`'));
    assert!(!matches_pattern(b"`xi'an2", '`'));
    assert!(!matches_pattern(b"`xi a", '`'));
    assert!(!matches_pattern(b"xi'an", '`'));
}

#[test]
fn punct_shape_comments_match_reference() {
    assert_eq!(punct_shape_comment("`"), "〔半角〕");
    assert_eq!(punct_shape_comment("｀"), "〔全角〕");
    assert_eq!(punct_shape_comment(""), "");
    assert_eq!(punct_shape_comment("ab"), "");
}

#[test]
fn fixture_index_reports_counts() {
    let index = fixture_index();
    assert_eq!(index.entry_count(), 27);
    assert_eq!(index.spellings.len(), 22);
}

#[test]
fn translate_matches_abbrev_and_pruning_candidates() {
    let index = fixture_index();
    let lexicon = Lexicon::load(&[], 0);
    let texts = |input: &[u8]| -> Vec<String> {
        translate(
            &index,
            &lexicon,
            input,
            '`',
            0,
            input.len(),
            None,
            &mut PairState::default(),
            false,
            crate::decode::CANDIDATE_LIMIT,
        )
        .into_iter()
        .map(|candidate| candidate.text)
        .collect()
    };
    // 缩写路径（「zh」+「o」）与剪枝（zhou 全拼可达 → 缩写全弃）。
    assert_eq!(
        texts(b"`zho"),
        ["中哦", "中龘", "中欧", "找哦", "兆欧", "找欧"]
    );
    assert_eq!(texts(b"`zhou"), ["周", "轴"]);
    assert_eq!(texts(b"`zhong"), ["中", "重", "种", "钟", "垚"]);
    assert!(texts(b"`zhon").is_empty());
    assert!(texts(b"`zuo").is_empty());
}

#[test]
fn translate_segments_preedit_by_syllable() {
    let index = fixture_index();
    let lexicon = Lexicon::load(&[], 0);
    let preedits = |input: &[u8]| -> Vec<String> {
        translate(
            &index,
            &lexicon,
            input,
            '`',
            0,
            input.len(),
            None,
            &mut PairState::default(),
            false,
            crate::decode::CANDIDATE_LIMIT,
        )
        .into_iter()
        .map(|candidate| candidate.preedit)
        .collect()
    };
    // 预编辑「按音节分码」：全拼段之后插空格；缩写段与后续合并。
    assert_eq!(preedits(b"`zhong")[0], "`zhong");
    assert_eq!(preedits(b"`zhongguo")[0], "`zhong guo");
    assert_eq!(preedits(b"`zhongg")[0], "`zhong g");
    assert_eq!(preedits(b"`zho")[0], "`zho");
}

/// 分隔符用例的合成索引（字段序同 [`SoundToCharShapeIndex::parse`]，借用下面的
/// [`IndexBuilder`]）：`xi` / `xian` / `xi an` 三条路径互相竞争 —— 金样夹具的 14 个音节里
/// 既无 `xi` 也无 `an`，表达不出「撇号强制断音」与「退化成单音节」的区别。
fn delimiter_index() -> SoundToCharShapeIndex {
    let builder = IndexBuilder {
        syllables: vec![
            "an".to_string(),
            "guo".to_string(),
            "xi".to_string(),
            "xian".to_string(),
            "zhong".to_string(),
        ],
        spellings: vec![
            (b"an".to_vec(), vec![(0, TYPE_NORMAL)]),
            (b"guo".to_vec(), vec![(1, TYPE_NORMAL)]),
            // `x` 是 `xi`/`xian` 的缩写键（两条 `abbrev` 规则的效果）。
            (b"x".to_vec(), vec![(2, TYPE_ABBREV), (3, TYPE_ABBREV)]),
            (b"xi".to_vec(), vec![(2, TYPE_NORMAL)]),
            (b"xian".to_vec(), vec![(3, TYPE_NORMAL)]),
            (b"zhong".to_vec(), vec![(4, TYPE_NORMAL)]),
        ],
        groups: vec![
            (vec![0], 1),
            (vec![1], 1),
            (vec![2], 1),
            (vec![2, 0], 1),
            (vec![3], 1),
            (vec![4], 1),
            (vec![4, 1], 1),
        ],
        entries: vec![
            (100, "安".to_string()),
            (100, "国".to_string()),
            (200, "西".to_string()),
            (300, "西安".to_string()),
            (500, "先".to_string()),
            (1000, "中".to_string()),
            (900, "中国".to_string()),
        ],
    };
    SoundToCharShapeIndex::parse(&builder.bytes()).expect("delimiter index")
}

/// 跑一次音反查（测试用固定前缀与上屏区间）。
fn reverse_lookup(index: &SoundToCharShapeIndex, input: &[u8]) -> Vec<Candidate> {
    translate(
        index,
        &Lexicon::load(&[], 0),
        input,
        '`',
        0,
        input.len(),
        None,
        &mut PairState::default(),
        false,
        crate::decode::CANDIDATE_LIMIT,
    )
}

fn candidate_texts(index: &SoundToCharShapeIndex, input: &[u8]) -> Vec<String> {
    reverse_lookup(index, input)
        .into_iter()
        .map(|candidate| candidate.text)
        .collect()
}

fn candidate_preedits(index: &SoundToCharShapeIndex, input: &[u8]) -> Vec<String> {
    reverse_lookup(index, input)
        .into_iter()
        .map(|candidate| candidate.preedit)
        .collect()
}

/// 音节分隔符在匹配拼写键时透明跳过，但**强制**断音：音节与尾部补全都不得跨段。
#[test]
fn translate_honors_syllable_delimiter() {
    let index = delimiter_index();
    // 无分隔符：`xian` 同时可达 [xian]（先）与 [xi][an]（西安）。
    assert_eq!(candidate_texts(&index, b"`xian"), ["先", "西安"]);
    // 强制断音：只剩 [xi][an]（西安），跨段的 [xian]（先）被剔除。
    assert_eq!(candidate_texts(&index, b"`xi'an"), ["西安"]);
    // 末尾分隔符等价于无分隔符；首部、连续分隔符等价于单个分隔符。
    assert_eq!(candidate_texts(&index, b"`xi"), ["西"]);
    assert_eq!(
        candidate_texts(&index, b"`xi'"),
        candidate_texts(&index, b"`xi")
    );
    assert_eq!(
        candidate_texts(&index, b"`'xi'an"),
        candidate_texts(&index, b"`xi'an")
    );
    assert_eq!(
        candidate_texts(&index, b"`xi'an'"),
        candidate_texts(&index, b"`xi'an")
    );
    assert_eq!(
        candidate_texts(&index, b"`xi''an"),
        candidate_texts(&index, b"`xi'an")
    );
    // 尾部补全不得跨段：`xia'n` 的 `an` 跨过末尾分隔符 ⇒ 无候选
    //（补全若跨段，[xi] + 补全 `an` 就会错出「西安」）。
    assert!(candidate_texts(&index, b"`xia'n").is_empty());
    // 某段拼不出音节 ⇒ 整段无候选（不报错、不 panic）。
    assert!(candidate_texts(&index, b"`xi'qan").is_empty());
    assert!(candidate_texts(&index, b"`zhq'guo").is_empty());
    // 裸分隔符（无音节）同样只是无候选。
    assert!(candidate_texts(&index, b"`'").is_empty());
    assert!(candidate_texts(&index, b"`''").is_empty());
}

/// 分隔符在预编辑里**原样保留为撇号**（与输入同形），即便前一音节是缩写/补全匹配；
/// 只有音节边界才插空格。
#[test]
fn translate_keeps_delimiter_in_preedit() {
    let index = delimiter_index();
    // 全拼 + 全拼：`xi'an` → [xi][an]，预编辑与输入同形。
    assert_eq!(candidate_preedits(&index, b"`xi'an")[0], "`xi'an");
    // 无分隔符时 `xian` 是一个音节，预编辑同样与输入同形（没有可插空格的边界）。
    assert_eq!(candidate_preedits(&index, b"`xian")[0], "`xian");
    let index = fixture_index();
    // 缩写 + 全拼：`zh'guo` → [zh][guo]（中国），分隔符保留。
    assert_eq!(candidate_texts(&index, b"`zh'guo"), ["中国"]);
    assert_eq!(candidate_preedits(&index, b"`zh'guo")[0], "`zh'guo");
    // 对照：音节边界（无分隔符）仍是空格。
    assert_eq!(candidate_preedits(&index, b"`zhongguo")[0], "`zhong guo");
    // 分隔符透明：`zhong'g` 与 `zhongg` 同候选，但预编辑各自保留分隔符 / 走空格规则。
    assert_eq!(
        candidate_texts(&index, b"`zhong'g"),
        candidate_texts(&index, b"`zhongg")
    );
    assert_eq!(candidate_preedits(&index, b"`zhong'g")[0], "`zhong'g");
    assert_eq!(candidate_preedits(&index, b"`zhongg")[0], "`zhong g");
    // 段首 / 段尾的分隔符同样原样可见（掩码两侧都记）。
    let index = delimiter_index();
    assert_eq!(candidate_preedits(&index, b"`'xi'an")[0], "`'xi'an");
    assert_eq!(candidate_preedits(&index, b"`xi'an'")[0], "`xi'an'");
    // 连续分隔符落在同一界上 ⇒ 预编辑里只出现一个。
    assert_eq!(candidate_preedits(&index, b"`xi''an")[0], "`xi'an");
    // 段尾分隔符：全拼未完成也当场可见（夹具里的 `zh` 是 `zhong` 的缩写）。
    let index = fixture_index();
    assert_eq!(candidate_preedits(&index, b"`zh'")[0], "`zh'");
}

// ------------------------------------------------------------ 畸形索引加固
//
// 索引从**用户目录优先**的数据目录懒加载（`decode.rs`），畸形/被替换的
// `tiger_sentence.pinyin.bin[.gz]` 必须返回诊断而非 panic（addon 内 panic
// 跨 FFI 即进程终止）。以下用例逐一构造畸形字节。

/// 一个拼写条目：`(拼写字节, [(音节下标, 该音节内的拼写键下标)])`。
type SpellingEntry = (Vec<u8>, Vec<(u32, u8)>);

/// TCSRV01 构造器（字段序与 [`SoundToCharShapeIndex::parse`] 一致）。
#[derive(Default)]
struct IndexBuilder {
    syllables: Vec<String>,
    spellings: Vec<SpellingEntry>,
    groups: Vec<(Vec<u16>, u32)>,
    entries: Vec<(u32, String)>,
}

impl IndexBuilder {
    fn push16(out: &mut Vec<u8>, value: u16) {
        out.extend_from_slice(&value.to_le_bytes());
    }

    fn push32(out: &mut Vec<u8>, value: u32) {
        out.extend_from_slice(&value.to_le_bytes());
    }

    /// 序列化；`header` 覆盖四个计数（构造畸形头部用）。
    fn bytes_with(&self, header: [u32; 4]) -> Vec<u8> {
        let mut out = MAGIC.to_vec();
        for value in header {
            Self::push32(&mut out, value);
        }
        for syllable in &self.syllables {
            Self::push16(&mut out, syllable.len() as u16);
            out.extend_from_slice(syllable.as_bytes());
        }
        for (key, alts) in &self.spellings {
            Self::push16(&mut out, key.len() as u16);
            out.extend_from_slice(key);
            out.push(alts.len() as u8);
            for &(syllable, kind) in alts {
                Self::push32(&mut out, syllable);
                out.push(kind);
            }
        }
        for (code, entries) in &self.groups {
            out.push(code.len() as u8);
            for &id in code {
                Self::push16(&mut out, id);
            }
            Self::push32(&mut out, *entries);
        }
        for (weight, text) in &self.entries {
            Self::push32(&mut out, *weight);
            Self::push16(&mut out, text.len() as u16);
            out.extend_from_slice(text.as_bytes());
        }
        out
    }

    fn bytes(&self) -> Vec<u8> {
        self.bytes_with([
            self.syllables.len() as u32,
            self.spellings.len() as u32,
            self.groups.len() as u32,
            self.entries.len() as u32,
        ])
    }
}

/// `parse` 的失败路径（`SoundToCharShapeIndex` 未派生 `Debug`，不用 `expect_err`）。
fn parse_error(bytes: &[u8]) -> anyhow::Error {
    match SoundToCharShapeIndex::parse(bytes) {
        Ok(_) => panic!("畸形索引不应解析成功"),
        Err(error) => error,
    }
}

/// 正对照：合法字节流仍照常解析（校验不得误伤正常索引）。
#[test]
fn parse_accepts_well_formed_bytes() {
    let builder = IndexBuilder {
        syllables: vec!["zhong".to_string(), "guo".to_string()],
        spellings: vec![
            (b"zhong".to_vec(), vec![(0, TYPE_NORMAL)]),
            (b"guo".to_vec(), vec![(1, TYPE_ABBREV)]),
        ],
        groups: vec![(vec![0, 1], 2)],
        entries: vec![(3, "中国".to_string()), (1, "中".to_string())],
    };
    let index = SoundToCharShapeIndex::parse(&builder.bytes()).expect("合法索引");
    assert_eq!(index.spellings.len(), 2);
    assert_eq!(index.entry_count(), 2);
    assert_eq!(index.character_pinyin('中'), ["zhongguo".to_string()]);
    // 头部计数与实际记录数的关系仍被校验（少一条即报错）。
    let mut short = builder.bytes_with([2, 2, 1, 1]);
    short.truncate(short.len() - (4 + 2 + "中".len()));
    assert!(SoundToCharShapeIndex::parse(&short).is_err());
}

/// 越界音节 id（拼写变体侧）：解析失败，不得在建边/查组时越界。
#[test]
fn parse_rejects_spelling_syllable_id_out_of_range() {
    let builder = IndexBuilder {
        syllables: vec!["a".to_string()],
        spellings: vec![(b"a".to_vec(), vec![(1, TYPE_NORMAL)])],
        groups: vec![],
        entries: vec![],
    };
    let error = parse_error(&builder.bytes());
    assert!(
        error.to_string().contains("syllable id out of range"),
        "{error}"
    );
    // kind 只有 0/1 两种合法取值。
    let builder = IndexBuilder {
        syllables: vec!["a".to_string()],
        spellings: vec![(b"a".to_vec(), vec![(0, 2)])],
        groups: vec![],
        entries: vec![],
    };
    assert!(SoundToCharShapeIndex::parse(&builder.bytes()).is_err());
}

/// 越界音节 id（组码侧）与超出 `u16` 宽度的音节表：解析失败。
#[test]
fn parse_rejects_group_syllable_id_out_of_range() {
    let builder = IndexBuilder {
        syllables: vec!["a".to_string()],
        spellings: vec![],
        groups: vec![(vec![1], 0)],
        entries: vec![],
    };
    let error = parse_error(&builder.bytes());
    assert!(
        error.to_string().contains("syllable id out of range"),
        "{error}"
    );
    // 音节数超过组码宽度（`u16`）：组码表示不了 ⇒ 拒绝（否则 `as u16` 静默截断）。
    let builder = IndexBuilder {
        syllables: vec!["a".to_string()],
        ..Default::default()
    };
    let header = [65_536u32, 0, 0, 0];
    let error = parse_error(&builder.bytes_with(header));
    assert!(error.to_string().contains("group code width"), "{error}");
}

/// 组条目数回绕：两组各 `0x8000_0000`、总条目数 0 —— `first += count` 回绕后
/// **恰好**满足原来的「总数一致」校验，随后按组切片越界（`&entries[0..0x8000_0000]`）。
/// 修法后由「逐组 `first + count <= entry_count`」先拦下（`checked_add` 是第二道）。
#[test]
fn parse_rejects_group_entry_offsets_that_wrap() {
    let builder = IndexBuilder {
        syllables: vec!["a".to_string()],
        spellings: vec![],
        groups: vec![(vec![0], 0x8000_0000), (vec![0], 0x8000_0000)],
        entries: vec![],
    };
    let bytes = builder.bytes_with([1, 0, 2, 0]);
    let error = parse_error(&bytes);
    assert!(
        error.to_string().contains("entry count"),
        "回绕必须在切片前被拒绝：{error}"
    );
    // `checked_add` 自身：单组 `u32::MAX` 已够（`entry_count` 也取 `u32::MAX`
    // 才能越过逐组上界检查，但条目循环会先因数据不足失败 —— 这里只要求
    // 「任何畸形头部都不得走到切片」）。
    let builder = IndexBuilder {
        syllables: vec!["a".to_string()],
        spellings: vec![],
        groups: vec![(vec![0], u32::MAX), (vec![0], 1)],
        entries: vec![],
    };
    assert!(SoundToCharShapeIndex::parse(&builder.bytes()).is_err());
}

/// 单组条目数超过总条目数：即便总和不回绕也要拒绝（切片边界依据）。
#[test]
fn parse_rejects_group_entry_count_exceeding_entries() {
    let builder = IndexBuilder {
        syllables: vec!["a".to_string()],
        spellings: vec![],
        groups: vec![(vec![0], 5)],
        entries: vec![(1, "甲".to_string())],
    };
    let error = parse_error(&builder.bytes_with([1, 0, 1, 1]));
    assert!(
        error.to_string().contains("exceeds the entry count"),
        "{error}"
    );
}

/// 巨型头部计数（`u32::MAX`）：容量按剩余字节钳制 ⇒ 快速失败而非巨额分配
/// （未钳制时 `Vec::with_capacity(u32::MAX)` 直接 OOM/abort）。
#[test]
fn parse_rejects_oversized_header_counts_without_huge_allocation() {
    let builder = IndexBuilder::default();
    for header in [
        [u32::MAX, 0, 0, 0],
        [0, u32::MAX, 0, 0],
        [0, 0, u32::MAX, 0],
        [0, 0, 0, u32::MAX],
    ] {
        let error = parse_error(&builder.bytes_with(header));
        let text = error.to_string();
        assert!(
            text.contains("truncated index") || text.contains("group code width"),
            "巨型头部必须以诊断失败：{text}"
        );
    }
}

/// 截断文件：头部声明多于实际字节 ⇒ 诊断（不 panic）；`load` 侧带路径上下文。
#[test]
fn parse_rejects_truncated_index() {
    let builder = IndexBuilder {
        syllables: vec!["zhong".to_string()],
        spellings: vec![(b"zhong".to_vec(), vec![(0, TYPE_NORMAL)])],
        groups: vec![(vec![0], 1)],
        entries: vec![(1, "中".to_string())],
    };
    let full = builder.bytes();
    for cut in [8, 9, 12, 20, full.len() - 1] {
        let error = parse_error(&full[..cut]);
        assert!(!error.to_string().is_empty());
    }
    // 尾部文本长度前缀超出剩余字节。
    let mut bad = full.clone();
    let len = bad.len();
    bad[len - 3..len - 1].copy_from_slice(&0xffffu16.to_le_bytes());
    assert!(SoundToCharShapeIndex::parse(&bad).is_err());
}

/// `load` 对畸形**文件**同样返回诊断（不 panic）；gzip 分支同路。
#[test]
fn load_reports_malformed_files_without_panicking() {
    let dir = std::env::temp_dir().join(format!("hux-tcsrv-bad-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("临时目录");
    let path = dir.join(SOUND_TO_CHAR_SHAPE_FILE);
    std::fs::write(
        &path,
        b"TCSRV01\n\xff\xff\xff\xff\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00",
    )
    .expect("写畸形索引");
    let error = match SoundToCharShapeIndex::load(&path) {
        Ok(_) => panic!("畸形文件不应加载成功"),
        Err(error) => error,
    };
    assert!(error.to_string().contains("解析"), "{error}");
    let gz = dir.join(SOUND_TO_CHAR_SHAPE_FILE_GZ);
    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    std::io::Write::write_all(&mut encoder, &std::fs::read(&path).expect("读回")).expect("压缩");
    std::fs::write(&gz, encoder.finish().expect("gzip")).expect("写畸形 gz");
    assert!(SoundToCharShapeIndex::load(&gz).is_err());
    std::fs::remove_dir_all(&dir).expect("清理临时目录");
}
