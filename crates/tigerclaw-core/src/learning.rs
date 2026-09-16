//! Tab 纠错学习，对应参照 `lua/tiger_sentence_learning.lua` 的纯计算部分。
//!
//! 本增量：`build`（全量重放 oracle）、`runtime_index`/`update`（运行时快照）、
//! `score`/`prefix_score`（含物化缓存）、`reward`（路径链）、`diff`、
//! `context`/`static`/`frame`/`unframe`/`hash`。
//! 持久化（LevelDb `open`/`confirm`）随 K3 数据层接入。
//!
//! 注意：浮点求和顺序仅在“同一 (code, mode, text) 的 ≥3 个上下文”时可能产生
//! ULP 差异（参照按 `pairs` 序累加）；差分语料限制为 ≤2 个上下文以保证逐位一致。

use crate::cache::Fifo;
use hashbrown::{HashMap, HashSet};
use std::rc::Rc;

const HALF_LIFE: f64 = 30.0 * 86400.0;
const MATERIALIZED_CODE_LIMIT: usize = 256;
const PREFIX_QUERY_LIMIT: usize = 4096;
const CODE_WINDOW_LIMIT: usize = 2048;
const CODE_WINDOW_SLOTS: usize = 64;
const MAX_FRAME_PART: usize = 8192;

#[derive(Clone, Debug)]
pub struct Event {
    pub time: f64,
    pub mode: String,
    pub code: String,
    pub text: String,
    pub context: String,
}

#[derive(Clone, Copy)]
struct Choice {
    weight: f64,
    count: f64,
    time: f64,
}

#[derive(Clone)]
struct Group {
    code: String,
    mode: String,
    context: String,
    choices: HashMap<String, Choice>,
}

#[derive(Clone)]
struct Summary {
    mode: String,
    text: String,
    exact: HashMap<String, f64>,
    weight: f64,
    count: f64,
    contexts: f64,
    general: f64,
}

#[derive(Clone)]
struct PrefixEntry {
    general: f64,
    exact: HashMap<String, f64>,
}

#[derive(Default)]
struct Materialized {
    exact: HashMap<String, Summary>,
    prefixes: HashMap<String, PrefixEntry>,
}

/// 奖励路径链节点（对应参照 `previous` 链上的一个状态）。
#[derive(Clone, Debug)]
pub struct RewardNode {
    pub text: String,
    pub text_length: usize,
    pub raw_length: usize,
    pub learning_score: f64,
}

/// diff 路径节点。
#[derive(Clone, Debug)]
pub struct DiffPathNode {
    pub raw_length: usize,
    pub text_length: usize,
}

/// diff 候选：文本 + 路径链（`path[0]` 为最外层节点）。
#[derive(Clone, Debug)]
pub struct DiffItem {
    pub text: String,
    pub path: Vec<DiffPathNode>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiffEvent {
    pub mode: String,
    pub code: String,
    pub text: String,
    pub context: String,
    pub raw_start: usize,
    pub raw_end: usize,
    pub text_start: usize,
    pub text_end: usize,
}

/// 学习索引：`build` 形态（全量重放）或运行时形态（分区 + 物化缓存）。
pub struct LearningIndex {
    pub codes: Vec<String>,
    pub now: f64,
    pub future: f64,
    partitions: Option<HashMap<String, HashMap<String, Group>>>,
    exact: Option<HashMap<String, Summary>>,
    prefixes: Option<HashMap<String, PrefixEntry>>,
    cache: Fifo<String, Rc<Materialized>>,
    prefix_queries: Fifo<String, f64>,
    code_windows: Fifo<String, (usize, isize)>,
}

// ---------------------------------------------------------------- 工具函数

/// 严格 UTF-8 序列长度（对齐参照 `chars`/`character_count` 的接受集合）。
fn utf8_len_at(bytes: &[u8], index: usize) -> Option<usize> {
    let a = *bytes.get(index)?;
    if a < 0x80 {
        return Some(1);
    }
    let cont = |offset: usize| {
        bytes
            .get(index + offset)
            .map(|byte| (0x80..=0xBF).contains(byte))
            .unwrap_or(false)
    };
    if (0xC2..=0xDF).contains(&a) && cont(1) {
        return Some(2);
    }
    if (0xE0..=0xEF).contains(&a) && cont(1) && cont(2) {
        let b = bytes[index + 1];
        if !(a == 0xE0 && b < 0xA0) && !(a == 0xED && b >= 0xA0) {
            return Some(3);
        }
        return None;
    }
    if (0xF0..=0xF4).contains(&a) && cont(1) && cont(2) && cont(3) {
        let b = bytes[index + 1];
        if !(a == 0xF0 && b < 0x90) && !(a == 0xF4 && b >= 0x90) {
            return Some(4);
        }
        return None;
    }
    None
}

/// 参照 `chars`：返回字符列表；非法 UTF-8 返回 None。
pub fn chars(text: &str) -> Option<Vec<char>> {
    let bytes = text.as_bytes();
    let mut result = Vec::new();
    let mut index = 0usize;
    while index < bytes.len() {
        let length = utf8_len_at(bytes, index)?;
        result.push(text[index..index + length].chars().next()?);
        index += length;
    }
    Some(result)
}

/// 参照 `character_count`：非法输入返回 0。
pub(crate) fn character_count(text: &str) -> usize {
    let bytes = text.as_bytes();
    let mut count = 0usize;
    let mut index = 0usize;
    while index < bytes.len() {
        let Some(length) = utf8_len_at(bytes, index) else {
            return 0;
        };
        index += length;
        count += 1;
    }
    count
}

/// 参照 `static`：1..16 字符，且无控制字符/花括号/私用区序列。
pub fn static_text(text: &str) -> bool {
    let Some(list) = chars(text) else {
        return false;
    };
    if list.is_empty() || list.len() > 16 {
        return false;
    }
    let bytes = text.as_bytes();
    for (index, byte) in bytes.iter().enumerate() {
        if *byte == 0
            || (0x01..=0x1F).contains(byte)
            || *byte == 0x7F
            || *byte == b'{'
            || *byte == b'}'
        {
            return false;
        }
        if *byte == 0xEE
            && index + 2 < bytes.len()
            && (0x80..=0xBF).contains(&bytes[index + 1])
            && (0x80..=0xBF).contains(&bytes[index + 2])
        {
            return false;
        }
        if *byte == 0xEF
            && index + 2 < bytes.len()
            && (0x80..=0xA3).contains(&bytes[index + 1])
            && (0x80..=0xBF).contains(&bytes[index + 2])
        {
            return false;
        }
    }
    true
}

/// 参照 `context`：保留最后 2 个字符（单字符文本保留其自身）。
pub fn context(text: &str) -> String {
    let Some(list) = chars(text) else {
        return String::new();
    };
    if list.is_empty() {
        return String::new();
    }
    let start = list.len().saturating_sub(2);
    list[start..].iter().collect()
}

/// 参照 `key`：`#value:value` 拼接。
fn key(parts: &[&str]) -> String {
    let mut out = String::new();
    for part in parts {
        out.push_str(&format!("{}:{}", part.len(), part));
    }
    out
}

/// 参照 `frame`。
pub fn frame(values: &[String]) -> String {
    let refs: Vec<&str> = values.iter().map(String::as_str).collect();
    key(&refs)
}

/// 参照 `unframe`。
pub fn unframe(value: &str) -> Option<Vec<String>> {
    let bytes = value.as_bytes();
    let mut result = Vec::new();
    let mut position = 0usize;
    while position < bytes.len() {
        let mut digit_end = position;
        while digit_end < bytes.len() && bytes[digit_end].is_ascii_digit() {
            digit_end += 1;
        }
        if digit_end == position || digit_end >= bytes.len() || bytes[digit_end] != b':' {
            return None;
        }
        let length: usize = std::str::from_utf8(&bytes[position..digit_end])
            .ok()?
            .parse()
            .ok()?;
        if length > MAX_FRAME_PART || digit_end + 1 + length > bytes.len() {
            return None;
        }
        result.push(value[digit_end + 1..digit_end + 1 + length].to_string());
        position = digit_end + 1 + length;
    }
    Some(result)
}

/// 参照 `M.hash`：双累加器 FNV 变体，输出 `%08x%08x`。
pub fn hash(text: &str) -> String {
    let mut a: u64 = 2166136261;
    let mut b: u64 = 5381;
    for byte in text.bytes() {
        a = (a * 65599 + byte as u64) % 4294967296;
        b = (b * 33 + byte as u64) % 4294967296;
    }
    format!("{a:08x}{b:08x}")
}

fn mode_valid(mode: &str) -> bool {
    !mode.is_empty() && mode.len() <= 512
}

fn code_valid(code: &str) -> bool {
    !code.is_empty() && code.len() <= 128
}

fn context_valid(context: &str) -> bool {
    if context.is_empty() {
        return true;
    }
    chars(context).map(|list| !list.is_empty()).unwrap_or(false)
        && chars(context).map(|list| list.len()).unwrap_or(usize::MAX) <= 2
}

fn event_valid(e: &Event) -> bool {
    mode_valid(&e.mode)
        && code_valid(&e.code)
        && static_text(&e.text)
        && context_valid(&e.context)
        && e.time >= 0.0
}

fn decay(weight: f64, delta: f64) -> f64 {
    weight * 2f64.powf(-delta.max(0.0) / HALF_LIFE)
}

fn exact_score(weight: f64) -> f64 {
    (9.0 + 2.0 * 0.001f64.max(weight).ln()).clamp(0.0, 16.0)
}

fn general_of(text: &str, count: f64, contexts: f64, weight: f64) -> f64 {
    let letters = chars(text).unwrap_or_default();
    if letters.len() > 1 && count >= 3.0 && contexts >= 2.0 {
        2.0 * 1.0f64.min(weight / 3.0)
    } else {
        0.0
    }
}

fn score_of(summary: Option<&Summary>, context: &str) -> f64 {
    match summary {
        None => 0.0,
        Some(summary) => summary
            .general
            .max(summary.exact.get(context).copied().unwrap_or(0.0)),
    }
}

/// 参照 `M.build` 的事件过滤。
fn build_valid(e: &Event) -> bool {
    event_valid(e)
}

fn append_group(partition: &mut HashMap<String, Group>, e: &Event, now: f64) {
    let k = key(&[&e.mode, &e.context]);
    let old = partition.remove(&k);
    let mut group = Group {
        code: e.code.clone(),
        mode: e.mode.clone(),
        context: e.context.clone(),
        choices: HashMap::new(),
    };
    let time = now.min(e.time);
    if let Some(old) = old {
        for (text, choice) in old.choices {
            let weight =
                decay(choice.weight, time - choice.time) * if text != e.text { 0.25 } else { 1.0 };
            group.choices.insert(
                text,
                Choice {
                    weight,
                    count: choice.count,
                    time: time.max(choice.time),
                },
            );
        }
    }
    let choice = group.choices.entry(e.text.clone()).or_insert(Choice {
        weight: 0.0,
        count: 0.0,
        time,
    });
    choice.weight = 3.5f64.exp().min(choice.weight + 1.0);
    choice.count = 3.0f64.min(choice.count + 1.0);
    partition.insert(k, group);
}

impl LearningIndex {
    fn empty() -> Self {
        Self {
            codes: Vec::new(),
            now: 0.0,
            future: 0.0,
            partitions: None,
            exact: None,
            prefixes: None,
            cache: Fifo::new(MATERIALIZED_CODE_LIMIT),
            prefix_queries: Fifo::new(PREFIX_QUERY_LIMIT),
            code_windows: Fifo::new(CODE_WINDOW_LIMIT),
        }
    }

    /// 参照 `M.build`：独立的全量重放 oracle。
    pub fn build(events: &[Event], now: f64) -> Self {
        let mut groups: HashMap<String, Group> = HashMap::new();
        for e in events {
            if !build_valid(e) {
                continue;
            }
            let k = key(&[&e.code, &e.mode, &e.context]);
            let group = groups.entry(k).or_insert_with(|| Group {
                code: e.code.clone(),
                mode: e.mode.clone(),
                context: e.context.clone(),
                choices: HashMap::new(),
            });
            let time = now.min(e.time);
            for (text, choice) in group.choices.iter_mut() {
                choice.weight = decay(choice.weight, time - choice.time);
                choice.time = time.max(choice.time);
                if *text != e.text {
                    choice.weight *= 0.25;
                }
            }
            let choice = group.choices.entry(e.text.clone()).or_insert(Choice {
                weight: 0.0,
                count: 0.0,
                time,
            });
            choice.weight = 3.5f64.exp().min(choice.weight + 1.0);
            choice.count = 3.0f64.min(choice.count + 1.0);
        }

        let mut summaries: HashMap<String, Summary> = HashMap::new();
        for group in groups.values() {
            for (text, choice) in &group.choices {
                let k = key(&[&group.code, &group.mode, text]);
                let summary = summaries.entry(k).or_insert_with(|| Summary {
                    mode: group.mode.clone(),
                    text: text.clone(),
                    exact: HashMap::new(),
                    weight: 0.0,
                    count: 0.0,
                    contexts: 0.0,
                    general: 0.0,
                });
                let weight = decay(choice.weight, now - choice.time);
                summary
                    .exact
                    .insert(group.context.clone(), exact_score(weight));
                summary.weight += weight;
                summary.count = 3.0f64.min(summary.count + choice.count);
                if !group.context.is_empty() && weight >= 0.1 {
                    summary.contexts += 1.0;
                }
            }
        }

        let mut exact: HashMap<String, Summary> = HashMap::new();
        let mut codes: Vec<String> = Vec::new();
        let mut seen: HashSet<String> = HashSet::new();
        let mut prefixes: HashMap<String, PrefixEntry> = HashMap::new();
        for (k, mut summary) in summaries {
            summary.general = general_of(
                &summary.text,
                summary.count,
                summary.contexts,
                summary.weight,
            );
            let code = summary_code(&k);
            if seen.insert(code.clone()) {
                codes.push(code);
            }
            exact.insert(k, summary);
        }
        for (k, summary) in &exact {
            let code = summary_code(k);
            let letters = chars(&summary.text).unwrap_or_default();
            let mut prefix = String::new();
            for letter in letters.iter().take(letters.len().saturating_sub(1)) {
                prefix.push(*letter);
                let pk = key(&[&code, &summary.mode, &prefix]);
                let entry = prefixes.entry(pk).or_insert_with(|| PrefixEntry {
                    general: 0.0,
                    exact: HashMap::new(),
                });
                entry.general = entry.general.max(summary.general);
                for (ctx, value) in &summary.exact {
                    let slot = entry.exact.entry(ctx.clone()).or_insert(0.0);
                    *slot = slot.max(*value);
                }
            }
        }
        codes.sort();
        Self {
            codes,
            now,
            future: 0.0,
            partitions: None,
            exact: Some(exact),
            prefixes: Some(prefixes),
            cache: Fifo::new(MATERIALIZED_CODE_LIMIT),
            prefix_queries: Fifo::new(PREFIX_QUERY_LIMIT),
            code_windows: Fifo::new(CODE_WINDOW_LIMIT),
        }
    }

    /// 参照 `M.runtime_index`。
    pub fn runtime(events: &[Event], now: f64) -> Self {
        let mut index = Self::empty();
        index.now = now;
        let mut partitions: HashMap<String, HashMap<String, Group>> = HashMap::new();
        let mut codes: Vec<String> = Vec::new();
        for e in events {
            if !event_valid(e) {
                continue;
            }
            let partition = partitions.entry(e.code.clone()).or_default();
            if !codes.contains(&e.code) {
                codes.push(e.code.clone());
            }
            append_group(partition, e, now);
            index.future = index.future.max(e.time);
        }
        codes.sort();
        index.codes = codes;
        index.partitions = Some(partitions);
        index
    }

    /// 参照 `update_index`：仅复制受影响的 code 分区。
    pub fn update(&self, accepted: &[Event], all_events: &[Event], now: f64) -> Self {
        let Some(partitions) = &self.partitions else {
            return Self::runtime(all_events, now);
        };
        if now < self.now || self.future > self.now {
            return Self::runtime(all_events, now);
        }
        let mut next_partitions = partitions.clone();
        let mut changed: HashSet<String> = HashSet::new();
        let mut new_codes: Vec<String> = Vec::new();
        let mut future = self.future;
        for e in accepted {
            if !event_valid(e) {
                continue;
            }
            if !changed.contains(&e.code) {
                let existing = next_partitions.get(&e.code).cloned();
                if existing.is_none() {
                    new_codes.push(e.code.clone());
                }
                next_partitions.insert(e.code.clone(), existing.unwrap_or_default());
                changed.insert(e.code.clone());
            }
            append_group(next_partitions.get_mut(&e.code).expect("inserted"), e, now);
            future = future.max(e.time);
        }
        let mut codes = self.codes.clone();
        for code in &new_codes {
            let position = codes.partition_point(|existing| existing < code);
            codes.insert(position, code.clone());
        }
        Self {
            codes,
            now,
            future,
            partitions: Some(next_partitions),
            exact: None,
            prefixes: None,
            cache: Fifo::new(MATERIALIZED_CODE_LIMIT),
            prefix_queries: Fifo::new(PREFIX_QUERY_LIMIT),
            code_windows: Fifo::new(CODE_WINDOW_LIMIT),
        }
    }

    /// 参照 `M.trim_caches`。
    pub fn trim_caches(&mut self) {
        if self.partitions.is_some() {
            self.cache.clear();
        }
        self.prefix_queries.clear();
        self.code_windows.clear();
    }

    fn materialized(&mut self, code: &str) -> Option<Rc<Materialized>> {
        let partitions = self.partitions.as_ref()?;
        if let Some(cached) = self.cache.get(&code.to_string()) {
            return Some(cached.clone());
        }
        let partition = partitions.get(code)?;
        let mut result = Materialized::default();
        for group in partition.values() {
            for (text, choice) in &group.choices {
                let k = key(&[code, &group.mode, text]);
                let summary = result.exact.entry(k).or_insert_with(|| Summary {
                    mode: group.mode.clone(),
                    text: text.clone(),
                    exact: HashMap::new(),
                    weight: 0.0,
                    count: 0.0,
                    contexts: 0.0,
                    general: 0.0,
                });
                let weight = decay(choice.weight, self.now - choice.time);
                summary
                    .exact
                    .insert(group.context.clone(), exact_score(weight));
                summary.weight += weight;
                summary.count = 3.0f64.min(summary.count + choice.count);
                if !group.context.is_empty() && weight >= 0.1 {
                    summary.contexts += 1.0;
                }
            }
        }
        for summary in result.exact.values_mut() {
            summary.general = general_of(
                &summary.text,
                summary.count,
                summary.contexts,
                summary.weight,
            );
        }
        for summary in result.exact.values() {
            let letters = chars(&summary.text).unwrap_or_default();
            let mut prefix = String::new();
            for letter in letters.iter().take(letters.len().saturating_sub(1)) {
                prefix.push(*letter);
                let pk = key(&[code, &summary.mode, &prefix]);
                let entry = result.prefixes.entry(pk).or_insert_with(|| PrefixEntry {
                    general: 0.0,
                    exact: HashMap::new(),
                });
                entry.general = entry.general.max(summary.general);
                for (ctx, value) in &summary.exact {
                    let slot = entry.exact.entry(ctx.clone()).or_insert(0.0);
                    *slot = slot.max(*value);
                }
            }
        }
        let shared = Rc::new(result);
        self.cache.put(code.to_string(), shared.clone());
        Some(shared)
    }

    /// 参照 `M.score`。
    pub fn score(&mut self, mode: &str, code: &str, text: &str, context: &str) -> f64 {
        let k = key(&[code, mode, text]);
        if let Some(exact) = &self.exact {
            return score_of(exact.get(&k), context);
        }
        match self.materialized(code) {
            None => 0.0,
            Some(materialized) => score_of(materialized.exact.get(&k), context),
        }
    }

    /// `code_window`：返回 `codes` 中 [first, last] 闭区间（0 基；last 可为 -1）。
    fn code_window(&mut self, code: &str) -> (usize, isize) {
        if let Some(&bounds) = self.code_windows.get(&code.to_string()) {
            return bounds;
        }
        let first = self
            .codes
            .partition_point(|existing| existing.as_str() < code);
        let mut last = first as isize - 1;
        let end = (first + CODE_WINDOW_SLOTS).min(self.codes.len());
        for candidate in self.codes.iter().take(end).skip(first) {
            if !candidate.starts_with(code) {
                break;
            }
            last += 1;
        }
        let bounds = (first, last);
        self.code_windows.put(code.to_string(), bounds);
        bounds
    }

    fn lookup_prefix(&mut self, code: &str, mode: &str, text: &str, context: &str) -> f64 {
        let pk = key(&[code, mode, text]);
        let entry = if let Some(prefixes) = &self.prefixes {
            prefixes.get(&pk).cloned()
        } else {
            self.materialized(code)
                .and_then(|materialized| materialized.prefixes.get(&pk).cloned())
        };
        entry
            .map(|entry| {
                entry
                    .general
                    .max(entry.exact.get(context).copied().unwrap_or(0.0))
            })
            .unwrap_or(0.0)
    }

    /// 参照 `M.prefix_score`。
    pub fn prefix_score(&mut self, mode: &str, code: &str, text: &str, context: &str) -> f64 {
        if code.is_empty() || text.is_empty() {
            return 0.0;
        }
        let (first, last) = self.code_window(code);
        if last < 0 || first as isize > last {
            return 0.0;
        }
        let query = key(&[mode, code, text, context]);
        if let Some(cached) = self.prefix_queries.get(&query) {
            return *cached;
        }
        let candidates: Vec<String> = self.codes[first..=last as usize].to_vec();
        let mut best = 0.0f64;
        for candidate in candidates {
            if candidate.len() > code.len() {
                best = best.max(self.lookup_prefix(&candidate, mode, text, context));
            }
        }
        self.prefix_queries.put(query, best);
        best
    }
}

fn summary_code(summary_key: &str) -> String {
    match unframe(summary_key) {
        Some(parts) if !parts.is_empty() => parts[0].clone(),
        _ => String::new(),
    }
}

/// 参照 `M.reward`：返回 `(best, potential)`。
pub fn reward(
    index: &mut LearningIndex,
    mode: &str,
    raw: &[u8],
    text: &str,
    finish: usize,
    chain: &[RewardNode],
) -> (f64, f64) {
    let mut best = chain.first().map(|node| node.learning_score).unwrap_or(0.0);
    let mut potential = 0.0f64;
    if index.codes.is_empty() || mode.is_empty() {
        return (best, potential);
    }
    // 参照以 `while true` 遍历：链走完后还会以 nil start（t=0, r=0）再处理一轮，
    // 等价于对整串做一次根节点评分。
    let mut position = 0usize;
    loop {
        let (node, t, r, node_score) = match chain.get(position) {
            Some(node) => (true, node.text_length, node.raw_length, node.learning_score),
            None => (false, 0, 0, 0.0),
        };
        let fragment = text.get(t..).unwrap_or("");
        if character_count(fragment) > 16 {
            break;
        }
        // 参照 `raw:sub(r + 1, finish)`：两端都做 Lua 式截断。
        let start = r.min(raw.len());
        let end = finish.min(raw.len());
        let code = if start < end {
            std::str::from_utf8(&raw[start..end]).unwrap_or("")
        } else {
            ""
        };
        // 参照 `text:sub(1, t)`：末端截断到文本长度。
        let prefix = text.get(..t.min(text.len())).unwrap_or("");
        let ctx = context(prefix);
        best = best.max(node_score + index.score(mode, code, fragment, &ctx));
        potential = potential.max(index.prefix_score(mode, code, fragment, &ctx));
        if !node || r == 0 {
            break;
        }
        position += 1;
    }
    (best, potential)
}

/// 参照 `M.diff`（`time` 字段不在比对范围）。
pub fn diff(
    raw: &[u8],
    before: Option<&DiffItem>,
    selected: Option<&DiffItem>,
    floor: usize,
    mode: &str,
) -> Vec<DiffEvent> {
    let (Some(before), Some(selected)) = (before, selected) else {
        return Vec::new();
    };
    if before.text == selected.text {
        return Vec::new();
    }
    let Some((a, ends)) = boundaries(before, raw) else {
        return Vec::new();
    };
    let Some((b, _)) = boundaries(selected, raw) else {
        return Vec::new();
    };
    let mut result = Vec::new();
    let mut first = 0usize;
    for &last in &ends {
        if let Some(&b_last) = b.get(&last) {
            let b_first = b.get(&first).copied().unwrap_or(0);
            let a_first = a.get(&first).copied().unwrap_or(0);
            let a_last = a.get(&last).copied().unwrap_or(0);
            let text = slice(&selected.text, b_first, b_last);
            if first >= floor && text != slice(&before.text, a_first, a_last) && static_text(&text)
            {
                let code = String::from_utf8_lossy(&raw[first..last]).to_ascii_lowercase();
                result.push(DiffEvent {
                    mode: mode.to_string(),
                    code,
                    text,
                    context: context(&selected.text[..b_first]),
                    raw_start: first,
                    raw_end: last,
                    text_start: b_first,
                    text_end: b_last,
                });
            }
            first = last;
        }
    }
    result
}

fn slice(text: &str, start: usize, end: usize) -> String {
    text.get(start..end).unwrap_or("").to_string()
}

fn boundaries(item: &DiffItem, raw: &[u8]) -> Option<(HashMap<usize, usize>, Vec<usize>)> {
    let mut map: HashMap<usize, usize> = HashMap::new();
    map.insert(0, 0);
    let mut ends = Vec::new();
    for node in &item.path {
        if node.raw_length > 0 {
            map.insert(node.raw_length, node.text_length);
            ends.push(node.raw_length);
        }
    }
    ends.sort_unstable();
    let (mut r, mut t) = (0usize, 0usize);
    for &last in &ends {
        let mapped = *map.get(&last)?;
        if last <= r || mapped <= t || mapped > item.text.len() {
            return None;
        }
        r = last;
        t = mapped;
    }
    if r != raw.len() || t != item.text.len() {
        return None;
    }
    Some((map, ends))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_matches_reference_vectors() {
        // 取自参照实现（也是线上学习库名后缀的来源）。
        assert_eq!(hash(""), "811c9dc500001505");
        assert_eq!(hash("tiger_sentence"), "f2d1c028532c0d94");
        assert_eq!(hash("虎整句"), "d81047ce8c7a3a3c");
    }

    #[test]
    fn context_keeps_last_two_characters() {
        assert_eq!(context("甲"), "甲");
        assert_eq!(context("甲乙"), "甲乙");
        assert_eq!(context("甲乙丙"), "乙丙");
        assert_eq!(context(""), "");
    }

    #[test]
    fn utf8_validation_and_static() {
        assert!(chars("甲乙").is_some());
        assert!(chars("\u{fffd}").is_some());
        assert!(!static_text(""));
        assert!(static_text("甲"));
        assert!(!static_text(&"甲".repeat(17)));
        assert!(!static_text("甲{乙"));
        assert!(!static_text("\u{e000}")); // 私用区（参照排除）
    }

    #[test]
    fn frame_roundtrip_and_rejects() {
        let values = vec!["1".to_string(), "ab".to_string(), String::new()];
        assert_eq!(unframe(&frame(&values)), Some(values));
        assert_eq!(unframe("5:abc"), None);
        assert_eq!(unframe("8193:ab"), None);
        assert_eq!(unframe("ab"), None);
    }
}
