// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! 整键路径基准（P6）：经方案契约驱动一个会话，测「按键 → 组合重建」的单键耗时。
//!
//! ```sh
//! cargo run --release --example key_bench -- [--codes N] [--repeat N] [--model <bin>]
//! ```
//!
//! 覆盖 `Scheme::process_key`（处理器 + 宿主链 + 早提交证据）与 `Scheme::rebuild`
//! （翻译 + 过滤 + update 通知器）——即宿主每次按键实际付出的代价。

use hux_core::key::KeyEvent;
use hux_core::scheme::{Scheme, SchemeConfig};
use hux_core::session::Context;
use hux_scheme_tiger::scheme::TigerScheme;
use hux_test_support::{decode_hex, open_golden, repo_path};
use std::io::BufRead;
use std::time::Instant;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let flag = |name: &str| -> Option<String> {
        args.iter()
            .position(|arg| arg == name)
            .and_then(|index| args.get(index + 1))
            .cloned()
    };
    let limit: usize = flag("--codes")
        .map(|value| value.parse().expect("--codes"))
        .unwrap_or(200);
    let repeat: usize = flag("--repeat")
        .map(|value| value.parse().expect("--repeat"))
        .unwrap_or(10);
    let config = SchemeConfig {
        high_freq_limit: 1500,
        page_size: 5,
        tab_learning: false, // 基准不引入学习库差异
        ..SchemeConfig::default()
    };

    // 语料：金样里的输入码（与 decode 差分同一批），逐字符作为按键送入。
    let mut codes = Vec::new();
    for line in open_golden("goldens/decode.tsv.gz").lines() {
        let line = line.expect("line");
        if let Some(rest) = line.strip_prefix("decode\t") {
            let code = decode_hex(rest.split('\t').next().expect("hex field"));
            if !code.is_empty() {
                codes.push(code);
            }
        }
        if codes.len() >= limit {
            break;
        }
    }
    assert!(!codes.is_empty(), "语料为空");

    let model_path = flag("--model");
    let mut keys = 0u64;
    let mut samples = Vec::new();
    for _ in 0..repeat {
        let (mut scheme, _notes) = TigerScheme::load(
            &[repo_path("goldens/lexicon")],
            model_path.clone().map(std::path::PathBuf::from),
            config.clone(),
        );
        let mut context = Context::new();
        let session = scheme.new_session(&mut context);
        for code in &codes {
            for ch in code.chars() {
                if !ch.is_ascii() {
                    continue;
                }
                let key = KeyEvent::new(ch as i32, 0);
                let started = Instant::now();
                scheme
                    .process_key(session, &mut context, &key, 0.0)
                    .expect("process_key");
                scheme
                    .rebuild(session, &mut context, true)
                    .expect("rebuild");
                samples.push(started.elapsed().as_nanos() as u64);
                keys += 1;
            }
            // 换码前清空组合（等价于上屏后继续），避免语料相互叠加。
            context.drain_events();
            context.clear();
            scheme.reset_session(session, &mut context);
        }
    }
    samples.sort_unstable();
    let total: u64 = samples.iter().sum();
    let pick = |quantile: f64| -> f64 {
        let index = ((samples.len() as f64 - 1.0) * quantile).round() as usize;
        samples[index] as f64 / 1000.0
    };
    println!(
        "{{\"codes\":{},\"repeat\":{},\"keys\":{},\"model\":{},\"mean_us\":{:.2},\"p50_us\":{:.2},\"p95_us\":{:.2},\"max_us\":{:.2}}}",
        codes.len(),
        repeat,
        keys,
        model_path.is_some(),
        total as f64 / samples.len() as f64 / 1000.0,
        pick(0.50),
        pick(0.95),
        samples[samples.len() - 1] as f64 / 1000.0,
    );
}
