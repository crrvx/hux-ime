// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! 日志编码自证：键与 `frame`/`unframe` 往返。

use hux_core::learning;

use crate::harness::Harness;

/// 校验日志键为 `e/%010d`，且值能 `unframe` 后逐字段还原事件。
pub fn verify(harness: &Harness) {
    // 日志编码：键为 e/%010d，值为 frame(time, mode, code, text, context)。
    for (position, (key, value)) in harness.journal_values.iter().enumerate() {
        let expected_key = format!("e/{:010}", position + 1);
        assert_eq!(*key, expected_key, "journal key");
        let parts = learning::unframe(value).expect("journal value decodes");
        assert_eq!(parts.len(), 5, "journal value parts");
        assert_eq!(learning::frame(&parts), *value, "frame round trip");
        let got_time: f64 = parts[0].parse().expect("time part");
        let event = &harness.journal_events[position];
        assert_eq!(got_time, event.time, "journal time");
        assert_eq!(parts[1], event.mode, "journal mode");
        assert_eq!(parts[2], event.code, "journal code");
        assert_eq!(parts[3], event.text, "journal text");
        assert_eq!(parts[4], event.context, "journal context");
    }
}
