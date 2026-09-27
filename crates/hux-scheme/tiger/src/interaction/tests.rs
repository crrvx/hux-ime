// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;
use hux_core::host::HostOptions;
use hux_core::session::Segment;

// 共享夹具与子模块声明；用例按被测生产模块归档在 tests/ 子目录。
mod composition;
mod early_commit;
mod fusion;
mod keys;
mod learning_glue;
mod processor;
mod state;
mod translate;

fn state_with_lock(raw: &str, text: &str) -> SentenceState {
    let mut state = SentenceState::fresh(1);
    state.locks.push(Lock {
        raw: raw.to_string(),
        text: text.to_string(),
        boundaries: "2,3;".to_string(),
    });
    state.committed_raw = raw.to_string();
    state.committed_text = text.to_string();
    state
}

fn lexicon_fixture() -> Decoder {
    let dir = hux_test_support::repo_path("goldens/lexicon");
    let lexicon = Lexicon::load(std::slice::from_ref(&dir), 1500);
    let supplement = crate::lexicon::Supplement::load_default(Some(&dir));
    Decoder::new(lexicon, supplement, None)
}

// 交交/交疒 的手工路径（码表事实：ab → 交 rank1、疒 rank2）。
fn diff_item(text: &str) -> DiffItem {
    DiffItem {
        text: text.to_string(),
        path: vec![
            DiffPathNode {
                raw_length: 2,
                text_length: 3,
            },
            DiffPathNode {
                raw_length: 4,
                text_length: 6,
            },
        ],
    }
}

fn learning_state() -> SentenceState {
    let mut state = SentenceState::fresh(1);
    state.committed_raw = "ab".to_string();
    state.committed_text = "交".to_string();
    state
}

fn learning_live(mode: &str) -> LiveLearning {
    LiveLearning {
        mode: mode.to_string(),
        ..LiveLearning::default()
    }
}

/// 真实链路宿主：出厂缺省选项（数字直选开、`_auto_commit` 开：与 librime 一致）。
struct FusionHarness {
    decoder: Decoder,
    context: Context,
    state: SentenceState,
    live: LiveLearning,
    dot_armed: bool,
    builder: CompositionBuilder,
}

impl FusionHarness {
    fn new(decoder: Decoder) -> Self {
        let mut context = Context::new();
        context.set_option("_auto_commit", true);
        context.set_option(OPTION_DIGIT_SELECT, true);
        let live = LiveLearning {
            mode: "t".to_string(),
            store_ready: true,
            ..LiveLearning::default()
        };
        Self {
            decoder,
            context,
            state: SentenceState::fresh(1),
            live,
            dot_armed: false,
            builder: CompositionBuilder::default(),
        }
    }

    /// 一次按键的完整平台序：处理器 →（Forward 时）宿主链 → 组合重建 → update 通知器。
    fn press(&mut self, repr: &str) -> ProcessorResult {
        let key = key_of(repr);
        let host_options = HostOptions::default();
        let mut env = ProcessorEnv {
            now: 0.0,
            dot_armed: &mut self.dot_armed,
            min_retained: None,
            page_size: 5,
            host_options: &host_options,
        };
        let result = processor(
            &key,
            &mut self.context,
            &mut self.state,
            &mut self.decoder,
            &mut self.live,
            &mut env,
        )
        .expect("processor");
        if result == ProcessorResult::Forward {
            hux_core::host::process_key(&key, &mut self.context, None, &host_options, None);
        }
        self.builder
            .rebuild(
                &mut self.decoder,
                &mut self.context,
                &self.state,
                false,
                None,
            )
            .expect("rebuild");
        update_notifier(&mut self.context, &mut self.state, &mut self.live);
        result
    }
}

fn rebuild(
    builder: &mut CompositionBuilder,
    decoder: &mut Decoder,
    context: &mut Context,
    state: &SentenceState,
    invalidated: bool,
) -> bool {
    builder
        .rebuild(decoder, context, state, invalidated, None)
        .expect("rebuild")
}

fn key_of(repr: &str) -> KeyEvent {
    KeyEvent::from_repr(repr).expect("key repr")
}
