//! 虎整句核心逻辑（Rust 直迁）。
//!
//! 参照实现：`tiger-sentense-rime` 仓库的 `lua/`（Lua，作为差分 oracle）。
//! 字符串/偏移语义与参照一致：UTF-8 字节串、字节偏移。
//! 见 `docs/rust-migration.md` 与 `docs/rime-semantics.md`。

pub mod ascii;
pub mod cache;
pub mod decode;
pub mod host;
pub mod interaction;
pub mod key;
pub mod key_table;
pub mod learning;
pub mod lexical;
pub mod lexicon;
pub mod ngram;
pub mod punct;
pub mod session;
