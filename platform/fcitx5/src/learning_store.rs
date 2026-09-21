// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! 学习库（参照 `tiger_sentence_learning.lua` 的 `M.open`/`M.confirm`/`M.refresh_scores`）：
//! `<user dir>/<name>.userdb/`（LevelDB，键 `e/%010d`、值 = frame 五元组）；
//! 上限 1 万条 / 16 MiB。`7b220ce` 起学习**不再随时间衰减**，故 `refresh_scores`
//! 不再重建索引（参照同函数直接返回 `store.index`）。

use std::path::Path;

use hux_core::learning::{self, Event, LearningIndex};
use rusty_leveldb::{DB, LdbIterator, Options};

/// 事件上限（参照 `store.count >= 10000`）。
pub const MAX_COUNT: usize = 10000;
/// 字节上限（参照 `16 * 1024 * 1024`）。
pub const MAX_BYTES: usize = 16 * 1024 * 1024;

const EVENT_PREFIX: &str = "e/";

/// 学习库名（参照 `"tiger_sentence_learning_" .. learning.hash(schema_id)`）。
pub fn store_name(schema_id: &str) -> String {
    format!("tiger_sentence_learning_{}", learning::hash(schema_id))
}

/// Lua `tostring(number)` 的常用等价（整数值 → 定点 + 保证小数点，如 `1726512345.0`）。
pub fn lua_number(value: f64) -> String {
    if value.is_finite() && value.fract() == 0.0 {
        format!("{value:.1}")
    } else {
        format!("{value}")
    }
}

/// 学习库状态（参照 `store` 表）。
pub struct LearningStore {
    pub name: String,
    /// 数据库句柄（打不开即 `None`，`error` 记原因）。
    db: Option<DB>,
    /// 全量事件（参照 `store.events`）。
    pub events: Vec<Event>,
    index: LearningIndex,
    /// 当前事件数 / 键序号 / 字节数（参照 `count`/`sequence`/`bytes`）。
    pub count: usize,
    pub sequence: u64,
    pub bytes: usize,
    /// 上次打分时间（参照 `scored_at`）。
    pub scored_at: f64,
    /// 打开/写入错误（参照 `error`）。
    pub error: Option<String>,
    /// 索引版本（每次索引变化递增；宿主据此重设 decoder 学习）。
    index_version: u64,
}

impl LearningStore {
    /// 未启用（用户目录不可用/LevelDb 不可用）的占位存储。
    pub fn disabled(reason: &str) -> Self {
        Self {
            name: String::new(),
            db: None,
            events: Vec::new(),
            index: LearningIndex::runtime(&[], 0.0),
            count: 0,
            sequence: 0,
            bytes: 0,
            scored_at: 0.0,
            error: Some(reason.to_string()),
            index_version: 0,
        }
    }

    /// 参照 `M.open`：打开数据库、加载 `e/` 事件、构建运行时索引。
    pub fn open(user_dir: &Path, name: &str, now: f64) -> Self {
        let mut store = Self {
            name: name.to_string(),
            db: None,
            events: Vec::new(),
            index: LearningIndex::runtime(&[], now),
            count: 0,
            sequence: 0,
            bytes: 0,
            scored_at: now,
            error: None,
            index_version: 0,
        };
        let path = user_dir.join(format!("{name}.userdb"));
        // 用户目录可能尚不存在（参照的 rime 用户目录总是由框架创建）。
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        // rusty-leveldb 默认 `create_if_missing`。
        let mut db = match DB::open(&path, Options::default()) {
            Ok(db) => db,
            Err(error) => {
                store.error = Some(format!(
                    "learning database is locked or unavailable: {error}"
                ));
                return store;
            }
        };
        let mut events = Vec::new();
        let mut count = 0usize;
        let mut bytes = 0usize;
        let mut sequence = 0u64;
        let mut failure: Option<String> = None;
        match db.new_iter() {
            Ok(mut iterator) => {
                iterator.seek(EVENT_PREFIX.as_bytes());
                while iterator.valid() {
                    let Some((key, value)) = iterator.current() else {
                        break;
                    };
                    if !key.starts_with(EVENT_PREFIX.as_bytes()) {
                        break;
                    }
                    count += 1;
                    bytes += key.len() + value.len();
                    sequence = sequence.max(
                        std::str::from_utf8(&key[EVENT_PREFIX.len()..])
                            .ok()
                            .and_then(|text| text.parse::<u64>().ok())
                            .unwrap_or(0),
                    );
                    if count > MAX_COUNT || bytes > MAX_BYTES {
                        failure = Some("learning database limit reached".to_string());
                        break;
                    }
                    if let Some(fields) = learning::unframe(&String::from_utf8_lossy(&value))
                        && fields.len() == 5
                        && let Ok(time) = fields[0].parse::<f64>()
                    {
                        events.push(Event {
                            time,
                            mode: fields[1].clone(),
                            code: fields[2].clone(),
                            text: fields[3].clone(),
                            context: fields[4].clone(),
                        });
                    }
                    if !iterator.advance() {
                        break;
                    }
                }
            }
            Err(error) => {
                store.error = Some(format!("learning database is unavailable: {error}"));
                return store;
            }
        }
        if let Some(reason) = failure {
            store.error = Some(reason);
            return store;
        }
        store.count = count;
        store.bytes = bytes;
        store.sequence = sequence;
        store.index = LearningIndex::runtime(&events, now);
        store.events = events;
        store.db = Some(db);
        store
    }

    pub fn store_ready(&self) -> bool {
        self.db.is_some()
    }

    pub fn index(&self) -> &LearningIndex {
        &self.index
    }

    pub fn index_version(&self) -> u64 {
        self.index_version
    }

    /// 参照 `M.confirm`：写入事件（受上限约束），更新索引；返回是否有变更。
    pub fn confirm(&mut self, events: &[Event]) -> bool {
        if self.db.is_none() || events.is_empty() {
            return false;
        }
        let mut changed = false;
        let mut accepted = Vec::new();
        for event in events {
            if self.count >= MAX_COUNT {
                break;
            }
            let key = format!("{EVENT_PREFIX}{:010}", self.sequence + 1);
            let value = learning::frame(&[
                lua_number(event.time),
                event.mode.clone(),
                event.code.clone(),
                event.text.clone(),
                event.context.clone(),
            ]);
            if self.bytes + key.len() + value.len() > MAX_BYTES {
                break;
            }
            let Some(db) = self.db.as_mut() else {
                break;
            };
            if db.put(key.as_bytes(), value.as_bytes()).is_err() {
                self.error = Some("learning database write failed".to_string());
                break;
            }
            self.count += 1;
            self.bytes += key.len() + value.len();
            self.sequence += 1;
            self.events.push(event.clone());
            accepted.push(event.clone());
            changed = true;
        }
        if changed {
            self.scored_at = crate::wall_clock();
            self.index = self.index.update(&accepted, &self.events, self.scored_at);
            self.index_version += 1;
        }
        changed
    }

    /// 参照 `M.refresh_scores`：`7b220ce` 起该函数只返回 `store.index`
    /// （「安静的时钟不得改变已发布的学习分」），故索引恒不变、恒返回 `false`。
    /// 保留该入口以对齐参照的调用点（`if not context:is_composing() then …`）。
    pub fn refresh_scores(&mut self, _now: f64) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("hux-learning-{}-{tag}", std::process::id()));
        std::fs::remove_dir_all(&dir).ok();
        std::fs::create_dir_all(&dir).expect("temp dir");
        dir
    }

    fn event(time: f64, text: &str) -> Event {
        Event {
            time,
            mode: "m".to_string(),
            code: "ab".to_string(),
            text: text.to_string(),
            context: String::new(),
        }
    }

    #[test]
    fn roundtrip_persists_events_and_keys() {
        let dir = temp_dir("roundtrip");
        let name = store_name(hux_scheme_tiger::scheme::SCHEME_ID);
        {
            let mut store = LearningStore::open(&dir, &name, 100.0);
            assert!(store.store_ready());
            assert!(store.confirm(&[event(100.0, "甲"), event(101.0, "乙")]));
            assert_eq!(store.count, 2);
            assert_eq!(store.sequence, 2);
        }
        let reopened = LearningStore::open(&dir, &name, 200.0);
        assert!(reopened.store_ready());
        assert_eq!(reopened.count, 2);
        assert_eq!(reopened.sequence, 2);
        assert_eq!(reopened.events[0].text, "甲");
        assert_eq!(reopened.events[1].time, 101.0);
        assert!(dir.join(format!("{name}.userdb")).is_dir());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn open_creates_missing_user_dir() {
        let base = std::env::temp_dir().join(format!("hux-learning-nested-{}", std::process::id()));
        std::fs::remove_dir_all(&base).ok();
        let user_dir = base.join("nested/user");
        let name = store_name("x");
        let store = LearningStore::open(&user_dir, &name, 100.0);
        assert!(store.store_ready(), "缺失的用户目录应被创建且库可用");
        assert!(user_dir.join(format!("{name}.userdb")).is_dir());
        std::fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn refresh_scores_never_republishes_the_index() {
        let dir = temp_dir("refresh");
        let base = crate::wall_clock();
        let mut store = LearningStore::open(&dir, &store_name("x"), base);
        store.confirm(&[event(base, "甲")]);
        let version = store.index_version();
        let epoch = store.index().now;
        assert_eq!(epoch, store.scored_at);
        // 参照 `7b220ce`：无时间衰减 ⇒ 无论过多久（含时钟回退）都不重建、不换 epoch。
        assert!(!store.refresh_scores(base + 30.0));
        assert!(!store.refresh_scores(base + 61.0));
        assert!(!store.refresh_scores(base - 1000.0));
        assert_eq!(store.index_version(), version);
        assert_eq!(store.index().now, epoch, "索引 epoch 不得被刷新改写");
        assert_eq!(store.scored_at, epoch);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn lua_number_matches_reference_format() {
        assert_eq!(lua_number(1726512345.0), "1726512345.0");
        assert_eq!(lua_number(1.5), "1.5");
        assert_eq!(lua_number(-3.0), "-3.0");
    }
}
