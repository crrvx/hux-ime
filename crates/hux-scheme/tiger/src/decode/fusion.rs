// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! 跨来源融合：按学习库的成对偏好把 Direct / Composed 两列归并
//! （`apply_fusion_ordering`）。

use super::*;

// ---------------------------------------------------------------- 跨来源融合

/// 参照 `learning.fusion_score`；无学习库（`None`，对应参照的 `learning_index == nil`）
/// 或空模式时恒 0。
fn fusion_score(
    index: &mut Option<&mut LearningIndex>,
    mode: &str,
    raw: &[u8],
    direct: &str,
    composed: &str,
) -> f64 {
    match index {
        None => 0.0,
        Some(index) => index.fusion_score(mode, raw, direct, composed),
    }
}

/// 参照 `learning.apply_fusion_ordering`：保持 Direct 的原始菜单序，
/// 再按成对偏好把 Direct / Composed 两列做**两指针归并**。
///
/// - 分列：`candidate_is_direct` 为真进 Direct 列，否则（含锁定重放的未标记来源）
///   进 Composed 列。仅当候选**全为 Direct** 时用重排后的 Direct 列回写
///   ——这就是「直接序保持」的落点；全为 Composed 时保持既有顺序。
/// - 每步前缀前瞻：Direct 侧 `max fusion_score(raw, direct[i], c)`，
///   Composed 侧 `max −fusion_score(raw, d, composed[i])`。两侧都不 `> 0`，
///   或差值落在 `1e-12` 内（含完全相等）时，回退到 `base`（原始下标）较小者，
///   即保持原交错序。
/// - 无学习库时所有 `fusion_score` 恒 0 ⇒ 归并退化为「全 Direct 时按 `direct_rank`
///   重排、其余保持原序」。
pub(super) fn apply_fusion_ordering(
    mut index: Option<&mut LearningIndex>,
    mode: &str,
    raw: &[u8],
    candidates: &mut [Evaluated],
) {
    if candidates.len() < 2 {
        return;
    }
    let mut base: HashMap<String, usize> = HashMap::new();
    let mut direct: Vec<usize> = Vec::new();
    let mut composed: Vec<usize> = Vec::new();
    for (position, item) in candidates.iter().enumerate() {
        base.entry(item.text.clone()).or_insert(position);
        if candidate_is_direct(item.source_mask) {
            direct.push(position);
        } else {
            composed.push(position);
        }
    }
    // `table.sort` 的比较器在此处是全序（`direct_rank` 同值回退 `base` 下标），
    // 故与参照的不稳定排序等价。
    direct.sort_by(|&left, &right| {
        let left_rank = candidates[left].direct_rank;
        let right_rank = candidates[right].direct_rank;
        if left_rank != right_rank {
            return left_rank
                .partial_cmp(&right_rank)
                .unwrap_or(std::cmp::Ordering::Equal);
        }
        base_index(&base, &candidates[left].text).cmp(&base_index(&base, &candidates[right].text))
    });
    if direct.is_empty() || composed.is_empty() {
        if composed.is_empty() {
            let reordered: Vec<Evaluated> = direct
                .iter()
                .map(|&position| candidates[position].clone())
                .collect();
            candidates.clone_from_slice(&reordered);
        }
        return;
    }
    let mut merged: Vec<usize> = Vec::with_capacity(candidates.len());
    let (mut di, mut ci) = (0usize, 0usize);
    while di < direct.len() && ci < composed.len() {
        let d = direct[di];
        let c = composed[ci];
        let mut direct_prefix = 0.0f64;
        for &ahead in &direct[di..] {
            direct_prefix = direct_prefix.max(fusion_score(
                &mut index,
                mode,
                raw,
                &candidates[ahead].text,
                &candidates[c].text,
            ));
        }
        let mut composed_prefix = 0.0f64;
        for &ahead in &composed[ci..] {
            composed_prefix = composed_prefix.max(-fusion_score(
                &mut index,
                mode,
                raw,
                &candidates[d].text,
                &candidates[ahead].text,
            ));
        }
        let by_base =
            || base_index(&base, &candidates[d].text) < base_index(&base, &candidates[c].text);
        let take_direct = if direct_prefix > 0.0 || composed_prefix > 0.0 {
            if (direct_prefix - composed_prefix).abs() > 1e-12 {
                direct_prefix > composed_prefix
            } else {
                by_base()
            }
        } else {
            by_base()
        };
        if take_direct {
            merged.push(d);
            di += 1;
        } else {
            merged.push(c);
            ci += 1;
        }
    }
    merged.extend_from_slice(&direct[di..]);
    merged.extend_from_slice(&composed[ci..]);
    let reordered: Vec<Evaluated> = merged
        .iter()
        .map(|&position| candidates[position].clone())
        .collect();
    candidates.clone_from_slice(&reordered);
}

/// 参照 `base[text] or math.huge`：缺失文本排到最后。
fn base_index(base: &HashMap<String, usize>, text: &str) -> usize {
    base.get(text).copied().unwrap_or(usize::MAX)
}
