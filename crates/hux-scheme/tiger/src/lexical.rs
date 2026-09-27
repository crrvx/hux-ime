// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! 紧凑词先验（TCSLEX01 Bloom filter），对应参照 `lua/tiger_sentence_lexical.lua`。
//!
//! 数据来源：由上游词先验导出（原作者署名，许可 CC BY 4.0）；参数摘要见本模块常量。
//! 只用于最终排序（Top-5 重排），不进入 mass/置信度。

use hux_core::collections::Map;
use std::path::{Path, PathBuf};

const MAGIC: &[u8; 8] = b"TCSLEX01";
const HEADER_SIZE: usize = 32;
const MODULUS: u64 = 4_294_967_291;

/// 已加载的词先验模型。
#[derive(Clone, Debug)]
pub struct LexicalModel {
    bits: Vec<u8>,
    pub bit_count: usize,
    pub hash_count: usize,
    pub entry_count: usize,
    pub minimum_length: usize,
    pub maximum_length: usize,
    pub bytes: usize,
}

fn u32le(data: &[u8], offset: usize) -> Option<u32> {
    let bytes = data.get(offset..offset + 4)?;
    Some(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

/// 参照 `load`：解析 TCSLEX01 内容。
pub fn parse(data: Vec<u8>) -> Result<LexicalModel, String> {
    if data.len() < HEADER_SIZE || &data[..8] != MAGIC {
        return Err("not a TCSLEX01 lexical model".to_string());
    }
    let version = u32le(&data, 8);
    let entry_count = u32le(&data, 12);
    let bit_count = u32le(&data, 16);
    let hash_count = u32le(&data, 20);
    let minimum_length = u32le(&data, 24);
    let maximum_length = u32le(&data, 28);
    let (Some(version), Some(entry_count), Some(bit_count), Some(hash_count)) =
        (version, entry_count, bit_count, hash_count)
    else {
        return Err("invalid TCSLEX01 header".to_string());
    };
    let (Some(minimum_length), Some(maximum_length)) = (minimum_length, maximum_length) else {
        return Err("invalid TCSLEX01 header".to_string());
    };
    if version != 1
        || entry_count < 1
        || bit_count < 8
        || bit_count % 8 != 0
        || !(1..=32).contains(&hash_count)
        || minimum_length < 2
        || maximum_length < minimum_length
        || maximum_length > 16
    {
        return Err("invalid TCSLEX01 header".to_string());
    }
    if data.len() != HEADER_SIZE + bit_count as usize / 8 {
        return Err("TCSLEX01 size does not match header".to_string());
    }
    Ok(LexicalModel {
        bits: data[HEADER_SIZE..].to_vec(),
        bit_count: bit_count as usize,
        hash_count: hash_count as usize,
        entry_count: entry_count as usize,
        minimum_length: minimum_length as usize,
        maximum_length: maximum_length as usize,
        bytes: data.len(),
    })
}

/// 参照 `load`：从文件读取并解析。
pub fn load(path: &Path) -> Result<LexicalModel, String> {
    let data = std::fs::read(path).map_err(|error| error.to_string())?;
    parse(data)
}

/// 参照 `M.load_first`：返回首个可加载模型与（若有）首个错误。
pub fn load_first(paths: &[PathBuf]) -> (Option<LexicalModel>, Option<String>) {
    let mut first_error: Option<String> = None;
    for path in paths {
        // 参照：打不开的路径静默跳过；打开后解析失败才记首个错误。
        let Ok(data) = std::fs::read(path) else {
            continue;
        };
        match parse(data) {
            Ok(model) => return (Some(model), None),
            Err(error) => {
                if first_error.is_none() {
                    first_error = Some(format!("{}: {error}", path.display()));
                }
            }
        }
    }
    (None, first_error)
}

/// 参照 `hashes`：双 32 位散列（整数中间量 < 2^53，按 u64 精确计算）。
pub fn hashes(text: &str) -> (u64, u64) {
    let mut first: u64 = 2_166_136_261;
    let mut second: u64 = 16_777_619;
    for byte in text.as_bytes() {
        first = (first * 131 + *byte as u64 + 17) % MODULUS;
        second = (second * 137 + *byte as u64 + 53) % MODULUS;
    }
    if second == 0 {
        second = 1;
    }
    (first, second)
}

impl LexicalModel {
    /// 参照模块内 `contains`：仅位图查询（不做词长检查）。
    pub fn contains_bits(&self, text: &str) -> bool {
        if text.is_empty() {
            return false;
        }
        let (first, second) = hashes(text);
        for index in 0..self.hash_count as u64 {
            let bit = (first + index * second + index * index * 97) % self.bit_count as u64;
            let byte = self.bits[(bit / 8) as usize];
            let mask = 1u8 << (bit % 8);
            if byte & mask == 0 {
                return false;
            }
        }
        true
    }

    /// 参照 `M.contains`：词长须在 `[minimum_length, maximum_length]`。
    pub fn contains(&self, text: &str) -> bool {
        let length = text.chars().count();
        length >= self.minimum_length && length <= self.maximum_length && self.contains_bits(text)
    }

    /// 参照 `M.score`（无缓存）。
    pub fn score(&self, text: &str) -> f64 {
        self.score_with_cache(text, &mut Map::new())
    }

    /// 参照 `M.score`：最大权不重叠词覆盖；`cache` 复刻参照的 `lookup_cache`。
    pub fn score_with_cache(&self, text: &str, cache: &mut Map<String, bool>) -> f64 {
        if text.is_empty() {
            return 0.0;
        }
        let chars: Vec<char> = text.chars().collect();
        let count = chars.len();
        let mut best = vec![0.0f64; count + 1];
        for finish in 1..=count {
            best[finish] = best[finish - 1];
            for length in self.minimum_length..=self.maximum_length {
                if length > finish {
                    break;
                }
                let start = finish - length;
                let word: String = chars[start..finish].iter().collect();
                let hit = match cache.get(&word) {
                    Some(value) => *value,
                    None => {
                        let value = self.contains_bits(&word);
                        cache.insert(word.clone(), value);
                        value
                    }
                };
                if hit {
                    let value = best[start] + 1.0 + 0.2 * (length as f64 - 2.0);
                    if value > best[finish] {
                        best[finish] = value;
                    }
                }
            }
        }
        best[count]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 构造一个最小 TCSLEX01（自洽位图，仅用于单测）。
    fn synthetic(entries: &[&str], bits: usize, hashes_count: usize) -> LexicalModel {
        assert_eq!(bits % 8, 0, "合成夹具的位图长度必须是整字节");
        let mut bitmap = vec![0u8; bits / 8];
        for entry in entries {
            let (first, second) = hashes(entry);
            for index in 0..hashes_count as u64 {
                let bit = (first + index * second + index * index * 97) % bits as u64;
                bitmap[(bit / 8) as usize] |= 1u8 << (bit % 8);
            }
        }
        let mut data = Vec::new();
        data.extend_from_slice(MAGIC);
        data.extend_from_slice(&1u32.to_le_bytes());
        data.extend_from_slice(&(entries.len() as u32).to_le_bytes());
        data.extend_from_slice(&(bits as u32).to_le_bytes());
        data.extend_from_slice(&(hashes_count as u32).to_le_bytes());
        data.extend_from_slice(&2u32.to_le_bytes());
        data.extend_from_slice(&4u32.to_le_bytes());
        data.extend_from_slice(&bitmap);
        parse(data).expect("parse synthetic")
    }

    /// 头部五字段是定位位图与长度闸门的唯一依据，必须逐字段回读校验。
    #[test]
    fn header_fields_are_parsed() {
        let model = synthetic(&["甲乙", "甲乙丙"], 8192, 4);
        assert_eq!(model.bit_count, 8192, "头部位图位数应回读写入的 8192");
        assert_eq!(model.hash_count, 4, "头部哈希个数应回读 4");
        assert_eq!(model.entry_count, 2, "头部词条数应回读 2");
        assert_eq!(model.minimum_length, 2, "最短词长应回读 2");
        assert_eq!(model.maximum_length, 4, "最长词长应回读 4");
    }

    /// 钉住 contains 与 contains_bits 的分工：前者先过长度闸门，后者直查位图，越界词上两者可以不一致。
    #[test]
    fn contains_honors_length_gate() {
        let model = synthetic(&["甲乙", "甲乙丙"], 8192, 4);
        assert!(model.contains("甲乙"), "闸门内且位图命中的「甲乙」应判存在");
        assert!(
            model.contains("甲乙丙"),
            "闸门内且位图命中的「甲乙丙」应判存在"
        );
        assert!(
            !model.contains("甲"),
            "短于 minimum_length 必须判不存在，与位图无关"
        );
        assert!(
            !model.contains("甲乙丙丁戊"),
            "长于 maximum_length 必须判不存在"
        );
        // 词长越界：M.contains 为假，但位图查询可能为真
        let (first, second) = hashes("甲");
        let mut bit_hit = true;
        for index in 0..4u64 {
            let bit = (first + index * second + index * index * 97) % 8192;
            if model.bits[(bit / 8) as usize] & (1u8 << (bit % 8)) == 0 {
                bit_hit = false;
            }
        }
        assert_eq!(
            model.contains_bits("甲"),
            bit_hit,
            "contains_bits 必须等价于逐位手算，不受长度闸门影响"
        );
    }

    /// 词图打分的基线契约：只累加不重叠词，空串得 0。
    #[test]
    fn score_sums_unoverlapping_words() {
        let model = synthetic(&["甲乙", "甲乙丙"], 8192, 4);
        // score：两个不重叠词 = 1.0 + 1.2
        assert!(
            (model.score("甲乙甲乙丙") - 2.2).abs() < 1e-12,
            "两个不重叠词应累计 1.0+1.2=2.2"
        );
        assert_eq!(model.score(""), 0.0, "空串不得得分");
    }

    /// 截断或版本不符的头部必须报错，不得解析出半个模型。
    #[test]
    fn parse_rejects_bad_headers() {
        assert!(
            parse(MAGIC.to_vec()).is_err(),
            "只有 MAGIC、缺头部的模型必须报错"
        );
        let mut bad = Vec::new();
        bad.extend_from_slice(MAGIC);
        bad.extend_from_slice(&2u32.to_le_bytes());
        bad.extend_from_slice(&[0u8; 24]);
        assert!(parse(bad).is_err(), "头部版本号非 1 必须报错");
    }

    /// 记忆化只是加速：命中缓存不得改变得分，比较按位型而非浮点近似。
    #[test]
    fn score_with_cache_matches_uncached() {
        let model = synthetic(&["甲乙", "甲乙丙"], 8192, 4);
        let text = "甲乙甲甲乙丙";
        let mut cache = Map::new();
        // 缓存路径与无缓存路径必须逐位一致（重复子串触发缓存命中）。
        assert_eq!(
            model.score_with_cache(text, &mut cache).to_bits(),
            model.score(text).to_bits(),
            "缓存路径与无缓存路径的得分必须逐位一致"
        );
        assert!(
            cache.contains_key("甲乙"),
            "重复子串必须真正写进缓存，否则本用例没覆盖到缓存路径"
        );
    }

    /// 钉住随仓发布词库的头部与体量，数据换代时这些数字必须显式更新。
    #[test]
    fn real_model_loads_when_present() {
        let path = hux_test_support::repo_path("data/tiger_sentence.lexical.bin");
        let model = load(&path).expect("load real lexical model");
        assert_eq!(model.bit_count, 1_200_000, "发布模型位图位数应为 1200000");
        assert_eq!(model.hash_count, 10, "发布模型哈希个数应为 10");
        assert_eq!(model.minimum_length, 2, "发布模型最短词长应为 2");
        assert_eq!(model.maximum_length, 4, "发布模型最长词长应为 4");
        assert_eq!(model.bytes, 150_032, "发布模型文件字节数应为 150032");
        assert!(model.contains_bits("我们"), "发布模型位图应命中「我们」");
        assert!(model.contains("我们"), "发布模型应判定「我们」存在");
        assert!(
            model.score("我们的") > 0.0,
            "发布模型给「我们的」的得分应为正"
        );
    }
}
