// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! 成熟前缀提交：按 tracker 成熟度择优并提交选中前缀。

use super::super::*;

/// 参照 `try_commit_mature_prefix` 内的 tracker 择优循环。
fn select_mature_tracker<'a>(
    trackers: &'a Map<String, Tracker>,
    evidence_raw: &[u8],
    lexicon: &Lexicon,
    retain: usize,
    committed_text: &str,
    committed_raw_length: usize,
    visible_top: Option<&str>,
) -> Option<&'a Tracker> {
    // 哈希表迭代序不确定：按 key 排序后再比较，保证平局时的确定性。
    let mut keys: Vec<&String> = trackers.keys().collect();
    keys.sort();
    let committed_text_elements = committed_text.chars().count();
    let mut selected: Option<&Tracker> = None;
    for key in keys {
        let tracker = &trackers[key];
        if (tracker.evidence_count >= EARLY_COMMIT_REQUIRED_EVIDENCE
            || tracker.strong_count >= EARLY_COMMIT_REQUIRED_STRONG)
            && tracker.raw_length > committed_raw_length
            && tracker.raw_length <= evidence_raw.len()
            && evidence_raw.len()
                - competing_boundary_end(
                    evidence_raw,
                    lexicon,
                    committed_raw_length,
                    tracker.raw_length,
                    tracker
                        .text_char_count
                        .saturating_sub(committed_text_elements),
                )
                >= retain
            && tracker.text.len() > committed_text.len()
            && tracker.text.starts_with(committed_text)
            && auto_commit_matches_visible_top(visible_top, &tracker.text)
            && selected
                .map(|current| tracker_better(tracker, current))
                .unwrap_or(true)
        {
            selected = Some(tracker);
        }
    }
    selected
}

/// 参照 `try_commit_mature_prefix`：证据成熟则提交选中前缀。
pub fn try_commit_mature_prefix(
    learning: &mut LearningCommit<'_>,
    context: &mut Context,
    state: &mut SentenceState,
    evidence_raw: &[u8],
    min_retained: usize,
    visible_top: Option<&str>,
    dot_armed: &mut bool,
) -> bool {
    let retain = if min_retained > 0 {
        EARLY_COMMIT_RETAINED_RAW_LENGTH.max(min_retained)
    } else {
        EARLY_COMMIT_RETAINED_RAW_LENGTH
    };
    let selected = select_mature_tracker(
        &state.trackers,
        evidence_raw,
        learning.decoder.lexicon(),
        retain,
        &state.committed_text,
        state.committed_raw.len(),
        visible_top,
    );
    let Some(selected) = selected else {
        return false;
    };
    if evidence_raw.len() - state.last_auto_commit_raw_length < EARLY_COMMIT_RETAINED_RAW_LENGTH {
        return false;
    }
    let commit = selected.text[state.committed_text.len()..].to_string();
    if commit.is_empty() {
        return false;
    }
    let selected_text = selected.text.clone();
    let selected_raw_length = selected.raw_length;
    state.committed_text = selected_text.clone();
    state.committed_raw =
        String::from_utf8_lossy(&evidence_raw[..selected_raw_length]).into_owned();
    state.last_auto_commit_raw_length = selected_raw_length;
    state.continuation_after_auto_commit = false;
    reset_early_evidence(state);
    state.save(context);
    if let Some(commit_text) = submit_early(context, state, &commit) {
        learning.commit_with_learning(
            context,
            state,
            &commit_text,
            &selected_text,
            selected_raw_length,
        );
    }
    // 参照：自动上屏文本以数字结尾时重新武装待发小数点（缓冲分支亦然）。
    if ends_with_digit(&commit) {
        *dot_armed = true;
    }
    restore_composition_input(context, &evidence_raw[selected_raw_length..]);
    true
}
