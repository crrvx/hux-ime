// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! 虎虚（hux-ime）引擎内核：与方案、平台无关的通用部分。
//!
//! 组成：
//! - `key` / `key_table`：键事件与 rime 键名表（由 librime 源码生成）；
//! - `session`：librime `Context` / `Composition` / `Menu` 子集（输入为字节串、`caret` 为字节偏移）；
//! - `punct`：标点表（`symbols.yaml`）；`host`：librime 宿主链等价物与提交点回调
//!   [`host::CommitObserver`]；`learning`：学习机制；`cache`：有界 FIFO 缓存。
//!
//! 纪律（`docs/refactor.md` §1）：内核不依赖任何方案与平台——不出现环境变量 / XDG / 系统时钟 /
//! 直接打印（CI 校验），也不 import `hux-scheme/*`。虎码方案实现见 `hux-scheme/tiger`；
//! 方案契约 `hux_core::scheme` 随 P4c 落地。

pub mod cache;
pub mod host;
pub mod key;
pub mod key_table;
pub mod learning;
pub mod punct;
pub mod scheme;
pub mod session;
