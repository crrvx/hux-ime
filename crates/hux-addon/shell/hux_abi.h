// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

/*
 * hux-ime（虎句方案）fcitx5 addon 的 C ABI（Rust 侧实现，C++ 薄壳调用）。
 * 头文件与 `crates/hux-addon/src/lib.rs` 的导出符号一一对应。
 */
#ifndef HUX_ABI_H_
#define HUX_ABI_H_

#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/* 不透明引擎句柄。 */
typedef struct hux_engine hux_engine;

/*
 * 宿主回调表（C++ 薄壳实现；函数指针可为 NULL）。
 *
 * commit: 上屏文本（UTF-8，NUL 结尾）。
 * update: UI 状态快照——preedit（UTF-8，NUL 结尾）+ 光标（字节偏移，与
 *         fcitx `Text::cursor` 一致）+ 候选数组（文本/注释，各 NUL 结尾）+
 *         候选数 + 当前高亮索引 + 两排辅助文本（auxUp = 字反查上排/光标左、
 *         auxDown = 下排/光标右；UTF-8，NUL 结尾，可为 ""）。
 *         候选数为 0 时宿主必须清除候选列表（置 `nullptr`）——不得留下
 *         「存在但为空」的列表：其他组件（如 fcitx5-table）会对它调用
 *         `candidate(0)` 并抛 `CommonCandidateList: invalid index`。
 */
typedef struct hux_host {
    void *user;
    void (*commit)(void *user, const char *utf8);
    void (*update)(void *user, const char *preedit_utf8, int32_t cursor_bytes,
                   const char *const *candidate_texts,
                   const char *const *candidate_comments,
                   int32_t candidate_count, int32_t candidate_selected,
                   const char *aux_up_utf8, const char *aux_down_utf8);
} hux_host;

hux_engine *hux_engine_new(const hux_host *host);
void hux_engine_free(hux_engine *engine);
void hux_engine_reset(hux_engine *engine);

/* 数据加载状态（诊断；随引擎存活，可为 NULL）。 */
const char *hux_engine_status(const hux_engine *engine);

/*
 * hux_engine_key 返回值位掩码：
 * HUX_KEY_CONSUMED              已消费（宿主不应再处理该键）。
 * HUX_KEY_FORWARD_AFTER_COMMIT  已提交且未消费——宿主应消费该键并以 forwardKey 重发，
 *                               保证客户端先收到提交、后收到按键（对齐核心 KeyEventOrderFix）。
 */
#define HUX_KEY_CONSUMED 0x1
#define HUX_KEY_FORWARD_AFTER_COMMIT 0x2

/*
 * 处理一次按键：返回位掩码（HUX_KEY_*）。
 * 提交/preedit/候选经宿主回调送出。
 */
int32_t hux_engine_key(hux_engine *engine, uint32_t keysym,
                             uint32_t states, int32_t release);

/* 送入应用侧周边文本（字符制光标；valid=0 表示不可用/应用不支持）。 */
int32_t hux_engine_set_surrounding(hux_engine *engine,
                                         const char *text_utf8,
                                         int32_t cursor_chars, int32_t valid);

/* 键位列表上限（与 Rust `HUX_MAX_KEYS` 一致）。 */
#define HUX_MAX_KEYS 8

/* 键位列表（fcitx5 KeyList → C ABI；`sym == 0` 的项忽略）。 */
typedef struct hux_key_list {
  int32_t count;
  int32_t sym[HUX_MAX_KEYS];
  int32_t states[HUX_MAX_KEYS];
} hux_key_list;

/* 外部配置（Rust 侧 Settings 的 C 布局；由壳从 fcitx5 配置读出后传入）。 */
typedef struct hux_options {
  int32_t early_commit;
  int32_t early_commit_to_preedit;
  int32_t allow_duplicate_single;
  int32_t full_shape;
  int32_t ascii_punct;
  int32_t tab_learning;
  int32_t high_freq_limit;
  hux_key_list sound_to_char_shape;
  hux_key_list char_to_sound_shape;
  int32_t page_size;
  hux_key_list page_up;
  hux_key_list page_down;
  int32_t digit_select;
} hux_options;

/* 应用外部配置；返回 1 = 已应用。 */
int32_t hux_engine_apply_settings(hux_engine *engine,
                                        const hux_options *options);

#ifdef __cplusplus
}
#endif

#endif /* HUX_ABI_H_ */
