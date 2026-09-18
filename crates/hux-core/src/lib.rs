// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! 虎句方案核心逻辑（Rust 直迁；项目：hux-ime / 虎虚）。
//!
//! 参照实现：[`tiger-sentense-rime`](https://github.com/lvyww/tiger-sentense-rime) 仓库的 `lua/`（Lua，作为差分 oracle）。
//! 字符串/偏移语义与参照一致：UTF-8 字节串、字节偏移。
//! 见 `docs/rust-migration.md`。

pub mod cache;
pub mod char_to_sound_shape;
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
pub mod sound_to_char_shape;
