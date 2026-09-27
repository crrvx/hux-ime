// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! beam 解码：`Decoder` 的主实现按阶段拆分到子模块，本文件只做子模块声明与重导出。
//!
//! - [`decoder`]：装配与配置访问
//! - [`entry`]：解码入口
//! - [`locked`]：锁播种
//! - [`expand`]：区间扩展
//! - [`state`]：状态链与比较器
//! - [`bucket`]：桶聚合与裁剪
//! - [`score`]：候选评分
//! - [`emit`]：候选发射
//! - [`learning`]：学习奖励
//! - [`util`]：解码工具

mod bucket;
mod decoder;
mod emit;
mod entry;
mod expand;
mod learning;
mod locked;
mod score;
mod state;
mod util;

use super::*;

pub(super) use util::{
    beam_limit_at, has_letter, logsumexp, normalize, parse_boundaries, parse_selector,
};
