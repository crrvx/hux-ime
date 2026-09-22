// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! 模型（n-gram）装载状态：文件名 / 格式标签 / 装载结果 / 错误 + 给宿主的一行摘要。
//!
//! 模型的装载点在本 crate 的模型读取器，故状态在方案侧**结构化**产出；
//! 平台只把 [`ModelStatus::summary`] 搬到状态菜单，**不解析**诊断串。
//!
//! 格式标签按**文件头 magic** 判定、与装载器解耦：文件名只决定查找顺序，不声明格式；
//! 认不出的（含空文件 / 非模型文件）一律「未知格式」。

use std::io::Read;
use std::path::Path;

/// 模型文件头 magic 的长度（三阶 `TCSKNM02` / 五阶 `TCSKNM03`）。
const MAGIC_LEN: usize = 8;

/// 未知格式标签。
const UNKNOWN_FORMAT: &str = "未知格式";

/// 装载结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ModelState {
    /// 已装载：整句排序使用模型打分。
    Loaded,
    /// 没有模型文件：整句排序退化为码表名次。
    NotFound,
    /// 找到文件但装载失败（格式不符 / 读取错误）。
    Failed,
}

/// 模型状态（宿主「模型」项的口径）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModelStatus {
    file: Option<String>,
    format: &'static str,
    state: ModelState,
    error: Option<String>,
}

impl ModelStatus {
    /// 没有模型文件。
    pub fn not_found() -> Self {
        Self {
            file: None,
            format: UNKNOWN_FORMAT,
            state: ModelState::NotFound,
            error: None,
        }
    }

    /// 记录装载成功（`path` = 实际装载的文件）。
    pub fn record_loaded(&mut self, path: &Path) {
        self.file = Some(file_name(path));
        self.format = format_label_of(path);
        self.state = ModelState::Loaded;
        self.error = None;
    }

    /// 记录装载失败（`error` = 装载器给出的原因，原样保留）。
    pub fn record_failed(&mut self, path: &Path, error: impl Into<String>) {
        self.file = Some(file_name(path));
        self.format = format_label_of(path);
        self.state = ModelState::Failed;
        self.error = Some(error.into());
    }

    /// 模型文件名（无文件时为 `None`）。
    pub fn file(&self) -> Option<&str> {
        self.file.as_deref()
    }

    /// 格式标签（三阶 `TCSKNM02` / 五阶 `TCSKNM03` / 未知格式）。
    pub fn format(&self) -> &'static str {
        self.format
    }

    /// 装载结果。
    pub fn state(&self) -> ModelState {
        self.state
    }

    /// 装载失败的原因（已装载/未找到时为 `None`）。
    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    /// 一行摘要（宿主状态菜单「模型」项直接显示）：
    /// `<文件名> — 已加载（<格式标签>）` / `未找到模型（整句排序退化为码表名次）` /
    /// `<文件名> — 装载失败：<原因>`。
    pub fn summary(&self) -> String {
        match self.state {
            ModelState::Loaded => format!("{} — 已加载（{}）", self.display_file(), self.format),
            ModelState::NotFound => "未找到模型（整句排序退化为码表名次）".to_string(),
            ModelState::Failed => format!(
                "{} — 装载失败：{}",
                self.display_file(),
                self.error.as_deref().unwrap_or("未知原因")
            ),
        }
    }

    fn display_file(&self) -> &str {
        self.file.as_deref().unwrap_or("模型")
    }
}

/// 模型文件头 magic → 格式标签（纯函数，不依赖装载器是否支持该格式）。
///
/// 三阶 `TCSKNM02`、五阶 `TCSKNM03` 是上游的两种模型格式；其余（含过短/空文件头）
/// 一律「未知格式」。
pub fn format_label(magic: &[u8]) -> &'static str {
    if magic.starts_with(b"TCSKNM02") {
        "三阶 TCSKNM02"
    } else if magic.starts_with(b"TCSKNM03") {
        "五阶 TCSKNM03"
    } else {
        UNKNOWN_FORMAT
    }
}

/// 读文件头判格式（读不到按未知格式）：只读前 [`MAGIC_LEN`] 字节，不整体读入模型。
fn format_label_of(path: &Path) -> &'static str {
    let mut magic = [0u8; MAGIC_LEN];
    match std::fs::File::open(path).and_then(|mut file| file.read_exact(&mut magic)) {
        Ok(()) => format_label(&magic),
        Err(_) => UNKNOWN_FORMAT,
    }
}

/// 文件名（摘要只显示文件名，不显示完整路径）。
fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn fixture(name: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../../goldens")
            .join(name)
    }

    #[test]
    fn format_label_follows_the_file_magic() {
        assert_eq!(format_label(b"TCSKNM02rest"), "三阶 TCSKNM02");
        assert_eq!(format_label(b"TCSKNM03rest"), "五阶 TCSKNM03");
        // 前 7 字节相同但第 8 字节不同 ⇒ 不是该格式（避免「前缀近似」误判）。
        assert_eq!(format_label(b"TCSKNM0X"), "未知格式");
        assert_eq!(format_label(b"TCSKNM0"), "未知格式", "过短的文件头不算命中");
        assert_eq!(format_label(b""), "未知格式");
        assert_eq!(format_label(b"NOTAMODEL"), "未知格式");
    }

    #[test]
    fn summary_covers_every_state_with_the_file_name() {
        let mut status = ModelStatus::not_found();
        assert_eq!(status.state(), ModelState::NotFound);
        assert_eq!(status.file(), None);
        assert_eq!(status.summary(), "未找到模型（整句排序退化为码表名次）");

        // 三阶夹具：真实文件头 ⇒ 三阶标签 + 文件名（不含路径）。
        let three = fixture("ngram_fixture.bin");
        status.record_loaded(&three);
        assert_eq!(status.state(), ModelState::Loaded);
        assert_eq!(status.file(), Some("ngram_fixture.bin"));
        assert_eq!(status.format(), "三阶 TCSKNM02");
        assert_eq!(
            status.summary(),
            "ngram_fixture.bin — 已加载（三阶 TCSKNM02）"
        );

        // 装载失败：错误原文照抄（调用方不做任何解析）。
        status.record_failed(&three, "not a mobile TCSKNM02 model");
        assert_eq!(status.state(), ModelState::Failed);
        assert_eq!(status.file(), Some("ngram_fixture.bin"));
        assert_eq!(status.error(), Some("not a mobile TCSKNM02 model"));
        assert_eq!(
            status.summary(),
            "ngram_fixture.bin — 装载失败：not a mobile TCSKNM02 model"
        );
    }

    /// 五阶标签不必等五阶夹具/读取器落地：标签按 magic 判定，用临时文件即可覆盖
    /// 「拿到五阶文件 ⇒ 报五阶标签」这一半。
    #[test]
    fn five_level_label_is_independent_of_the_reader() {
        let dir = hux_test_support::temp_dir("model-status-fivegram");
        let path = dir.join("sentence-fivegram-mobile.bin");
        std::fs::write(&path, b"TCSKNM03placeholder").expect("write");
        let mut status = ModelStatus::not_found();
        status.record_failed(&path, "unsupported magic");
        assert_eq!(status.state(), ModelState::Failed);
        assert_eq!(status.format(), "五阶 TCSKNM03");
        assert_eq!(status.file(), Some("sentence-fivegram-mobile.bin"));
        assert_eq!(
            status.summary(),
            "sentence-fivegram-mobile.bin — 装载失败：unsupported magic"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    /// 非模型文件（存在但文件头不是任何模型格式）：标签未知，状态仍由装载结果决定。
    #[test]
    fn unknown_magic_reports_unknown_format() {
        let path = fixture("lexicon/tiger_sentence.codes.txt");
        let mut status = ModelStatus::not_found();
        status.record_loaded(&path);
        assert_eq!(status.format(), "未知格式");
        assert_eq!(status.state(), ModelState::Loaded);
        assert_eq!(
            status.summary(),
            "tiger_sentence.codes.txt — 已加载（未知格式）"
        );
    }
}
