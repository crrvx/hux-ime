// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! 组合与组合段：候选、段与组合的取文（提交文本/脚本文本）及分段推进。

/// 候选（对应参照经 `Candidate(...)` 构造的对象）。
#[derive(Clone, Debug, PartialEq)]
pub struct Candidate {
    pub kind: String,
    pub start: usize,
    pub end: usize,
    pub text: String,
    pub comment: String,
    pub preedit: String,
}

impl Candidate {
    pub fn new(kind: &str, start: usize, end: usize, text: &str, comment: &str) -> Self {
        Self {
            kind: kind.to_string(),
            start,
            end,
            text: text.to_string(),
            comment: comment.to_string(),
            preedit: String::new(),
        }
    }
}

/// 组合段（对应 librime `Segment` 的常用子集）。
#[derive(Clone, Debug, Default)]
pub struct Segment {
    pub start: usize,
    pub end: usize,
    pub tags: Vec<String>,
    /// 段提示（参照 `Segment::prompt`；如音反查段的「〔拼音〕」）。
    pub prompt: String,
    pub selected_index: usize,
    pub candidates: Vec<Candidate>,
    /// 是否已被确认（librime `Segment::status >= kSelected`）。
    pub selected: bool,
    /// 是否已建立菜单（librime `Segment::status >= kGuess`；`menu` 非空）。
    /// 已翻译的段在重分段时保留菜单与高亮。
    pub translated: bool,
}

impl Segment {
    pub fn has_tag(&self, tag: &str) -> bool {
        self.tags.iter().any(|value| value == tag)
    }

    pub fn selected_candidate(&self) -> Option<&Candidate> {
        self.candidates.get(self.selected_index)
    }

    /// 参照 `Menu::Prepare(n)`：返回当前可用数量（本实现候选为即时生成）。
    pub fn prepare(&self, count: usize) -> usize {
        self.candidates.len().min(count)
    }
}

/// 组合（对应 librime `Composition`）。
#[derive(Clone, Debug, Default)]
pub struct Composition {
    pub segments: Vec<Segment>,
}

impl Composition {
    pub fn empty(&self) -> bool {
        self.segments.is_empty()
    }

    pub fn back(&self) -> Option<&Segment> {
        self.segments.last()
    }

    /// 参照 `Segmentation::Forward`：末段非空时追加空尾段（下一轮起点）。
    pub fn forward(&mut self) -> bool {
        let Some(back) = self.segments.last() else {
            return false;
        };
        if back.start == back.end {
            return false;
        }
        let position = back.end;
        self.segments.push(Segment {
            start: position,
            end: position,
            ..Segment::default()
        });
        true
    }

    pub fn back_mut(&mut self) -> Option<&mut Segment> {
        self.segments.last_mut()
    }

    /// 参照 `Segmentation::GetCurrentStartPosition`。
    pub fn current_start_position(&self) -> usize {
        self.segments
            .last()
            .map(|segment| segment.start)
            .unwrap_or(0)
    }

    /// 参照 `Segmentation::GetCurrentEndPosition`。
    pub fn current_end_position(&self) -> usize {
        self.segments.last().map(|segment| segment.end).unwrap_or(0)
    }

    /// 参照 `Segmentation::HasFinishedSegmentation`。
    pub fn has_finished_segmentation(&self, input: &[u8]) -> bool {
        self.current_end_position() >= input.len()
    }

    /// 参照 `Segmentation::Trim`：移除末尾空段。
    pub fn trim(&mut self) -> bool {
        if self
            .segments
            .last()
            .map(|segment| segment.start == segment.end)
            .unwrap_or(false)
        {
            self.segments.pop();
            return true;
        }
        false
    }

    /// 参照 `Segmentation::GetConfirmedPosition`：最后一个已选段的末尾。
    pub fn confirmed_position(&self) -> usize {
        let mut confirmed = 0usize;
        for segment in &self.segments {
            if segment.selected {
                confirmed = segment.end;
            }
        }
        confirmed
    }

    /// 参照 `Composition::GetCommitText`：有选中候选的段取候选文本（不论段状态），
    /// 否则取原始输入切片（`phony` 段跳过）；末尾追加未被段覆盖的输入。
    pub fn commit_text(&self, input: &[u8]) -> String {
        let mut out = Vec::new();
        let mut end = 0usize;
        for segment in &self.segments {
            if let Some(candidate) = segment.selected_candidate() {
                end = candidate.end.min(input.len());
                out.extend_from_slice(candidate.text.as_bytes());
                continue;
            }
            end = segment.end.min(input.len());
            let start = segment.start.min(end);
            if !segment.has_tag("phony") {
                out.extend_from_slice(&input[start..end]);
            }
        }
        if input.len() > end {
            out.extend_from_slice(&input[end..]);
        }
        String::from_utf8_lossy(&out).into_owned()
    }

    /// 参照 `Composition::GetScriptText(keep_selection)`：**脚本文本**（`Ctrl+Return` 提交）。
    ///
    /// 每段按参照的三级判据取文本：① `keep_selection` 且段已确认（`status >= kSelected`）
    /// 且候选文字非空 ⇒ 候选文字；② 否则候选 `preedit` 非空 ⇒ `preedit` **去掉首个 `\t`**
    /// （`erase_first_copy`）；③ 否则非 `phony` 段 ⇒ 原始输入切片。末尾追加未被段覆盖的输入。
    ///
    /// 与 [`Composition::commit_text`] 的差异即「脚本文本 ≠ 提交文本」：确认段取候选文字
    /// （`keep_selection`）或 preedit，而不是候选 `text`；与参照一致地**不**按候选 `end`
    /// 截断段的原始切片。候选 `end` 超出输入时按输入长度钳制（参照无此护栏；此处与
    /// `commit_text` 同口径，避免越界切片）。
    pub fn script_text(&self, input: &[u8], keep_selection: bool) -> String {
        let mut out = Vec::new();
        let mut end = 0usize;
        for segment in &self.segments {
            let start = end;
            let candidate = segment.selected_candidate();
            end = candidate
                .map(|candidate| candidate.end)
                .unwrap_or(segment.end)
                .min(input.len());
            let stop = start.min(end);
            if keep_selection
                && let Some(candidate) = candidate
                && !candidate.text.is_empty()
                && segment.selected
            {
                out.extend_from_slice(candidate.text.as_bytes());
            } else if let Some(candidate) = candidate
                && !candidate.preedit.is_empty()
            {
                match candidate.preedit.split_once('\t') {
                    Some((head, tail)) => {
                        out.extend_from_slice(head.as_bytes());
                        out.extend_from_slice(tail.as_bytes());
                    }
                    None => out.extend_from_slice(candidate.preedit.as_bytes()),
                }
            } else if !segment.has_tag("phony") {
                out.extend_from_slice(&input[stop..end]);
            }
        }
        if input.len() > end {
            out.extend_from_slice(&input[end..]);
        }
        String::from_utf8_lossy(&out).into_owned()
    }
}

#[cfg(test)]
mod tests;
