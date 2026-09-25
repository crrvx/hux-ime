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
 * 热键绑定诊断都会替换内部状态串，此前返回的指针随即失效。宿主应在每次需要时调用本函数取最新串，
 * **不要缓存指针**；引擎实例释放后同样失效。
 */
const char *hux_engine_status(const hux_engine *engine);

/*
 * 模型信息（宿主托盘首项「虎虚：」后接的那段短名：按文件头 magic 认出的模型格式名，
 * 如 `三阶 TCSKNM02` / `无模型` / `五阶 TCSKNM03（装载失败）`）。
 *
 * **指针有效期 = 下一次 hux_engine_redeploy 之前**：重新部署会替换内部摘要串，此前返回的
 * 指针随即失效（同 hux_engine_status 的风格，宿主每次需要时重新调用、不要缓存）。
 * 引擎为空指针时返回 NULL。
 *
 * 短名由方案侧结构化产出，宿主直接展示、**不得解析**；文件名与失败原因等诊断在
 * `hux_engine_status` 的 `model:` 行里（只落日志，不进菜单）。
 */
const char *hux_engine_model_info(const hux_engine *engine);

/*
 * 引擎解析出的模型文件路径（UTF-8）。已装载 / 装载失败 ⇒ 该文件本身；
 * 未找到模型 ⇒ 默认查找路径（其父目录即「模型该放的地方」，文件可以不存在）。
 * 引擎为空指针时返回 NULL。指针有效期同 hux_engine_model_info（下一次重新部署前有效）。
 */
const char *hux_engine_model_path(const hux_engine *engine);

/*
 * 重新部署：重走一遍构造期的读取（数据/选项目录、模型、方案数据、选项存储、学习库），
 * 并重置全部会话状态。
 *
 * 会话 id 继续有效（宿主侧的输入上下文不需要重建），但旧组合与旧候选作废：宿主应在调用后
 * 重新推送设置（hux_engine_apply_settings；设置值仍是权威并写回持久化选项）、
 * 清空面板/客户端预编辑，并重新读取 hux_engine_model_info 与 hux_engine_status 刷新展示。
 * 进程级环境变量（HUX_DATA_DIRS / HUX_MODEL）在同一进程内无法改变，重算得到的是同一组目录。
 * 返回 1 = 成功；0 = 引擎不可用。
 */
int32_t hux_engine_redeploy(hux_engine *engine);

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
  /* 启用全字集（追加码表）：0 = 关（只装主表），1 = 开（默认）。 */
  int32_t full_charset;
  /* 过滤非汉字（追加码表里的部首/笔画/注音/假名等）：0 = 关，1 = 开（默认）。 */
  int32_t filter_non_han;
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
  HUX_OPTION_FULL_CHARSET = 5,
  HUX_OPTION_FILTER_NON_HAN = 6,
  /* 角色总数（哨兵；不是合法角色下标）。 */
  HUX_OPTION_COUNT = 7,
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

/*
 * 数据装载摘要（启动日志用）：装载了哪几张码表（主表在前、追加表按装载顺序）、条目数、单字数，
 * 以及两个字集开关的生效值，形如（出厂口径：全字集开、过滤非汉字开）
 *   `code_tables=[tiger_sentence.codes.txt,tiger_sentence.codes.huma.txt] entries=116762 chars=102529 full_charset=1 filter_non_han=1`
 * 指针有效期 = 下一次 hux_engine_redeploy / 设置变更之前（同 hux_engine_status 的风格：
 * 每次需要时重新调用、不要缓存）；引擎为空返回 NULL。
 */
const char *hux_engine_data_info(const hux_engine *engine);

#ifdef __cplusplus
}
#endif

#endif /* HUX_ABI_H_ */
