// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! 按键路由：翻页键不被标点分支遮蔽、修饰键 / 释放 / 空闲态放行、组合内导航与标点。
//!
//! 大写早提交与粘滞 forward 亦在此核对；夹具与 `serial()` 串行约定见父模块 `tests.rs`。

use super::*;

// 主题分组：翻页（`paging`）、修饰键/释放/空闲放行（`pass_through`）、组合内导航（`composition`）、
// 标点（`punctuation`）、大写早提交与粘滞 forward（`forward`）、配置页早提交（`early_commit`）。
mod composition;
mod early_commit;
mod forward;
mod paging;
mod pass_through;
mod punctuation;

// 本用例推送的页大小：与缺省 5 不同，故期望值只可能来自配置。
const PAGE_SIZE: usize = 3;
// 本用例的引擎统一在这里构造：一律先推送同一页大小，翻页期望值才只可能来自配置。
fn paging_engine() -> TestEngine {
    let mut engine = TestEngine::new(host(), fixture_dirs(), None, None);
    engine.apply_settings(Settings {
        page_size: PAGE_SIZE,
        ..Default::default()
    });
    engine
}
