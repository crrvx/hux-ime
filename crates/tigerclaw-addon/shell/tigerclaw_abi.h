/*
 * 虎爪（虎句方案）fcitx5 addon 的 C ABI（Rust 侧实现，C++ 薄壳调用）。
 * 头文件与 `crates/tigerclaw-addon/src/lib.rs` 的导出符号一一对应。
 */
#ifndef TIGERCLAW_ABI_H_
#define TIGERCLAW_ABI_H_

#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/* 不透明引擎句柄。 */
typedef struct tigerclaw_engine tigerclaw_engine;

/*
 * 宿主回调表（C++ 薄壳实现；函数指针可为 NULL）。
 *
 * commit: 上屏文本（UTF-8，NUL 结尾）。
 * update: UI 状态快照——preedit（UTF-8，NUL 结尾）+ 光标（字节偏移，与
 *         fcitx `Text::cursor` 一致）+ 候选数组（文本/注释，各 NUL 结尾）+
 *         候选数 + 当前高亮索引。
 */
typedef struct tigerclaw_host {
    void *user;
    void (*commit)(void *user, const char *utf8);
    void (*update)(void *user, const char *preedit_utf8, int32_t cursor_bytes,
                   const char *const *candidate_texts,
                   const char *const *candidate_comments,
                   int32_t candidate_count, int32_t candidate_selected);
} tigerclaw_host;

tigerclaw_engine *tigerclaw_engine_new(const tigerclaw_host *host);
void tigerclaw_engine_free(tigerclaw_engine *engine);
void tigerclaw_engine_reset(tigerclaw_engine *engine);

/* 数据加载状态（诊断；随引擎存活，可为 NULL）。 */
const char *tigerclaw_engine_status(const tigerclaw_engine *engine);

/*
 * 处理一次按键：返回 1 = 已消费（宿主不应再处理该键）。
 * 提交/preedit/候选经宿主回调送出。
 */
int32_t tigerclaw_engine_key(tigerclaw_engine *engine, uint32_t keysym,
                             uint32_t states, int32_t release);

#ifdef __cplusplus
}
#endif

#endif /* TIGERCLAW_ABI_H_ */

#ifdef __cplusplus
extern "C" {
#endif

/* 外部配置（Rust 侧 Settings 的 C 布局；由壳从 fcitx5 配置读出后传入）。 */
typedef struct tigerclaw_options {
  int32_t early_commit;
  int32_t early_commit_to_preedit;
  int32_t allow_duplicate_single;
  int32_t full_shape;
  int32_t ascii_punct;
  int32_t tab_learning;
  int32_t high_freq_limit;
  int32_t pinyin_lookup_sym, pinyin_lookup_states;
  int32_t character_lookup_sym, character_lookup_states;
  int32_t quick_input_sym, quick_input_states;
} tigerclaw_options;

/* 应用外部配置；返回 1 = 已应用。 */
int32_t tigerclaw_engine_apply_settings(tigerclaw_engine *engine,
                                        const tigerclaw_options *options);

#ifdef __cplusplus
}
#endif
