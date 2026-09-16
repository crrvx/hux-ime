# 设计（K0–K4）

hux-ime：虎句（`tiger_sentence`）输入方案的 fcitx5 原生 Rust 实现。
参照实现（测试 oracle，仅开发/CI 使用）：<https://github.com/crrvx/tiger-sentense-rime>；
金样清单与复现命令见 [`../goldens/README.md`](../goldens/README.md)。

## 1. 路线与状态

| 阶段 | 内容 | 状态 |
|---|---|---|
| K0 | spike：`cache`/`ngram` 移植、差分工具链、陷阱审计 | ✅ fixture 29,617 行 + 真实模型 62,777 行逐位一致；查询吞吐约 82× |
| K1 | 计算核：lexicon、beam 解码、早提交证据、learning | ✅ 快照差分全绿 |
| K1.5 | 上游追平：紧凑排序先验（TCSLEX01）、锁播种修复 | ✅ |
| K2 | 交互引擎：键事件/键表、会话、交互层、宿主链 | ✅ 键序列金样一致 |
| K3 | fcitx5 addon：注册与候选、编辑语义、标点、音查虎、字查音+虎、配置与学习库 | 进行中（打包与状态菜单待做） |
| K4 | 验收与打包 | 待做 |

- **移植纪律**：计算部分机械翻译（浮点按位模式比较）；交互部分按行为契约自由设计。
- **确定性纪律**：凡排序必带全序 tie-breaker；凡 `pairs` 影响可观测结果处显式排序；凡时间/随机全部注入。

## 2. 仓库结构

```
crates/hux-core/         # 纯逻辑，无 fcitx5 依赖
  cache.rs  ngram.rs                     # K0：缓存、TCSKNM02 模型读取
  lexicon.rs  decode.rs  learning.rs     # K1：码表、beam 解码、Tab 学习
  lexical.rs                             # K1.5：紧凑词先验 TCSLEX01
  key.rs  key_table.rs  session.rs  interaction.rs  host.rs   # K2：键事件、会话、交互层、宿主链
  pinyin_lookup.rs  character_lookup.rs  # 反查：音查虎、字查音+虎
crates/hux-addon/        # K3：C++ 薄壳（shell/）+ Rust FFI（src/）→ core
data/                    # 随包数据源
goldens/                 # 差分金样与夹具
tools/                   # 金样生成器（generators/）、探针与基准（probes/）、探针用例（cases/）
docs/                    # 本文档、词先验署名、数据清单
```

依赖方向：`addon → core`；core 不依赖 fcitx5、不依赖 Lua。

## 3. 模块映射（参照 → Rust）

| 参照 | Rust | 差分手段 |
|---|---|---|
| `lua/tiger_sentence_cache.lua` | `cache.rs` | fixture 金样（状态/淘汰序） |
| `lua/tiger_sentence_ngram.lua` | `ngram.rs` | `logp`/`obs`/`status` 逐位 |
| `lua/tiger_sentence.lua`（词库/解码/证据） | `lexicon.rs` + `decode.rs` | 数据索引 + 解码/证据/学习快照 |
| `lua/tiger_sentence_learning.lua` | `learning.rs` | 检查重放 + learning 金样 |
| `lua/tiger_sentence_lexical.lua` | `lexical.rs`（TCSLEX01） | 词先验金样 |
| `lua/tiger_sentence.lua`（processor/translator/filter/选项） | `key.rs` + `session.rs` + `interaction.rs` | 键序列金样 |
| librime `key_event`/`key_table` | `key.rs` + `key_table.rs`（由源码生成） | 键金样（真 librime 探针） |
| librime `reverse_lookup_translator` | `pinyin_lookup.rs`（TCSRV01） | 音查虎金样 |
| librime 宿主链 | `host.rs`（含 `punct.rs`） | 键序列金样 |

## 4. 数据与目录

- 用户目录 `~/.local/share/fcitx5/hux`，共享目录 `/usr/share/fcitx5/hux`；
  开发可用 `HUX_DATA_DIRS`（冒号分隔）与 `HUX_MODEL` 覆盖。
- 运行数据：码表四件套（`tiger_sentence.{codes,char_ranks,full_code_whitelist,supplement}.txt`）、
  `models/sentence-ngram-mobile.bin`（TCSKNM02）、`symbols.yaml`、`tiger_sentence.lexical.bin`（TCSLEX01）、
  `tiger_sentence.pinyin.bin.gz`（TCSRV01）、`tiger_sentence.options.yaml`、学习库 `<hash>.userdb/`（LevelDB 同构）。
- 仓库 `data/`：发布默认 `symbols.yaml`（仅覆盖 half_shape 的 `/` 提交 `/`）、词先验位图（CC BY 4.0，
  署名见 [`LEXICAL_PRIOR_ATTRIBUTION.md`](LEXICAL_PRIOR_ATTRIBUTION.md)，参数见
  [`LEXICAL_PRIOR_MANIFEST.json`](LEXICAL_PRIOR_MANIFEST.json)）、音查虎索引（清单见
  [`PINYIN_INDEX_MANIFEST.json`](PINYIN_INDEX_MANIFEST.json)）；详见 [`../data/README.md`](../data/README.md)。

## 5. fcitx5 集成要点

- **注册与构建**：`Category=InputMethod` + `OnDemand` + 输入法条目 conf；C++ 薄壳链接 Rust 静态库：

  ```sh
  cmake -S crates/hux-addon -B build/addon -DCMAKE_BUILD_TYPE=Release -DCMAKE_INSTALL_PREFIX=/usr
  cmake --build build/addon -j && sudo cmake --install build/addon
  ```

  产物：`/usr/lib/fcitx5/libhux.so`、`/usr/share/fcitx5/{addon,inputmethod}/hux.conf`。
- **会话**：每引擎单会话（`activate/deactivate/reset` 清空）；组合重建由 `interaction::CompositionBuilder`
  按参照 `ConcreteEngine::Compose` 语义（`input[..caret]`、按公共前缀增量保留段、提交后旧段不复用）。
- **宿主语义**（core `host.rs`，`processor` 返回 Forward 后执行）：

  | 组件 | 行为要点 |
  |---|---|
  | `key_binder` | `Tab`→Down、`Shift+Tab`→Up（`when: has_menu`） |
  | `selector` | 菜单导航与翻页（`page_size: 5`；`-`(paging)→Page_Up、`=`(has_menu)→Page_Down）、Home/End |
  | `navigator` | 字节光标移动；Ctrl(+Shift)+Left/Right 按音节跳；Home/End 到组合起点/末尾 |
  | `express_editor` | space 确认/提交、BackSpace 撤销编辑、Delete 删光标处、Return 提交原文、Escape 取消；可打印字符先提交组合再交宿主 |
  | `punctuator` | 单键可打印 ASCII 查 `symbols.yaml`；组合中提交「组合文本 + 标点」；`{pair}` 交替 |

- **英文模式不实现**（设计取舍）：英文输入交由 fcitx5 切换输入法；大写字母经 `char_handler` 直通（先提交组合）。
- **提交与按键顺序**：可打印字符的 `char_handler` 在核心语义为「提交组合 + 不消费」（同 librime）；宿主层
  （addon）据此消费该键并以 `forwardKey` 重发，保证客户端先收到提交、后收到按键
  （与 fcitx5 核心 `KeyEventOrderFix` 修法一致）。
- **UI 同步**：preedit 取高亮候选的 `preedit`（正常段按词分码，如 `sh ks`；音查虎段按音节，如
  `` `zhong guo ``），光标为字节偏移；高亮不在实况输入末尾时回退「缓冲 + 原始输入」。
- **反查**：音查虎（`pinyin_lookup.rs`）语义对齐 librime 词典反查——拼写缩写罚 `log 0.5`、全拼可达时
  缩写路径剪枝、补全罚 `log 0.05`、排序 = 可信度 + `ln(权重)`、上限 20；字查音+虎（`character_lookup.rs`）
  取光标左侧 1 字，上排拼音（排头「咅」）、下排虎码（排头「虍」）。两者触发键可配置，**仅单字符触发键**
  给默认可上屏候选。详见 [`../crates/hux-addon/README.md`](../crates/hux-addon/README.md)。
- **学习**：提交点通知器内建于核心路径（`confirm_selection`、自动上屏）；宿主排空
  `LiveLearning::submitted` 落库并在 `store_ready` 后生效；存储
  `<user>/tiger_sentence_learning_<hash(schema_id)>.userdb/`（1 万条 / 16 MiB，60 秒节流刷新）。
- **选项与配置**：`tiger_sentence.options.yaml`（主）+ legacy `user.yaml` 的 `var/option/*`（只读回退）；
  合并顺序 **options.yaml > 设置 > 内建缺省**；图形配置由 C++ `HuxConfig` schema 生成（引擎 9 项 +
  宿主显示项 `PanelPreedit`），经 `hux_engine_apply_settings` 应用；状态菜单 4 项核心开关待接线。

## 6. 测试

1. **Rust 差分**：模块对金样逐位断言（fixture 入库；真实模型本地/定期）；
2. **键序列金样**：真 librime 探针生成「键序列 → 提交/候选/预编辑」，Rust 重放比对；
3. **CI**：fmt/clippy/差分 + 以固定参照提交重生成 fixture 金样比对（溯源校验），见
   [`../goldens/README.md`](../goldens/README.md)。
