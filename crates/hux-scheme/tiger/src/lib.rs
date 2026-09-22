// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! 虎句（`tiger_sentence`）方案：虎码字/词/句的整句输入。
//!
//! 参照实现：[`tiger-sentense-rime`](https://github.com/lvyww/tiger-sentense-rime) 的 `lua/`
//! （Lua，作为差分 oracle）；字符串/偏移语义与参照一致：UTF-8 字节串、字节偏移。
//!
//! 依赖方向：`hux-scheme/* → hux-core`；内核不依赖任何方案。
//!
//! 组成：
//! - 数据与计算：`lexicon`（码表/字频/白名单/补充）、`decode`（beam 解码与早提交证据）、
//!   `lexical`（TCSLEX01 词先验）、`ngram`（TCSKNM02 三阶模型）、`fivegram`（TCSKNM03 五阶模型）；
//! - 反查：`sound_to_char_shape`（音反查）、`char_to_sound_shape`（字反查）；
//! - 交互策略：`interaction`（处理器管线、锁与瞬态状态、早提交、学习粘合、宿主提交点回调实现）。

pub mod char_to_sound_shape;
pub mod decode;
pub mod fivegram;
pub mod interaction;
pub mod lexical;
pub mod lexicon;
pub mod ngram;
pub mod scheme;
pub mod sound_to_char_shape;
