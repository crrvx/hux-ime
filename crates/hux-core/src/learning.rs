// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tab 纠错学习，对应参照 `lua/tiger_sentence_learning.lua` 的纯计算部分。
//!
//! 实现：`build`（全量重放 oracle）、`runtime`/`update`（运行时快照）、
//! `score`/`prefix_score`（含物化缓存）、`reward`（路径链）、`diff`、
//! `context`/`static_text`/`frame`/`unframe`/`hash`。
//! 持久化（LevelDB `open`/`confirm`）在平台层实现：见 `platform/fcitx5/src/learning_store.rs`。
//!
//! **人工纠错等级（`7b220ce` 起）**：事件只累加**离散等级**（每次确认 +1，上限 10），
//! 分数按等级取整（same-context `7+2L`、跨上下文 `4+2L`）；**不再按时间衰减**，
//! 时间戳只作持久化元数据。故浮点求和只发生在 `weight`（各上下文的整数等级之和）
//! 累加上，跨进程哈希序不改变结果。

use crate::cache::Fifo;
use hashbrown::{HashMap, HashSet};
use std::rc::Rc;

/// 参照 `MAX_LEVEL`：单条 `(code, mode, context)` 纠错的最大累积等级。
const MAX_LEVEL: f64 = 10.0;
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
    /// 累积纠错次数（等级），上限 `MAX_LEVEL`。
    weight: f64,
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
    pub text_length: usize,
    pub raw_length: usize,
    pub learning_score: f64,
    /// 参照 `learning_early_commit_bonus`（个性化早提交置信度的学习分量）。
    pub learning_early_commit_bonus: f64,
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

#[derive(Clone, Debug, PartialEq)]
pub struct DiffEvent {
    /// 参照 `diff` 内 `time=os.time()`；由调用方传入（金样不比对）。
    pub time: f64,
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
#[derive(Clone)]
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
pub fn character_count(text: &str) -> usize {
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

/// 参照 `unframe`：帧的**长度前缀是字节数**，逐段切出。
///
/// 越界与「落点不在 UTF-8 字符边界」都返回 `None`（参照 Lua 的 `sub` 同样不 panic）。
/// 本函数是坏值的唯一错值通道，而调用方在 `extern "C"` 的构造路径上（读持久化库），
/// 故**不得 panic**：坏帧只能是「跳过该条记录 + 诊断」（审计 F1）。
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
        // `get(a..b)` 同时兜住「越界」与「非字符边界」——`&value[a..b]` 在后者 panic。
        result.push(
            value
                .get(digit_end + 1..digit_end + 1 + length)?
                .to_string(),
        );
        position = digit_end + 1 + length;
    }
    Some(result)
}

/// 参照 `M.hash`：双累加器 FNV 变体，输出 `%08x%08x`（按字节，允许非 UTF-8 输入）。
pub fn hash_bytes(text: &[u8]) -> String {
    let mut a: u64 = 2166136261;
    let mut b: u64 = 5381;
    for byte in text {
        a = (a * 65599 + *byte as u64) % 4294967296;
        b = (b * 33 + *byte as u64) % 4294967296;
    }
    format!("{a:08x}{b:08x}")
}

/// 参照 `M.hash`。
pub fn hash(text: &str) -> String {
    hash_bytes(text.as_bytes())
}

/// 参照 `M.fusion_mode`：空模式串保持空（= 不学习），否则加 `fusion-v1|` 前缀。
pub fn fusion_mode(mode: &str) -> String {
    if mode.is_empty() {
        String::new()
    } else {
        format!("fusion-v1|{mode}")
    }
}

/// 参照 `M.fusion_pair_code`：成对偏好记录的键。
///
/// 原始输入按 `raw .. "\0D\0" .. direct .. "\0C\0" .. composed` 逐字节拼接后哈希；
/// `~f` 前缀把融合码移出 raw 码的前缀索引命名空间（虎码 raw 只含 `a-z`，而 `~` > `z`）。
pub fn fusion_pair_code(raw: &[u8], direct: &str, composed: &str) -> String {
    let mut framed = Vec::with_capacity(raw.len() + direct.len() + composed.len() + 6);
    framed.extend_from_slice(raw);
    framed.extend_from_slice(b"\0D\0");
    framed.extend_from_slice(direct.as_bytes());
    framed.extend_from_slice(b"\0C\0");
    framed.extend_from_slice(composed.as_bytes());
    format!("~f{}", hash_bytes(&framed))
}

/// 参照 `M.fusion_event`：成对偏好事件的构造（`time` 由调用方注入，参照取 `os.time()`）。
///
/// `direct_wins` 决定 `text` 取 `"D"`（Direct 胜）还是 `"C"`；`context` 恒为空串
/// （只命中 `exact[""]`）；`raw_end` 只用于提交点筛选，不落库。`mode == ""` → `None`。
pub fn fusion_event(
    mode: &str,
    raw: &[u8],
    direct: &str,
    composed: &str,
    direct_wins: bool,
    raw_end: usize,
    now: f64,
) -> Option<DiffEvent> {
    if mode.is_empty() {
        return None;
    }
    Some(DiffEvent {
        time: now,
        mode: fusion_mode(mode),
        code: fusion_pair_code(raw, direct, composed),
        text: if direct_wins { "D" } else { "C" }.to_string(),
        context: String::new(),
        raw_start: 0,
        raw_end,
        text_start: 0,
        text_end: 1,
    })
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

/// 参照 `correction_level`：等级 = 向下取整的累积权重，钳到 `0..=MAX_LEVEL`。
fn correction_level(weight: f64) -> f64 {
    (weight + 1e-12).floor().clamp(0.0, MAX_LEVEL)
}

/// 参照 `exact_score`：同上下文纠错等级分（L1=9 … L10=27）。
fn exact_score(weight: f64) -> f64 {
    let level = correction_level(weight);
    if level > 0.0 { 7.0 + 2.0 * level } else { 0.0 }
}

/// 参照 `general_score`：跨上下文纠错等级分（L1=6 … L10=24）。
fn general_of(weight: f64) -> f64 {
    let level = correction_level(weight);
    if level > 0.0 { 4.0 + 2.0 * level } else { 0.0 }
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

/// 参照 `append_group`：把一条事件并入分区（同组竞争项各 ×0.25，命中项等级 +1）。
fn append_group(partition: &mut HashMap<String, Group>, e: &Event) {
    let k = key(&[&e.mode, &e.context]);
    let old = partition.remove(&k);
    let mut group = Group {
        code: e.code.clone(),
        mode: e.mode.clone(),
        context: e.context.clone(),
        choices: HashMap::new(),
    };
    if let Some(old) = old {
        for (text, choice) in old.choices {
            let weight = choice.weight * if text != e.text { 0.25 } else { 1.0 };
            group.choices.insert(text, Choice { weight });
        }
    }
    let choice = group
        .choices
        .entry(e.text.clone())
        .or_insert(Choice { weight: 0.0 });
    choice.weight = MAX_LEVEL.min(choice.weight + 1.0);
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
    ///
    /// `now` 只写入 `index.now`（参照字段仍在，但学习不再随时间衰减；`M.build`
    /// 的同名参数如今也不再参与计算）。
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
            for (text, choice) in group.choices.iter_mut() {
                if *text != e.text {
                    choice.weight *= 0.25;
                }
            }
            let choice = group
                .choices
                .entry(e.text.clone())
                .or_insert(Choice { weight: 0.0 });
            choice.weight = MAX_LEVEL.min(choice.weight + 1.0);
        }

        let mut summaries: HashMap<String, Summary> = HashMap::new();
        // 汇总为浮点累加，顺序需确定（哈希序跨进程不定）：group 键排序后遍历。
        let mut group_keys: Vec<&String> = groups.keys().collect();
        group_keys.sort();
        for group_key in group_keys {
            let group = &groups[group_key];
            for (text, choice) in &group.choices {
                let k = key(&[&group.code, &group.mode, text]);
                let summary = summaries.entry(k).or_insert_with(|| Summary {
                    mode: group.mode.clone(),
                    text: text.clone(),
                    exact: HashMap::new(),
                    weight: 0.0,
                    general: 0.0,
                });
                summary
                    .exact
                    .insert(group.context.clone(), exact_score(choice.weight));
                summary.weight += choice.weight;
            }
        }

        let mut exact: HashMap<String, Summary> = HashMap::new();
        let mut codes: Vec<String> = Vec::new();
        let mut seen: HashSet<String> = HashSet::new();
        let mut prefixes: HashMap<String, PrefixEntry> = HashMap::new();
        for (k, mut summary) in summaries {
            summary.general = general_of(summary.weight);
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
    ///
    /// `now` 与 `future` 仍按参照写入（`future` 记录过最大事件时间），但学习
    /// 不再随「当前时间相对事件时间」衰减，故二者只作元数据。
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
            append_group(partition, e);
            index.future = index.future.max(e.time);
        }
        codes.sort();
        index.codes = codes;
        index.partitions = Some(partitions);
        index
    }

    /// 参照 `update_index`：重建受影响的 code 分区。
    ///
    /// 无时间衰减后，`7b220ce` 删除了「时钟回退/未来事件 ⇒ 全量重放」的判据，
    /// 也删除了逐事件的 `future` 更新（`future` 原样带过）。
    /// 注意：`partitions.clone()` 为整体深拷贝（参照的 `copy` 只复制外层表），
    /// 单次确认代价 O(历史规模)；如需优化可改为共享分区（性能项，K3 复核）。
    pub fn update(&self, accepted: &[Event], all_events: &[Event], now: f64) -> Self {
        let Some(partitions) = &self.partitions else {
            return Self::runtime(all_events, now);
        };
        let mut next_partitions = partitions.clone();
        let mut changed: HashSet<String> = HashSet::new();
        let mut new_codes: Vec<String> = Vec::new();
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
            append_group(next_partitions.get_mut(&e.code).expect("inserted"), e);
        }
        let mut codes = self.codes.clone();
        for code in &new_codes {
            let position = codes.partition_point(|existing| existing < code);
            codes.insert(position, code.clone());
        }
        Self {
            codes,
            now,
            future: self.future,
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
        // 同 `build`：累加顺序需确定（哈希序跨进程不定）。
        let mut group_keys: Vec<&String> = partition.keys().collect();
        group_keys.sort();
        for group_key in group_keys {
            let group = &partition[group_key];
            for (text, choice) in &group.choices {
                let k = key(&[code, &group.mode, text]);
                let summary = result.exact.entry(k).or_insert_with(|| Summary {
                    mode: group.mode.clone(),
                    text: text.clone(),
                    exact: HashMap::new(),
                    weight: 0.0,
                    general: 0.0,
                });
                summary
                    .exact
                    .insert(group.context.clone(), exact_score(choice.weight));
                summary.weight += choice.weight;
            }
        }
        for summary in result.exact.values_mut() {
            summary.general = general_of(summary.weight);
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

    /// 参照 `M.fusion_score`：同一 `(raw, direct, composed)` 三元组内 `D` 与 `C` 的得分差。
    ///
    /// 入参 `mode` 是**实时模式**（内部自行加 `fusion-v1|` 前缀）；空模式一律 0
    /// （参照的「索引为 nil」在 Rust 由调用侧用 `Option` 表达，空索引本身也返回 0）。
    pub fn fusion_score(&mut self, mode: &str, raw: &[u8], direct: &str, composed: &str) -> f64 {
        if mode.is_empty() {
            return 0.0;
        }
        let fusion = fusion_mode(mode);
        let code = fusion_pair_code(raw, direct, composed);
        self.score(&fusion, &code, "D", "") - self.score(&fusion, &code, "C", "")
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

/// 参照 `M.early_commit_maturity`：把纠错等级分映射到 `0..1` 的成熟度。
///
/// `7b220ce` 起等级是离散的：`9`（L1，首次同上下文纠错）→ 0、
/// `11`（L2）→ 0.5、`13`（L3 及以上）→ 1；不再是 `exp` 连续曲线。
pub fn early_commit_maturity(score: f64) -> f64 {
    ((score - 9.0) / 4.0).clamp(0.0, 1.0)
}

/// 参照 `M.early_commit_contribution`：单条学习奖励对早提交置信度的有界贡献。
pub fn early_commit_contribution(score: f64) -> f64 {
    (score.max(0.0) * early_commit_maturity(score) * 0.075).min(0.75)
}

/// 参照 `M.reward`：返回 `(best, potential, early_bonus)`。
pub fn reward(
    index: &mut LearningIndex,
    mode: &str,
    raw: &[u8],
    text: &str,
    finish: usize,
    chain: &[RewardNode],
) -> (f64, f64, f64) {
    let seed = chain.first();
    let mut best = seed.map(|node| node.learning_score).unwrap_or(0.0);
    let mut potential = 0.0f64;
    let mut early_bonus = seed
        .map(|node| node.learning_early_commit_bonus)
        .unwrap_or(0.0);
    // 参照 `previous.learning_early_commit_bonus`：整轮迭代都取**链首**（种子）的奖励，
    // 而不是当前 `start` 节点的。
    let seed_bonus = early_bonus;
    if index.codes.is_empty() || mode.is_empty() {
        return (best, potential, early_bonus);
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
        let reward = index.score(mode, code, fragment, &ctx);
        let candidate = node_score + reward;
        let candidate_bonus = seed_bonus.max(early_commit_contribution(reward));
        if candidate > best || (candidate == best && candidate_bonus > early_bonus) {
            best = candidate;
        }
        early_bonus = early_bonus.max(candidate_bonus);
        potential = potential.max(index.prefix_score(mode, code, fragment, &ctx));
        if !node || r == 0 {
            break;
        }
        position += 1;
    }
    (best, potential, early_bonus)
}

/// 参照 `M.diff`（`time` 由调用方传入，金样不比对）。
pub fn diff(
    raw: &[u8],
    before: Option<&DiffItem>,
    selected: Option<&DiffItem>,
    floor: usize,
    mode: &str,
    now: f64,
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
                    time: now,
                    mode: mode.to_string(),
                    code,
                    text,
                    context: context(selected.text.get(..b_first).unwrap_or("")),
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
        assert_eq!(hash("虎句"), "2b025b23302e3acd");
        assert_eq!(hash_bytes(b""), hash(""));
    }

    #[test]
    fn fusion_keys_match_reference_vectors() {
        // 向量取自参照 `lua/tiger_sentence_learning.lua` @ d7b01e5 直算
        // （`hash`/`fusion_*` 在 bd83900..d7b01e5 区间内未改动，与 b2bbd23 逐字节相同）。
        assert_eq!(fusion_mode(""), "");
        assert_eq!(fusion_mode("m"), "fusion-v1|m");
        assert_eq!(
            fusion_pair_code(b"ii", "C", "A"),
            "~f422122101c436982",
            "`raw \"\\0D\\0\" direct \"\\0C\\0\" composed` 的哈希 + `~f` 前缀"
        );
        assert_eq!(
            fusion_pair_code(b"", "", ""),
            "~f4576f3060cad8fac",
            "空组件按 `a or \"\"` 处理"
        );
        assert_eq!(
            fusion_pair_code(b"ab", "疒否", "交否"),
            "~f39bc299c6ab38db6"
        );
        // 融合码在 `codes` 中排在全部 raw 码之后（`~` > `z`）：命名空间隔离的前提。
        assert!("~f422122101c436982" > "zzzz");
    }

    #[test]
    fn fusion_score_is_pairwise_difference() {
        // 提交点把 `DiffEvent` 投影成落库的 `Event`（丢弃只用于筛选的偏移）。
        let persisted = |direct: &str| {
            let event = fusion_event("m", b"ii", direct, "A", true, 2, 1000.0).expect("事件");
            Event {
                time: event.time,
                mode: event.mode,
                code: event.code,
                text: event.text,
                context: event.context,
            }
        };
        let events = vec![persisted("C"), persisted("B")];
        let mut index = LearningIndex::build(&events, 1000.0);
        // 单笔确认权重 1 → 9；未记录的 pair 恒 0。
        assert_eq!(index.fusion_score("m", b"ii", "C", "A"), 9.0);
        assert_eq!(index.fusion_score("m", b"ii", "B", "A"), 9.0);
        assert_eq!(index.fusion_score("m", b"ii", "A", "C"), 0.0);
        assert_eq!(index.fusion_score("m", b"ii", "Z", "A"), 0.0);
        // 空模式 = 不学习。
        assert_eq!(index.fusion_score("", b"ii", "C", "A"), 0.0);
    }

    #[test]
    fn fusion_event_encodes_direction_and_offsets() {
        assert!(fusion_event("", b"ii", "C", "A", true, 2, 0.0).is_none());
        let event = fusion_event("m", b"ii", "C", "A", true, 2, 1700000000.0).expect("事件");
        assert_eq!(event.time, 1700000000.0);
        assert_eq!(event.mode, "fusion-v1|m");
        assert_eq!(event.code, "~f422122101c436982");
        assert_eq!(event.text, "D");
        assert_eq!(event.context, "");
        assert_eq!(
            (
                event.raw_start,
                event.raw_end,
                event.text_start,
                event.text_end
            ),
            (0, 2, 0, 1)
        );
        let reversed = fusion_event("m", b"ii", "C", "A", false, 2, 0.0).expect("事件");
        assert_eq!(reversed.text, "C");
    }

    #[test]
    fn context_keeps_last_two_characters() {
        assert_eq!(context("甲"), "甲");
        assert_eq!(context("甲乙"), "甲乙");
        assert_eq!(context("甲乙丙"), "乙丙");
        assert_eq!(context(""), "");
    }

    #[test]
    fn chars_validates_utf8_tags() {
        assert!(chars("甲乙").is_some());
        assert!(chars("\u{fffd}").is_some());
    }

    #[test]
    fn static_text_constrains_tags() {
        assert!(!static_text(""));
        assert!(static_text("甲"));
        assert!(!static_text(&"甲".repeat(17)));
        assert!(!static_text("甲{乙"));
        assert!(!static_text("\u{e000}")); // 私用区（参照排除）
    }

    #[test]
    fn frame_roundtrip() {
        let values = vec!["1".to_string(), "ab".to_string(), String::new()];
        assert_eq!(unframe(&frame(&values)), Some(values));
    }

    #[test]
    fn early_commit_maturity_maps_correction_levels() {
        // 取自参照测试（`7b220ce` 后）：L1/L2/L3 次同上下文纠错 → 0 / 0.5 / 1。
        assert_eq!(early_commit_maturity(9.0), 0.0);
        assert_eq!(early_commit_maturity(8.0), 0.0);
        assert_eq!(early_commit_maturity(11.0), 0.5);
        assert_eq!(early_commit_maturity(13.0), 1.0);
        assert_eq!(early_commit_maturity(100.0), 1.0);
    }

    #[test]
    fn early_commit_contribution_is_bounded() {
        let close = |left: f64, right: f64| (left - right).abs() < 1e-12;
        assert_eq!(early_commit_contribution(0.0), 0.0);
        assert_eq!(early_commit_contribution(-5.0), 0.0);
        assert_eq!(early_commit_contribution(9.0), 0.0);
        // 未成熟的观测只给部分贡献；成熟后 saturate 到上限 0.75。
        assert!(close(early_commit_contribution(10.0), 0.187_5));
        // 等级分 12（成熟度 0.75）尚未到上限；13（成熟度 1）才封顶。
        assert!(close(early_commit_contribution(12.0), 0.675));
        assert_eq!(early_commit_contribution(13.0), 0.75);
        assert_eq!(early_commit_contribution(20.0), 0.75);
    }

    /// 参照 `tools/test_sentence_learning.lua` @ d7b01e5 的等级语义断言。
    #[test]
    fn correction_levels_advance_two_points_and_cap_at_ten() {
        let event = |code: &str, text: &str, context: &str| Event {
            time: 1000.0,
            mode: "test".to_string(),
            code: code.to_string(),
            text: text.to_string(),
            context: context.to_string(),
        };
        // 单次确认 = 同上下文 L1 = 9；跨上下文 L1 = 6；模式隔离。
        let mut single = LearningIndex::build(&[event("ab", "疒", "")], 1000.0);
        assert_eq!(single.score("test", "ab", "疒", ""), 9.0);
        assert_eq!(single.score("test", "ab", "疒", "甲"), 6.0);
        assert_eq!(single.score("other", "ab", "疒", ""), 0.0);
        // 无时间衰减：时间推后 10 年分值不变。
        let mut aged = LearningIndex::build(&[event("ab", "疒", "")], 1000.0 + 3650.0 * 86400.0);
        assert_eq!(aged.score("test", "ab", "疒", ""), 9.0);
        // 每次确认 +1 级、等级分 +2，第 10 级封顶（同上下文 27 / 跨上下文 24）。
        let mut repeated = Vec::new();
        for i in 1..=40 {
            repeated.push(event("ab", "疒", ""));
            let level = 10.0f64.min(i as f64);
            let mut index = LearningIndex::build(&repeated, 1000.0);
            assert_eq!(
                index.score("test", "ab", "疒", ""),
                7.0 + 2.0 * level,
                "repeated corrections at {i}"
            );
            assert_eq!(
                index.score("test", "ab", "疒", "其他"),
                4.0 + 2.0 * level,
                "cross-context score at {i}"
            );
        }
        let mut capped = LearningIndex::build(&repeated, 1000.0);
        assert_eq!(capped.score("test", "ab", "疒", "其他"), 24.0);
        // 三次确认跨上下文达 L3 = 10；手工竞争纠错把旧选择等级归零（无时间衰减）。
        let mut competing = vec![
            event("ab", "甲乙", "前"),
            event("ab", "甲乙", "后"),
            event("ab", "甲乙", "后"),
        ];
        let mut index = LearningIndex::build(&competing, 1000.0);
        assert_eq!(index.score("test", "ab", "甲乙", "新"), 10.0);
        competing.push(event("ab", "甲丙", "后"));
        let mut demoted = LearningIndex::build(&competing, 1000.0);
        assert_eq!(demoted.score("test", "ab", "甲乙", "后"), 6.0);
    }

    #[test]
    fn unframe_rejects_bad_input() {
        assert_eq!(unframe("5:abc"), None);
        assert_eq!(unframe("8193:ab"), None);
        assert_eq!(unframe("ab"), None);
    }

    /// 长度前缀合法但落点**不在字符边界**：`&str` 的字节切片会 panic（`Option` 签名承诺不 panic）。
    ///
    /// 生产触发面：`platform/fcitx5/src/learning_store.rs` 把 LevelDB 的任意值经
    /// `String::from_utf8_lossy` 交给本函数（非法 UTF-8 换成 U+FFFD 后长度错位），
    /// 且发生在 `hux_engine_new`（`extern "C"`）⇒ 坏库会让 addon 加载即 abort（审计 core F1 / 平台 F3）。
    #[test]
    fn unframe_rejects_non_char_boundary_slices() {
        // "1:é"：`é` 占 2 字节，长度 1 的切片正好落在其内部。
        assert_eq!(unframe("1:é"), None);
        // 尾段落点不在边界（前段良构）：先切出 "1:a"，再对 `é` 切 1 字节。
        assert_eq!(unframe("3:1:a1:é"), None);
        // lossy 替换后的形态（非法 UTF-8 → U+FFFD，3 字节）同样只是坏帧，不 panic。
        let lossy = String::from_utf8_lossy(b"2:\xff\xfe").to_string();
        assert_eq!(unframe(&lossy), None);
        // 良构帧不受影响（含多字节字符的正常切分）。
        assert_eq!(
            unframe("1:a2:é"),
            Some(vec!["a".to_string(), "é".to_string()])
        );
    }
}
