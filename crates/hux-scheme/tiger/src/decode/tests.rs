// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! decode 用例：共享夹具与子模块声明；用例按被测主题归档在 tests/ 子目录。

mod fusion;
mod locked;
mod reachability;
mod scoring;
mod util;

use super::*;

fn fixture_lexicon() -> Lexicon {
    let dir = hux_test_support::repo_path("goldens/lexicon");
    Lexicon::load(std::slice::from_ref(&dir), 1500)
}

fn fixture_decoder() -> Decoder {
    let dir = hux_test_support::repo_path("goldens/lexicon");
    let lexicon = Lexicon::load(std::slice::from_ref(&dir), 1500);
    let supplement = Supplement::load_default(Some(&dir));
    Decoder::new(lexicon, supplement, None)
}
