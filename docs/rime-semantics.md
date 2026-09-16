# Rime 参照实现行为参考（移植对照）

> 2026-09-16 ｜ 关联：[`rust-migration.md`](rust-migration.md)

移植 Rust 时逐项对照本清单，保证行为等价；判定标准是差分测试与键序列金样。
字符串为 UTF-8 字节串；`caret_pos`、段偏移均为**字节偏移**。

## 1. 全局函数

| 名称 | 签名 | 语义 |
|---|---|---|
| `rime_api.get_user_data_dir` | `() -> string` | 用户可写目录 |
| `rime_api.get_shared_data_dir` | `() -> string` | 只读共享目录 |
| `Candidate` | `(type, start, end, text, comment) -> Candidate` | 构造候选 |
| `yield` | `(candidate)` | 仅 translator/filter 内有效 |
| `Schema` | `(name) -> Schema` | `".default"` 返回默认配置 |
| `Config` | `() -> Config` | 空配置，随后 `load_from_file` |
| `Component.Processor` | `(engine, schema, ns, klass) -> object` | 原生组件构造（至少 `"ascii_composer"`） |
| `LevelDb` | `(name) -> Db` | 学习库（目录 `<user>/<name>.userdb/`） |

## 2. Candidate / Segment / Composition

| 成员 | 访问 | 说明 |
|---|---|---|
| `Candidate.type` / `.text` | 读 | 核心用 `"sentence"` / `"sentence_buffered"` |
| `Candidate.comment` / `.quality` / `.preedit` | 写 | 注释、排序质量、预编辑 |
| `Candidate.start` / `._end` | 读 | 构造参数，段内字节范围 |
| `Segment.start` / `._end` / `.selected_index` | 读 | `selected_index` 为当前高亮 |
| `Segment:has_tag(name)` | 调用 | 码注释过滤器判 `"reverse_lookup"` |
| `Composition:empty()` / `:back()` | 调用 | 无段 / 末段 |

## 3. Context

| 成员 | 语义 |
|---|---|
| `input`（读/写）、`caret_pos`（读/写）、`composition`（读） | 输入串与光标（字节偏移） |
| `is_composing()` | `input` 非空**或**组合非空（librime `IsComposing`） |
| `has_menu()` | 末段候选菜单可准备 |
| `get_option` / `set_option` | 布尔选项；变更触发选项通知 |
| `get_property` / `set_property` | 字符串属性 KV（锁/错误/瞬态） |
| `clear()` | 清空组合 |
| `push_input(ch)` | caret 处插入字节，caret 后移 |
| `pop_input(n)` / `delete_input(n)` | 删除 caret 前 / 处 n 字节；越界不改动并返回 false（`n=0` 亦触发更新） |
| `set_input(str)` | 整体替换输入串 |
| `highlight(index)` | 移动高亮（截断到 `count-1`；空菜单归 0；未变化返回 false） |
| `confirm_current_selection()` | 确认并提交当前选中候选 |
| `refresh_non_confirmed_composition()` | 保留已确认段，重建未确认组合 |
| `get_commit_text()` | 按当前组合即时计算（未组合时为空串） |

通知器（`connect(fn)` 返回 `disconnect()` 连接；同步、单线程）：

| 名称 | 回调 | 时机 |
|---|---|---|
| `commit_notifier` | `fn(ctx)` | 提交完成后 |
| `update_notifier` | `fn(ctx)` | 组合/输入每次变化 |
| `option_update_notifier` | `fn(ctx, name)` | 选项变更 |

## 4. Engine / Schema / Config

| 成员 | 说明 |
|---|---|
| `engine.context` / `engine.schema` | 读 |
| `engine:commit_text(text)` | 直接上屏 |
| `Schema.schema_id` / `.config` | 学习库名依据；配置可变 |
| `Config:load_from_file` / `save_to_file` / `get_bool` / `set_bool` / `get_string` / `set_string` / `get_map` | 路径语法 `a/b/c`；缺失返回 `nil` |

核心读取点：`tiger_sentence/memory_profile`、`tab_learning`、`option_defaults/*`、
`ascii_composer/good_old_caps_lock`、`ascii_composer/switch_key/*`（含 `.default` 回退）。

## 5. KeyEvent

| 成员 | 说明 |
|---|---|
| `keycode` | X11 keysym |
| `repr()` | 键名串，必须与 librime 逐字节一致：`"a"`、`"semicolon"`、`"apostrophe"`、`"space"`、`"period"`、`"Return"`、`"KP_Enter"`、`"ISO_Left_Tab"`… |
| `release()` | fcitx5 不派发 release，恒 `false` |
| `shift()` / `ctrl()` / `alt()` / `super()` | 修饰键状态 |

## 6. CandidateList（filter 的 `input`）

`input:iter()` 按菜单顺序遍历；filter 遍历并 `yield` 保留的候选（可改写字段）。

## 7. 组件回调（参照侧注册于 `rime.lua`）

| 类型 | 形态 | 调用 |
|---|---|---|
| processor | `{init(env), func(key_event, env)->int, fini(env)}` | 按键管线顺序调用 |
| translator | `fn(input, seg, env)` | 组合构建；`yield` 产候选 |
| filter | `fn(input, env)` | translator 之后；遍历 `input:iter()` 并 `yield` |

- 返回值：`0`=Rejected（交还前端）、`1`=Accepted（吞键）、其他=Noop（交给后续处理器）。
- `env` 为每组件实例一张持久表，核心存私有键（`_tiger_*`、`native` 等），不得清理；
  全部回调单线程、不可重入。
- Lua state 为进程级：`rime.lua` 仅加载一次，模块级缓存（词库/模型）跨会话共享；
  组件实例与 `env` 随会话创建/销毁。

## 8. 宿主处理器链（原生组件）

`lua_processor@tiger_sentence_*` 之后是参照 schema 的原生链：
`key_binder` → `speller` → `punctuator` → `selector` → `navigator` → `express_editor`。
Rust 侧对应 `host` 模块（⑥ 已实现 key_binder/selector/navigator/express_editor 子集；
`speller` 见 ⑦）。关键事实（pin `33e78140` + schema）：

| 组件 | 行为 |
|---|---|
| `ascii_composer` | 参照链首组件；**本项目不实现英文模式**（按设计取舍）：英文输入交由 fcitx5 切换输入法；Shift/CapsLock 不切换模式，大写字母照常直通（见 `express_editor` 的 `char_handler`）|
| `key_binder` | `Tab`→Down、`Shift+Tab`→Up（`when: has_menu` 且非 ascii_mode）；`paging` 条件由段上的 `paging` 标签决定 |
| `punctuator` | 单个可打印 ASCII 键（无 Ctrl/Alt/Super；`ascii_punct` 关闭；`use_space=false` 时组合中空格除外）查表（`punctuator/<half|full>_shape`，`import_preset: symbols`）：标量/`{commit}` 直提交、`{pair}` 按键交替；组合中提交「组合文本 + 标点」并清空；`digit_separators: ""` 不做数字分隔符 |
| `selector` | 仅当末段有菜单（`status >= kGuess` 且非 `raw` 标签）；Horizontal\|Stacked：Up/Down 移候选（不环绕）、Page_Up/Down 翻页（`menu/page_size: 5`，`page_down_cycle` 缺省 false）、Home/End 回高亮 0（高亮为 0 时交 navigator）；末页号上报为 `selected_index / page_size`、页内高亮 `% page_size` |
| `navigator` | 组合中：Left/Right 移动字节光标（多段时按 spans 跨段跳）；Ctrl(+Shift)+Left/Right 按音节跳（单段即首/尾）；Home/End 到组合起点/输入末尾；`_vertical` 时改用上/下键；`FallbackOptions::All`：Shift 依次按 Ctrl、忽略 Shift 重试 |
| `express_editor` | `_auto_commit=true`：space → 确认/提交、BackSpace → 撤销上次编辑（`PopInput`）、Delete → 删光标处、Return → 提交原始输入、Escape → 取消组合；可打印字符按 `char_handler`（ExpressEditor = `DirectCommit`）：先提交当前组合再交宿主（保证上屏顺序） |
| 引擎分段 | `Compose`：`input[..caret]` 分段（caret 处无已确认段且不在末尾时翻译到 caret 后一段）；`Segmentation::Reset` 按新旧输入公共前缀增量保留段 → 未变的段保留菜单与高亮；提交后翻译失效（旧段不复用） |

## 9. LevelDb

`open()` / `close()` / `query(prefix)` → `:iter()`（`k, v`）/ `update(k, v)`；
目录 `<user_data_dir>/<name>.userdb/`，与 Rime 同构、可直接迁移。

## 10. 原生组件构造

`Component.Processor(engine, schema, "", "ascii_composer")`：核心用它构造私有
ascii_composer（`switch_key` 样式 + `good_old_caps_lock`），引擎需提供该原生组件。

## 11. 生命周期

1. 会话创建：加载 Schema 配置 → 创建组件实例 → `init(env)`；
2. 按键：processors 依序；`Accepted` 吞键、`Rejected` 交还、其余继续；
3. 更新：Context 变化 → `update_notifier` → 组合重建（segmentors → translators → filters）；
4. 提交：`commit_text` / `confirm_current_selection` → `commit_notifier`；
5. 选项：`set_option` → `option_update_notifier`；
6. 会话销毁：`fini(env)`。

## 12. 移植核对清单

- 全局对象与方法存在、类型正确；`Schema(".default")` / `Schema("tiger_sentence_ascii")` 可读；
- KeyEvent `repr` 表（字母/数字/标点/功能键/`KP_*`）与修饰查询；
- Context 编辑语义：caret 插入、`pop_input`/`delete_input`、`highlight`（截断/空菜单）、确认、重建组合、清空；
- 通知器：commit/update/option 的触发时机与 `disconnect`；
- CandidateList：过滤丢弃与注释改写后顺序保持；
- Config 往返、缺失返回 `nil`；LevelDb 打开/查询/更新/关闭与重启保留；
- ascii_composer 的 Shift/CapsLock 行为；`env` 生命周期与跨键状态；
- 处理器返回值映射与管线行为。
