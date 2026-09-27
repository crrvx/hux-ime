// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! `Decoder` 的装配与配置访问：构造、词典/模型访问器与路径摘要。

use super::*;

impl Decoder {
    pub fn new(lexicon: Lexicon, supplement: Supplement, model: Option<MobileModel>) -> Self {
        let rank_of = lexicon.character_ranks.as_ref().map(|ranks| {
            ranks
                .iter()
                .filter_map(|(text, rank)| text.chars().next().map(|ch| (ch, *rank)))
                .collect()
        });
        // 参照模块初始化：从数据目录加载紧凑词先验（缺省关闭）。
        let (lexical, lexical_load_error) = {
            let paths: Vec<PathBuf> = lexicon
                .dirs()
                .iter()
                .map(|directory| directory.join(LEXICAL_FILE))
                .collect();
            lexical::load_first(&paths)
        };
        Self {
            lexicon,
            supplement,
            model,
            learning: None,
            rank_of,
            arena: Vec::new(),
            allow_duplicate_single: true,
            learning_affected: false,
            ranking_prior: RankingPriorParameters::default(),
            lexical,
            lexical_load_error,
            pinyin: None,
            pinyin_checked: false,
            pinyin_load_error: None,
        }
    }

    /// 音反查索引（首次访问时按数据目录懒加载）。
    pub fn pinyin_index(&mut self) -> Option<&crate::sound_to_char_shape::SoundToCharShapeIndex> {
        if !self.pinyin_checked {
            self.pinyin_checked = true;
            let (index, error) = crate::sound_to_char_shape::load_first(self.lexicon.dirs());
            self.pinyin = index;
            self.pinyin_load_error = error;
        }
        self.pinyin.as_ref()
    }

    /// 音反查索引载入错误（有文件但无效时记录）。
    pub fn pinyin_load_error(&self) -> Option<&str> {
        self.pinyin_load_error.as_deref()
    }

    /// 字反查两排提示（上排 = 光标左侧拼音、下排 = 虎码；懒加载索引，缺索引返回 `None`）。
    pub fn char_to_sound_shape_rows(
        &mut self,
        text: &str,
        anchor: usize,
    ) -> Option<(String, String)> {
        self.pinyin_index();
        let index = self.pinyin.as_ref()?;
        Some(crate::char_to_sound_shape::rows(
            index,
            &self.lexicon,
            text,
            anchor,
        ))
    }

    /// 音反查候选（含虎码注释过滤；上限 [`CANDIDATE_LIMIT`]）。
    ///
    /// 参数较多是因为要透传「索引 / 区间 / 标点表 / 会话态 / 形状」——与
    /// [`crate::sound_to_char_shape::translate`] 同源，故与同文件既有先例一致地豁免。
    #[allow(clippy::too_many_arguments)]
    pub fn sound_to_char_shape_candidates(
        &mut self,
        input: &[u8],
        prefix: char,
        start: usize,
        end: usize,
        punct: Option<&PunctTable>,
        pairs: &mut PairState,
        full_shape: bool,
    ) -> Vec<Candidate> {
        self.pinyin_index();
        let Some(index) = self.pinyin.as_ref() else {
            return Vec::new();
        };
        crate::sound_to_char_shape::translate(
            index,
            &self.lexicon,
            input,
            prefix,
            start,
            end,
            punct,
            pairs,
            full_shape,
            CANDIDATE_LIMIT,
        )
    }

    pub fn lexicon(&self) -> &Lexicon {
        &self.lexicon
    }

    /// 应用高频字过滤上限：重建词库索引（参照 `M.apply_high_freq_limit`）。
    ///
    /// 解码器不缓存词库派生的查询表（`rank_of` 只依赖字频文件，与上限无关），
    /// 故重建后无需其他失效动作。
    pub fn apply_high_freq_limit(&mut self, limit: usize) {
        self.lexicon.apply_high_freq_limit(limit);
    }

    /// 应用高频上限与字集开关：重建词库索引（同 [`Decoder::apply_high_freq_limit`] 的路径，
    /// 两者一起落位只重建一次）。
    pub fn apply_lexicon_options(&mut self, limit: usize, options: LexiconOptions) {
        self.lexicon.apply_lexicon_options(limit, options);
    }

    pub fn model(&self) -> Option<&MobileModel> {
        self.model.as_ref()
    }

    pub fn set_allow_duplicate_single(&mut self, allowed: bool) {
        self.allow_duplicate_single = allowed;
    }

    /// 参照 `M.set_learning_for_test`：接入学习索引与模式串。
    pub fn set_learning(&mut self, index: LearningIndex, mode: &str) {
        self.learning = Some(LearningWiring {
            index,
            mode: mode.to_string(),
        });
        self.learning_affected = false;
    }

    /// 参照 `M.decoder_parameters`（排序先验部分）。
    pub fn ranking_prior_parameters(&self) -> RankingPriorParameters {
        self.ranking_prior
    }

    /// 参照 `M.set_decoder_parameters_for_test`（排序先验部分）。
    pub fn set_ranking_prior_parameters(&mut self, parameters: RankingPriorParameters) {
        self.ranking_prior = parameters;
    }

    /// 参照 `lexicon_state.lexical_model`：紧凑词先验模型（缺省关闭）。
    pub fn lexical_model(&self) -> Option<&LexicalModel> {
        self.lexical.as_ref()
    }

    pub fn set_lexical_model(&mut self, model: Option<LexicalModel>) {
        self.lexical = model;
    }

    /// 参照 `ranking_prior.lexical_load_error`（有文件但无效时记录）。
    pub fn lexical_load_error(&self) -> Option<&str> {
        self.lexical_load_error.as_deref()
    }

    /// 参照 `item.path`：返回路径末节点 raw 长度与 `learning.diff` 所需路径
    /// （`DiffItem.path[0]` 为最外层非根节点）。
    /// 仅对最近一次 `decode*` 返回的项有效（arena 每次解码重建）。
    pub fn path_summary(&self, item: &Evaluated) -> (usize, DiffItem) {
        let raw_length = self.arena[item.path].raw_length;
        let mut nodes = Vec::new();
        let mut current = Some(item.path);
        while let Some(index) = current {
            let node = &self.arena[index];
            if node.raw_length > 0 {
                nodes.push(DiffPathNode {
                    raw_length: node.raw_length,
                    text_length: node.text_length,
                });
            }
            current = node.previous;
        }
        nodes.reverse();
        (
            raw_length,
            DiffItem {
                text: item.text.clone(),
                path: nodes,
            },
        )
    }
}
