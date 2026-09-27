// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! 提交点通知器：镜像 librime `Context::Commit()` 的通知回调（`commit_notifier_`；`observer` 为 `None` 时 no-op）。

use super::CommitObserver;
use crate::session::Context;

/// 宿主链提交点回调（对应 librime `Context::Commit()` 的通知器：组合仍完整时记录）；
/// `observer` 为 `None` 时 no-op。
pub(super) fn commit_notifier(
    observer: &mut Option<&mut dyn CommitObserver>,
    context: &Context,
    commit_text: &str,
) {
    if let Some(observer) = observer.as_deref_mut() {
        observer.on_commit(context, commit_text);
    }
}
