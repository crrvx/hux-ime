// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! 方案装配的接线：补充短语目录查找、触发键属性同步、学习 mode 自算，
//! 以及处理器未消费的键向前宿主链的转发。

use std::path::{Path, PathBuf};

use hux_core::host::{self, HostOptions};
use hux_core::key::KeyEvent;
use hux_core::punct::PunctTable;
use hux_core::scheme::{KeyOutcome, find_asset};
use hux_core::session::Context;

use crate::decode::Decoder;
use crate::interaction::{
    HostCommitObserver, K_CHAR_TO_SOUND_SHAPE_KEY, K_SOUND_TO_CHAR_SHAPE_KEY,
};
use crate::lexicon::SUPPLEMENT_FILE;

use super::TigerScheme;
use super::TigerSession;
use super::config::Config;

impl TigerScheme {
    /// 学习 mode 串（参照 `prepare_learning`）：关闭 Tab 学习 → 空串 = 不记录。
    /// 模式串自带版本号（`c69c1a8` 起 v1→v2）：事件与索引按 mode 分区，
    /// 旧版记录仍留在库中但不再命中。
    pub(super) fn mode_from_config(&self) -> String {
        if !self.config.learning_on_tab {
            return String::new();
        }
        format!(
            "sentence-v2|rules={}|optimal={}|dup={}",
            self.learning_rules,
            self.config.high_freq_limit,
            u8::from(self.config.allow_duplicate_single)
        )
    }
}

/// 补充短语所在目录：在**全部**数据目录中取首个存在该文件者。
///
/// 与 lexical / symbols 一致走资产查找——若只读 `dirs.first()`（用户目录恒排第一），
/// 一键安装把数据装到系统级目录时该文件会永不生效。
pub(super) fn supplement_dir(dirs: &[PathBuf]) -> Option<PathBuf> {
    find_asset(dirs, SUPPLEMENT_FILE).and_then(|path| path.parent().map(Path::to_path_buf))
}

/// 触发键（属性）：把两项触发键的 rime 键名列表（逗号分隔）交给 core 解析 / 匹配。
/// 配置变更后在下一次按键 / 重建时惰性同步（属性仅在按键处理中读取）。
pub(super) fn sync_trigger_keys(context: &mut Context, config: &Config) {
    for (property, value) in [
        (
            K_SOUND_TO_CHAR_SHAPE_KEY,
            config.reverse_lookup_pronunciation_keys.join(","),
        ),
        (
            K_CHAR_TO_SOUND_SHAPE_KEY,
            config.reverse_lookup_character_keys.join(","),
        ),
    ] {
        hux_core::session::set_property_if_changed(context, property, &value);
    }
}

/// 参照链：处理器未消费的键交宿主等价物（selector/navigator/express_editor 等）。
pub(super) fn forward_to_host(
    key: &KeyEvent,
    context: &mut Context,
    punct: Option<&PunctTable>,
    host_options: &HostOptions,
    decoder: &mut Decoder,
    state: &mut TigerSession,
    now: f64,
) -> KeyOutcome {
    let mut observer = HostCommitObserver {
        decoder,
        live: &mut state.live,
        state: &state.state,
        now,
    };
    // 契约结果类型即宿主链结果类型（`KeyOutcome` ≡ `HostResult`），无需转换。
    host::process_key(key, context, punct, host_options, Some(&mut observer))
}
