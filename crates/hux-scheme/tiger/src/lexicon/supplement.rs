// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! 补充短语的 Aho–Corasick 匹配器与奖励曲线。

use hashbrown::HashMap;
use hux_core::collections::Map;
use std::path::Path;

use super::files::{SUPPLEMENT_FILE, is_lua_space_byte, trim_lua_whitespace};

// ---------------------------------------------------------------- 补充短语

const SUPPLEMENT_BASELINE_REWARD: f64 = 9.0;
const SUPPLEMENT_WEIGHT_SCALE: f64 = 2.0;
const SUPPLEMENT_BASELINE_WEIGHT: f64 = 1000.0;
const SUPPLEMENT_MAXIMUM_REWARD: f64 = 16.0;

/// 参照 `reward_for_weight`：重量 → 补充奖励。
///
/// NaN 口径：Lua 的 `math.max(1, math.min(1e9, nan))` 返回 `1e9`，
/// Rust 的 `clamp` 对 NaN 返回 NaN ⇒ 两端相反。此处**保持 Rust 语义**：正常数据不可达
/// （`parse_supplement_content` 的 `weight > 0.0` 已排除 NaN），唯一可达面是
/// [`Supplement::build`] 的公开入参（畸形输入、无金样支撑），故只注明差异、不改行为。
pub(super) fn reward_for_weight(weight: f64) -> f64 {
    let bounded = weight.clamp(1.0, 1_000_000_000.0);
    let reward = SUPPLEMENT_BASELINE_REWARD
        + SUPPLEMENT_WEIGHT_SCALE * (bounded / SUPPLEMENT_BASELINE_WEIGHT).ln();
    reward.clamp(0.0, SUPPLEMENT_MAXIMUM_REWARD)
}

struct SupplementNode {
    transitions: HashMap<char, usize>,
    failure: usize,
    reward: f64,
}

/// 补充短语的 Aho–Corasick 匹配器（`supplement` 表）。
///
/// 节点编号受构建顺序影响，但 `advance` 的奖励结果与顺序无关；
/// 差分验证以奖励序列为准（随 decode 金样覆盖）。
pub struct Supplement {
    nodes: Vec<SupplementNode>,
    pub path: Option<String>,
    pub count: usize,
    pub error: Option<String>,
}

impl Supplement {
    fn empty(path: Option<String>, error: Option<String>) -> Self {
        Self {
            nodes: vec![SupplementNode {
                transitions: HashMap::new(),
                failure: 0,
                reward: 0.0,
            }],
            path,
            count: 0,
            error,
        }
    }

    /// 参照 `supplement.load_default`：文件缺失是正常状态（count=0）。
    pub fn load_default(user_dir: Option<&Path>) -> Self {
        let Some(directory) = user_dir else {
            return Self::empty(None, None);
        };
        let path = directory.join(SUPPLEMENT_FILE);
        Self::load_file(&path)
    }

    fn load_file(path: &Path) -> Self {
        let display = path.to_string_lossy().into_owned();
        // 与 `Lexicon::read_data_file` 一致：非法 UTF-8 视作空数据并记错误（有意偏离）。
        let content = match std::fs::read_to_string(path) {
            Ok(content) => content,
            Err(error) => return Self::empty(Some(display), Some(error.to_string())),
        };
        Self::build(&parse_supplement_content(&content), Some(display))
    }

    pub(super) fn build(entries: &Map<String, f64>, path: Option<String>) -> Self {
        let mut nodes = vec![SupplementNode {
            transitions: HashMap::new(),
            failure: 0,
            reward: 0.0,
        }];
        let count = insert_entries(&mut nodes, entries);

        if count == 0 {
            return Self::empty(path, None);
        }

        build_failure_links(&mut nodes);

        Self {
            nodes,
            path,
            count,
            error: None,
        }
    }

    /// 参照 `supplement.advance`：返回 (状态, 奖励)；状态为 1 基（根 = 1）。
    pub(crate) fn advance(&self, state: usize, character: char) -> (usize, f64) {
        if self.count == 0 {
            return (1, 0.0);
        }
        let mut current = if state >= 1 && state <= self.nodes.len() {
            state
        } else {
            1
        };
        while current != 1 && !self.nodes[current - 1].transitions.contains_key(&character) {
            current = self.nodes[current - 1].failure + 1;
        }
        current = self.nodes[current - 1]
            .transitions
            .get(&character)
            .map(|index| index + 1)
            .unwrap_or(1);
        (current, self.nodes[current - 1].reward)
    }

    pub fn status(&self) -> SupplementStatus {
        SupplementStatus {
            count: self.count,
            has_error: self.error.is_some(),
        }
    }
}

/// 把短语逐字插入 trie，返回插入条数（空文本与非正奖励的行跳过）。
fn insert_entries(nodes: &mut Vec<SupplementNode>, entries: &Map<String, f64>) -> usize {
    let mut count = 0usize;
    for (text, weight) in entries.iter() {
        let reward = reward_for_weight(*weight);
        if text.is_empty() || reward <= 0.0 {
            continue;
        }
        let mut state = 0usize;
        for character in text.chars() {
            let next = nodes[state].transitions.get(&character).copied();
            let next = match next {
                Some(next) => next,
                None => {
                    let index = nodes.len();
                    nodes.push(SupplementNode {
                        transitions: HashMap::new(),
                        failure: 0,
                        reward: 0.0,
                    });
                    nodes[state].transitions.insert(character, index);
                    index
                }
            };
            state = next;
        }
        nodes[state].reward = nodes[state].reward.max(reward);
        count += 1;
    }
    count
}

/// AC 失配链与奖励上推（BFS）。
fn build_failure_links(nodes: &mut [SupplementNode]) {
    let mut queue: Vec<usize> = Vec::new();
    let root_children: Vec<usize> = nodes[0].transitions.values().copied().collect();
    for &child in &root_children {
        nodes[child].failure = 0;
        queue.push(child);
    }
    let mut head = 0usize;
    while head < queue.len() {
        let current = queue[head];
        head += 1;
        let children: Vec<(char, usize)> = nodes[current]
            .transitions
            .iter()
            .map(|(character, index)| (*character, *index))
            .collect();
        for (character, child) in children {
            let mut fallback = nodes[current].failure;
            while fallback != 0 && !nodes[fallback].transitions.contains_key(&character) {
                fallback = nodes[fallback].failure;
            }
            let failure_target = nodes[fallback].transitions.get(&character).copied();
            if let Some(target) = failure_target
                && target != child
            {
                nodes[child].failure = target;
            } else {
                nodes[child].failure = 0;
            }
            let failure = nodes[child].failure;
            nodes[child].reward = nodes[child].reward.max(nodes[failure].reward);
            queue.push(child);
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SupplementStatus {
    pub count: usize,
    pub has_error: bool,
}

impl SupplementStatus {
    pub fn canonical(&self) -> String {
        format!("count={} error={}", self.count, self.has_error as u8)
    }
}

/// 参照 `supplement.load_file` 的行解析：`text [weight]`，非法权重丢弃该行。
fn parse_supplement_content(content: &str) -> Map<String, f64> {
    let mut entries = Map::new();
    let body = content.strip_prefix('\u{feff}').unwrap_or(content);
    // `(content .. "\n"):gmatch("(.-)\r?\n")`：按行切分，含空行。
    for raw_line in body.split('\n') {
        let raw_line = raw_line.strip_suffix('\r').unwrap_or(raw_line);
        let line = trim_lua_whitespace(raw_line);
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (text, rest) = match first_two_tokens_relaxed(line) {
            Some((text, rest)) => (text, rest),
            None => (line, ""),
        };
        let weight = if rest.is_empty() {
            1000.0
        } else if !rest.bytes().all(|byte| byte.is_ascii_digit()) {
            continue;
        } else {
            match rest.parse::<f64>() {
                Ok(weight) if weight > 0.0 => weight,
                _ => continue,
            }
        };
        entries.insert(text.to_string(), weight);
    }
    entries
}

/// 参照 `^(%S+)%s*(.-)$`：首 token 与其后剩余（去前导空白）。
fn first_two_tokens_relaxed(line: &str) -> Option<(&str, &str)> {
    let bytes = line.as_bytes();
    let mut index = 0;
    while index < bytes.len() && !is_lua_space_byte(bytes[index]) {
        index += 1;
    }
    if index == 0 {
        return None;
    }
    let text = &line[..index];
    while index < bytes.len() && is_lua_space_byte(bytes[index]) {
        index += 1;
    }
    Some((text, &line[index..]))
}
