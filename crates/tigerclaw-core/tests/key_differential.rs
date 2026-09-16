//! 差分测试：重放键金样（由 librime 探针生成），逐条比对。
//!
//! 金样 `goldens/key.tsv.gz` 由 `tools/gen_key_golden.sh` 生成
//! （系统 librime 1.17.0 + `tools/key_cases.txt`）。

use flate2::read::GzDecoder;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use tigerclaw_core::key::{self, KeyEvent};

fn repo_path(relative: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(relative)
}

#[test]
fn key_transcript_is_bit_exact() {
    let file = File::open(repo_path("goldens/key.tsv.gz")).expect("open key golden");
    let reader = BufReader::new(GzDecoder::new(file));
    let mut records = 0usize;
    for line in reader.lines() {
        let line = line.expect("read golden line");
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let fields: Vec<&str> = line.split('\t').collect();
        match fields[0] {
            "name" => {
                let keyval: i32 = fields[1].parse().expect("keyval");
                let expected = if fields[2] == "-" {
                    None
                } else {
                    Some(fields[2])
                };
                assert_eq!(key::key_name(keyval), expected, "name for {keyval:#x}");
            }
            "repr" => {
                let keyval: i32 = fields[1].parse().expect("keyval");
                let modifier: i32 = fields[2].parse().expect("modifier");
                let expected = fields[3];
                assert_eq!(
                    KeyEvent::new(keyval, modifier).repr(),
                    expected,
                    "repr for {keyval:#x}/{modifier:#x}"
                );
            }
            "parse" => {
                let repr = fields[1];
                let expected_ok = fields[2] == "ok";
                let parsed = KeyEvent::from_repr(repr);
                assert_eq!(parsed.is_some(), expected_ok, "parse ok flag for {repr:?}");
                if let Some(event) = parsed {
                    let keycode: i32 = fields[3].parse().expect("keycode");
                    let modifier: i32 = fields[4].parse().expect("modifier");
                    assert_eq!(event.keycode, keycode, "parse keycode for {repr:?}");
                    assert_eq!(event.modifier, modifier, "parse modifier for {repr:?}");
                    assert_eq!(event.repr(), fields[5], "re-repr for {repr:?}");
                }
            }
            "modifier" => {
                let index: u32 = fields[1].parse().expect("index");
                let expected = if fields[2] == "-" {
                    None
                } else {
                    Some(fields[2])
                };
                assert_eq!(
                    key::modifier_name(1i32 << index),
                    expected,
                    "modifier {index}"
                );
            }
            other => panic!("unknown record kind: {other}"),
        }
        records += 1;
    }
    assert!(records > 5_000, "transcript too short: {records} records");
    println!("key: {records} golden records verified");
}
