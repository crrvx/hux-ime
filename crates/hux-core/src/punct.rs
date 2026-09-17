// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! 标点表（librime `punctuator` 等价物）：`symbols.yaml` 的 half/full shape 映射。
//!
//! 参照配置：`punctuator/import_preset: symbols`、`digit_separators: ""`（不做数字分隔符）、
//! `use_space` 缺省 false。条目形态（参照 `PunctTranslator`）：
//! - 标量字符串 / `{ commit: X }` → 唯一文本；
//! - `{ pair: [a, b] }` → 成对符号，按键交替（参照 `PairPunct` 的 oddness）。
//!
//! `full_shape` 选项切换两张表；`ascii_punct` 选项关闭标点（由宿主链判定）。

use std::path::{Path, PathBuf};

use hashbrown::HashMap;
use yaml_rust2::{Yaml, YamlLoader};

/// 标点定义（参照 `symbols.yaml` 支持的形态）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PunctDef {
    /// 唯一文本：标量字符串或 `{ commit: X }`。
    Text(String),
    /// 成对符号 `{ pair: [a, b] }`。
    Pair(String, String),
}

/// 标点表（half/full shape 两张映射 + pair 交替状态）。
#[derive(Clone, Debug, Default)]
pub struct PunctTable {
    half: HashMap<char, PunctDef>,
    full: HashMap<char, PunctDef>,
    /// pair 交替状态（按「形状 + 键」；参照按定义对象记录，两张表互不影响）。
    pair_oddness: HashMap<(char, bool), bool>,
}

impl PunctTable {
    /// 从 `symbols.yaml` 内容解析（忽略无关节；条目非法则跳过）。
    pub fn parse(content: &str) -> Result<Self, String> {
        let documents = YamlLoader::load_from_str(content).map_err(|error| error.to_string())?;
        let Some(document) = documents.first() else {
            return Ok(Self::default());
        };
        let punctuator = child(document, "punctuator");
        Ok(Self {
            half: parse_shape(child_owned(punctuator, "half_shape")),
            full: parse_shape(child_owned(punctuator, "full_shape")),
            pair_oddness: HashMap::new(),
        })
    }

    /// 读取首个存在的路径（与词库数据同目录探测规则一致）。
    pub fn load_first(paths: &[PathBuf]) -> (Option<Self>, Option<String>) {
        let mut last_error = None;
        for path in paths {
            match std::fs::read_to_string(path) {
                Ok(content) => match Self::parse(&content) {
                    Ok(table) if !table.is_empty() => return (Some(table), None),
                    Ok(_) => last_error = Some(format!("{}: 空标点表", path.display())),
                    Err(error) => last_error = Some(format!("{}: {error}", path.display())),
                },
                Err(error) => last_error = Some(format!("{}: {error}", path.display())),
            }
        }
        (None, last_error)
    }

    /// 读取单个文件。
    pub fn load(path: &Path) -> Result<Self, String> {
        let content = std::fs::read_to_string(path).map_err(|error| error.to_string())?;
        Self::parse(&content)
    }

    pub fn is_empty(&self) -> bool {
        self.half.is_empty() && self.full.is_empty()
    }

    /// 解析 `<key>` 的提交文本（`full_shape` 选项切换映射；pair 交替）。
    pub fn resolve(&mut self, key: char, full_shape: bool) -> Option<String> {
        let table = if full_shape { &self.full } else { &self.half };
        match table.get(&key)? {
            PunctDef::Text(text) => Some(text.clone()),
            PunctDef::Pair(first, second) => {
                let oddness = self.pair_oddness.entry((key, full_shape)).or_insert(false);
                let text = if *oddness { second } else { first };
                *oddness = !*oddness;
                Some(text.clone())
            }
        }
    }
}

fn child<'a>(node: &'a Yaml, key: &str) -> Option<&'a Yaml> {
    match node {
        Yaml::Hash(map) => map.get(&Yaml::String(key.to_string())),
        _ => None,
    }
}

fn child_owned(node: Option<&Yaml>, key: &str) -> Option<Yaml> {
    node.and_then(|node| child(node, key)).cloned()
}

fn parse_shape(node: Option<Yaml>) -> HashMap<char, PunctDef> {
    let mut out = HashMap::new();
    let Some(Yaml::Hash(map)) = node else {
        return out;
    };
    for (key, value) in map {
        let Yaml::String(key) = key else { continue };
        let Some(ch) = single_char(&key) else {
            continue;
        };
        let Some(definition) = parse_definition(&value) else {
            continue;
        };
        out.insert(ch, definition);
    }
    out
}

fn single_char(text: &str) -> Option<char> {
    let mut chars = text.chars();
    let first = chars.next()?;
    if chars.next().is_none() {
        Some(first)
    } else {
        None
    }
}

fn parse_definition(value: &Yaml) -> Option<PunctDef> {
    match value {
        Yaml::String(text) => Some(PunctDef::Text(text.clone())),
        Yaml::Hash(map) => {
            if let Some(Yaml::String(commit)) = map.get(&Yaml::String("commit".to_string())) {
                return Some(PunctDef::Text(commit.clone()));
            }
            if let Some(Yaml::Array(pair)) = map.get(&Yaml::String("pair".to_string())) {
                if pair.len() == 2 {
                    if let (Yaml::String(first), Yaml::String(second)) = (&pair[0], &pair[1]) {
                        return Some(PunctDef::Pair(first.clone(), second.clone()));
                    }
                }
            }
            None
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"
punctuator:
  full_shape:
    ",": { commit: ， }
    "'": { pair: [ "‘", "’" ] }
    "\\": 、
  half_shape:
    ",": { commit: ， }
    "-": "-"
    "'": { pair: [ "‘", "’" ] }
"#;

    #[test]
    fn parses_shapes_and_definitions() {
        let table = PunctTable::parse(SAMPLE).expect("parse");
        assert!(!table.is_empty());
        let mut table = table;
        assert_eq!(table.resolve(',', false), Some("，".to_string()));
        assert_eq!(table.resolve('-', false), Some("-".to_string()));
        assert_eq!(table.resolve('-', true), None, "full_shape 未定义则无标点");
        assert_eq!(table.resolve('\\', true), Some("、".to_string()));
    }

    #[test]
    fn pair_alternates_per_key() {
        let mut table = PunctTable::parse(SAMPLE).expect("parse");
        assert_eq!(table.resolve('\'', false), Some("‘".to_string()));
        assert_eq!(table.resolve('\'', false), Some("’".to_string()));
        assert_eq!(table.resolve('\'', false), Some("‘".to_string()));
        // 另一张表（full_shape）独立交替
        assert_eq!(table.resolve('\'', true), Some("‘".to_string()));
        assert_eq!(table.resolve('\'', true), Some("’".to_string()));
    }

    #[test]
    fn shipped_default_symbols_override_slash() {
        // 发布默认（data/symbols.yaml）：half_shape 的 "/" 提交 "/"（非 、）；full_shape 仍为 ／。
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data/symbols.yaml");
        let mut table = PunctTable::load(&path).expect("load data/symbols.yaml");
        assert_eq!(table.resolve('/', false), Some("/".to_string()));
        assert_eq!(table.resolve('/', true), Some("／".to_string()));
    }

    #[test]
    fn rejects_malformed_documents() {
        assert!(PunctTable::parse("\t\t: [").is_err());
        let empty = PunctTable::parse("other: 1").expect("parse");
        assert!(empty.is_empty());
    }
}
