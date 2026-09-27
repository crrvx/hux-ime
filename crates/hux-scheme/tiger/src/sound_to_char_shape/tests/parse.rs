// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! 索引解析与畸形字节加固的用例。

use super::*;

/// 正对照：合法字节流仍照常解析（校验不得误伤正常索引）。
#[test]
fn parse_accepts_well_formed_bytes() {
    let builder = IndexBuilder {
        syllables: vec!["zhong".to_string(), "guo".to_string()],
        spellings: vec![
            (b"zhong".to_vec(), vec![(0, TYPE_NORMAL)]),
            (b"guo".to_vec(), vec![(1, TYPE_ABBREV)]),
        ],
        groups: vec![(vec![0, 1], 2)],
        entries: vec![(3, "中国".to_string()), (1, "中".to_string())],
    };
    let index = SoundToCharShapeIndex::parse(&builder.bytes()).expect("合法索引");
    assert_eq!(index.spellings.len(), 2);
    assert_eq!(index.entry_count(), 2);
    assert_eq!(index.character_pinyin('中'), ["zhongguo".to_string()]);
    // 头部计数与实际记录数的关系仍被校验（少一条即报错）。
    let mut short = builder.bytes_with([2, 2, 1, 1]);
    short.truncate(short.len() - (4 + 2 + "中".len()));
    assert!(SoundToCharShapeIndex::parse(&short).is_err());
}

/// 越界音节 id（拼写变体侧）：解析失败，不得在建边/查组时越界。
#[test]
fn parse_rejects_spelling_syllable_id_out_of_range() {
    let builder = IndexBuilder {
        syllables: vec!["a".to_string()],
        spellings: vec![(b"a".to_vec(), vec![(1, TYPE_NORMAL)])],
        groups: vec![],
        entries: vec![],
    };
    let error = parse_error(&builder.bytes());
    assert!(
        error.to_string().contains("syllable id out of range"),
        "{error}"
    );
    // kind 只有 0/1 两种合法取值。
    let builder = IndexBuilder {
        syllables: vec!["a".to_string()],
        spellings: vec![(b"a".to_vec(), vec![(0, 2)])],
        groups: vec![],
        entries: vec![],
    };
    assert!(SoundToCharShapeIndex::parse(&builder.bytes()).is_err());
}

/// 越界音节 id（组码侧）与超出 `u16` 宽度的音节表：解析失败。
#[test]
fn parse_rejects_group_syllable_id_out_of_range() {
    let builder = IndexBuilder {
        syllables: vec!["a".to_string()],
        spellings: vec![],
        groups: vec![(vec![1], 0)],
        entries: vec![],
    };
    let error = parse_error(&builder.bytes());
    assert!(
        error.to_string().contains("syllable id out of range"),
        "{error}"
    );
    // 音节数超过组码宽度（`u16`）：组码表示不了 ⇒ 拒绝（否则 `as u16` 静默截断）。
    let builder = IndexBuilder {
        syllables: vec!["a".to_string()],
        ..Default::default()
    };
    let header = [65_536u32, 0, 0, 0];
    let error = parse_error(&builder.bytes_with(header));
    assert!(error.to_string().contains("group code width"), "{error}");
}

/// 组条目数回绕：两组各 `0x8000_0000`、总条目数 0 —— `first += count` 回绕后
/// **恰好**满足原来的「总数一致」校验，随后按组切片越界（`&entries[0..0x8000_0000]`）。
/// 修法后由「逐组 `first + count <= entry_count`」先拦下（`checked_add` 是第二道）。
#[test]
fn parse_rejects_group_entry_offsets_that_wrap() {
    let builder = IndexBuilder {
        syllables: vec!["a".to_string()],
        spellings: vec![],
        groups: vec![(vec![0], 0x8000_0000), (vec![0], 0x8000_0000)],
        entries: vec![],
    };
    let bytes = builder.bytes_with([1, 0, 2, 0]);
    let error = parse_error(&bytes);
    assert!(
        error.to_string().contains("entry count"),
        "回绕必须在切片前被拒绝：{error}"
    );
    // `checked_add` 自身：单组 `u32::MAX` 已够（`entry_count` 也取 `u32::MAX`
    // 才能越过逐组上界检查，但条目循环会先因数据不足失败 —— 这里只要求
    // 「任何畸形头部都不得走到切片」）。
    let builder = IndexBuilder {
        syllables: vec!["a".to_string()],
        spellings: vec![],
        groups: vec![(vec![0], u32::MAX), (vec![0], 1)],
        entries: vec![],
    };
    assert!(SoundToCharShapeIndex::parse(&builder.bytes()).is_err());
}

/// 单组条目数超过总条目数：即便总和不回绕也要拒绝（切片边界依据）。
#[test]
fn parse_rejects_group_entry_count_exceeding_entries() {
    let builder = IndexBuilder {
        syllables: vec!["a".to_string()],
        spellings: vec![],
        groups: vec![(vec![0], 5)],
        entries: vec![(1, "甲".to_string())],
    };
    let error = parse_error(&builder.bytes_with([1, 0, 1, 1]));
    assert!(
        error.to_string().contains("exceeds the entry count"),
        "{error}"
    );
}

/// 巨型头部计数（`u32::MAX`）：容量按剩余字节钳制 ⇒ 快速失败而非巨额分配
/// （未钳制时 `Vec::with_capacity(u32::MAX)` 直接 OOM/abort）。
#[test]
fn parse_rejects_oversized_header_counts_without_huge_allocation() {
    let builder = IndexBuilder::default();
    for header in [
        [u32::MAX, 0, 0, 0],
        [0, u32::MAX, 0, 0],
        [0, 0, u32::MAX, 0],
        [0, 0, 0, u32::MAX],
    ] {
        let error = parse_error(&builder.bytes_with(header));
        let text = error.to_string();
        assert!(
            text.contains("truncated index") || text.contains("group code width"),
            "巨型头部必须以诊断失败：{text}"
        );
    }
}

/// 截断文件：头部声明多于实际字节 ⇒ 诊断（不 panic）；`load` 侧带路径上下文。
#[test]
fn parse_rejects_truncated_index() {
    let builder = IndexBuilder {
        syllables: vec!["zhong".to_string()],
        spellings: vec![(b"zhong".to_vec(), vec![(0, TYPE_NORMAL)])],
        groups: vec![(vec![0], 1)],
        entries: vec![(1, "中".to_string())],
    };
    let full = builder.bytes();
    for cut in [8, 9, 12, 20, full.len() - 1] {
        let error = parse_error(&full[..cut]);
        assert!(!error.to_string().is_empty());
    }
    // 尾部文本长度前缀超出剩余字节。
    let mut bad = full.clone();
    let len = bad.len();
    bad[len - 3..len - 1].copy_from_slice(&0xffffu16.to_le_bytes());
    assert!(SoundToCharShapeIndex::parse(&bad).is_err());
}

/// `load` 对畸形**文件**同样返回诊断（不 panic）；gzip 分支同路。
#[test]
fn load_reports_malformed_files_without_panicking() {
    let dir = std::env::temp_dir().join(format!("hux-tcsrv-bad-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("临时目录");
    let path = dir.join(SOUND_TO_CHAR_SHAPE_FILE);
    std::fs::write(
        &path,
        b"TCSRV01\n\xff\xff\xff\xff\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00",
    )
    .expect("写畸形索引");
    let error = match SoundToCharShapeIndex::load(&path) {
        Ok(_) => panic!("畸形文件不应加载成功"),
        Err(error) => error,
    };
    assert!(error.to_string().contains("解析"), "{error}");
    let gz = dir.join(SOUND_TO_CHAR_SHAPE_FILE_GZ);
    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    std::io::Write::write_all(&mut encoder, &std::fs::read(&path).expect("读回")).expect("压缩");
    std::fs::write(&gz, encoder.finish().expect("gzip")).expect("写畸形 gz");
    assert!(SoundToCharShapeIndex::load(&gz).is_err());
    std::fs::remove_dir_all(&dir).expect("清理临时目录");
}
