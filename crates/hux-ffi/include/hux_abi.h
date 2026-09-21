// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

/*
 * hux-ime（虎虚）fcitx5 addon 的 C ABI（Rust 侧实现，C++ 薄壳调用）。
 * 布局定义见 `crates/hux-ffi/src/lib.rs`（类型）与 `platform/fcitx5/src/abi.rs`（导出函数）。
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

/*
 * 会话：每输入上下文（窗口/输入框）一个，组合与候选互相隔离。
 * 输入上下文注册时创建，销毁时释放；未知 id 的操作忽略。
 */
uint64_t hux_engine_session_new(hux_engine *engine);
void hux_engine_session_free(hux_engine *engine, uint64_t session);

/* 重置会话（失焦 / 切换输入法 / 重置事件）。 */
void hux_engine_reset(hux_engine *engine, uint64_t session);

/*
 * 数据加载状态（诊断；可为 NULL）。
 *
 * **指针有效期 = 下一次状态刷新之前**：选项保存失败、配置诊断、运行期学习库错误、
 * 热键绑定诊断都会替换内部状态串，此前返回的指针随即失效（复核整改第 4 批 F8——
 * 原注释「随引擎存活」与实现不符）。宿主应在每次需要时调用本函数取最新串，
 * **不要缓存指针**；引擎实例释放后同样失效。
 */
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
int32_t hux_engine_key(hux_engine *engine, uint64_t session, uint32_t keysym,
                       uint32_t states, int32_t release);

/*
 * 候选点击（面板候选 `CandidateWord::select`）：按全局索引选中并上屏
 * （与空格相同的确认/学习链）。返回 1 = 已处理；0 = 忽略。
 */
int32_t hux_engine_select_candidate(hux_engine *engine, uint64_t session,
                                    int32_t index);

/* 送入应用侧周边文本（字符制光标；valid=0 表示不可用/应用不支持）。 */
int32_t hux_engine_set_surrounding(hux_engine *engine, uint64_t session,
                                   const char *text_utf8, int32_t cursor_chars,
                                   int32_t valid);

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
  int32_t learning_on_tab;
  int32_t high_freq_limit;
  hux_key_list reverse_lookup_pronunciation;
  hux_key_list reverse_lookup_character;
  int32_t page_size;
  hux_key_list page_up;
  hux_key_list page_down;
  int32_t digit_select;
  /* 候选排列：0 = 跟随全局（默认），1 = 横排，2 = 竖排。 */
  int32_t candidate_layout;
  /* 预编辑内容：0 = 候选分码（默认），1 = 原始输入，2 = 不显示。 */
  int32_t preedit_mode;
  /* 翻页循环：0 = 关（默认），1 = 开。 */
  int32_t page_cycle;
  /* 提前上屏最短保留码数（0..=20；0 = 不额外限制）。 */
  int32_t min_retained_input_length;
} hux_options;

/*
 * 引擎选项角色（顺序即状态菜单项顺序；宿主据此取选项键，不得硬编码方案选项名）。
 *
 * 该顺序是 **ABI**：Rust 侧由 `hux_cfg::roles::RUNTIME_OPTION_ROLES` 派生
 * （`platform/fcitx5/src/abi.rs` 的 `hux_engine_option_role_count`），
 * 并由用例 `option_role_order_matches_the_abi_header` 逐项比对；
 * 宿主侧以 `HUX_OPTION_COUNT` 做 `static_assert` 长度守卫（`shell/hux.cpp`）。
 * 加角色 / 调序必须同时改这三处，否则编译或测试失败（此前 `kLabels[role]` 会越界读）。
 */
enum {
  HUX_OPTION_EARLY_COMMIT = 0,
  HUX_OPTION_EARLY_COMMIT_TO_PREEDIT = 1,
  HUX_OPTION_ALLOW_DUPLICATE_SINGLE = 2,
  HUX_OPTION_FULL_SHAPE = 3,
  HUX_OPTION_DIGIT_SELECT = 4,
  /* 角色总数（哨兵；不是合法角色下标）。 */
  HUX_OPTION_COUNT = 5,
};

/* 选项角色总数。 */
int32_t hux_engine_option_role_count(void);

/* 角色对应的选项键（NUL 结尾，引擎存活期内有效）；角色越界/引擎为空返回 NULL。 */
const char *hux_engine_option_key(const hux_engine *engine, int32_t role);

/* 应用外部配置；返回 1 = 已应用。 */
int32_t hux_engine_apply_settings(hux_engine *engine,
                                        const hux_options *options);

/* 读取运行时开关（状态菜单）：1/0；未知选项 -1。 */
int32_t hux_engine_option_value(hux_engine *engine, const char *name);

/* 设置运行时开关（状态菜单）：1 = 已应用；未知选项 0。 */
int32_t hux_engine_set_option(hux_engine *engine, const char *name,
                              int32_t value);

#ifdef __cplusplus
}
#endif

#endif /* HUX_ABI_H_ */
