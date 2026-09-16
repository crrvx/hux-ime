//! 紧凑词先验（TCSLEX01 Bloom filter），对应参照 `lua/tiger_sentence_lexical.lua`。
//!
//! 数据来源与许可见 `docs/LEXICAL_PRIOR_ATTRIBUTION.md`（CC BY 4.0）；
//! 参数与校验和见 `docs/LEXICAL_PRIOR_MANIFEST.json`。
//! 只用于最终排序（Top-5 重排），不进入 mass/置信度。

use hashbrown::HashMap;
use std::path::{Path, PathBuf};

const MAGIC: &[u8; 8] = b"TCSLEX01";
const HEADER_SIZE: usize = 32;
const MODULUS: u64 = 4_294_967_291;

/// 已加载的词先验模型。
#[derive(Clone, Debug)]
pub struct LexicalModel {
    pub path: PathBuf,
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
pub fn parse(data: Vec<u8>, path: PathBuf) -> Result<LexicalModel, String> {
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
        path,
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
    parse(data, path.to_path_buf())
}

/// 参照 `M.load_first`：返回首个可加载模型与（若有）首个错误。
pub fn load_first(paths: &[PathBuf]) -> (Option<LexicalModel>, Option<String>) {
    let mut first_error: Option<String> = None;
    for path in paths {
        // 参照：打不开的路径静默跳过；打开后解析失败才记首个错误。
        let Ok(data) = std::fs::read(path) else {
            continue;
        };
        match parse(data, path.to_path_buf()) {
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
        self.score_with_cache(text, &mut HashMap::new())
    }

    /// 参照 `M.score`：最大权不重叠词覆盖；`cache` 复刻参照的 `lookup_cache`。
    pub fn score_with_cache(&self, text: &str, cache: &mut HashMap<String, bool>) -> f64 {
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
        assert_eq!(bits % 8, 0);
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
        parse(data, PathBuf::from("synthetic")).expect("parse synthetic")
    }

    #[test]
    fn header_validation_and_queries() {
        let model = synthetic(&["甲乙", "甲乙丙"], 8192, 4);
        assert_eq!(model.bit_count, 8192);
        assert_eq!(model.hash_count, 4);
        assert_eq!(model.entry_count, 2);
        assert_eq!(model.minimum_length, 2);
        assert_eq!(model.maximum_length, 4);
        assert!(model.contains("甲乙"));
        assert!(model.contains("甲乙丙"));
        assert!(!model.contains("甲"));
        assert!(!model.contains("甲乙丙丁戊"));
        // 词长越界：M.contains 为假，但位图查询可能为真
        let (first, second) = hashes("甲");
        let mut bit_hit = true;
        for index in 0..4u64 {
            let bit = (first + index * second + index * index * 97) % 8192;
            if model.bits[(bit / 8) as usize] & (1u8 << (bit % 8)) == 0 {
                bit_hit = false;
            }
        }
        assert_eq!(model.contains_bits("甲"), bit_hit);
        // score：两个不重叠词 = 1.0 + 1.2
        assert!((model.score("甲乙甲乙丙") - 2.2).abs() < 1e-12);
        assert_eq!(model.score(""), 0.0);
        // 头部拒绝
        assert!(parse(MAGIC.to_vec(), PathBuf::from("x")).is_err());
        let mut bad = Vec::new();
        bad.extend_from_slice(MAGIC);
        bad.extend_from_slice(&2u32.to_le_bytes());
        bad.extend_from_slice(&[0u8; 24]);
        assert!(parse(bad, PathBuf::from("x")).is_err());
    }

    #[test]
    fn score_cache_matches_uncached_and_length_gate() {
        let model = synthetic(&["甲乙", "甲乙丙"], 8192, 4);
        let text = "甲乙甲甲乙丙";
        let mut cache = HashMap::new();
        // 缓存路径与无缓存路径必须逐位一致（重复子串触发缓存命中）。
        assert_eq!(
            model.score_with_cache(text, &mut cache).to_bits(),
            model.score(text).to_bits()
        );
        assert!(cache.contains_key("甲乙"));
        // 词长门：1 字与 5 字恒 false，位图查询与门控结果解耦
        assert!(!model.contains("甲"));
        assert!(!model.contains("甲乙丙丁戊"));
        let _ = model.contains_bits("甲");
    }

    #[test]
    fn real_model_loads_when_present() {
        let path =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../data/tiger_sentence.lexical.bin");
        let model = load(&path).expect("load real lexical model");
        assert_eq!(model.bit_count, 1_200_000);
        assert_eq!(model.hash_count, 10);
        assert_eq!(model.minimum_length, 2);
        assert_eq!(model.maximum_length, 4);
        assert_eq!(model.bytes, 150_032);
        assert!(model.contains_bits("我们"));
        assert!(model.contains("我们"));
        assert!(model.score("我们的") > 0.0);
    }
}
