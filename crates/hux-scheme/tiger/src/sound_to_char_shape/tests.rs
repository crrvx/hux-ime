// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! 索引格式、图翻译与标点注释的单元测试（原名内联于门面文件）。

use super::index::{MAGIC, TYPE_ABBREV, TYPE_NORMAL};
use super::*;
use crate::lexicon::Lexicon;

mod parse;
mod pattern;
mod translate;

fn fixture_index() -> SoundToCharShapeIndex {
    let path = hux_test_support::repo_path("goldens/sound_to_char_shape/tiger_sentence.pinyin.bin");
    SoundToCharShapeIndex::load(&path).expect("fixture index")
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
    pub(super) syllables: Vec<String>,
    pub(super) spellings: Vec<SpellingEntry>,
    pub(super) groups: Vec<(Vec<u16>, u32)>,
    pub(super) entries: Vec<(u32, String)>,
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
