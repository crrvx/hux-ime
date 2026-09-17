# hux-addon（K3）

fcitx5 addon：**C++ 薄壳**（`shell/`，只做 fcitx5 接口适配）+ **Rust 逻辑**（`src/`，经 C ABI
调用 `hux-core`）。按键 → Rust（core `processor`/`translate`）→ 提交 / preedit / 候选 → fcitx5。

## 构建与安装

```sh
cmake -S crates/hux-addon -B build/addon \
  -DCMAKE_BUILD_TYPE=Release -DCMAKE_INSTALL_PREFIX=/usr
cmake --build build/addon -j
sudo cmake --install build/addon      # /usr/lib/fcitx5/libhux.so + 两个 conf
```

安装后重启 fcitx5（`fcitx5 -r -d`），在配置工具中添加「hux-ime」（方案：虎句）。

## 数据目录

默认按 `~/.local/share/fcitx5/hux` → `/usr/share/fcitx5/hux` 查找
（码表四件套、`tiger_sentence.lexical.bin`、`symbols.yaml`、可选 `models/sentence-ngram-mobile.bin`）；
发布默认 `symbols.yaml` 取自参照、覆盖 half_shape 的 `/`（提交 `/` 而非 `、`），见 `data/symbols.yaml`。
开发可用环境变量覆盖（目录冒号分隔 / 模型路径）：

```sh
HUX_DATA_DIRS="$PWD/goldens/lexicon:$PWD/data" \
HUX_MODEL="$HOME/.local/share/fcitx5/rime/models/sentence-ngram-mobile.bin" \
fcitx5 -r -d
```

开发数据也可直接铺到用户目录（免环境变量）：

```sh
mkdir -p ~/.local/share/fcitx5/hux/models
cp goldens/lexicon/*.txt data/tiger_sentence.lexical.bin ~/.local/share/fcitx5/hux/
ln -sf ~/.local/share/fcitx5/rime/models/sentence-ngram-mobile.bin \
  ~/.local/share/fcitx5/hux/models/
```

## 配置（fcitx5-configtool 生成的设置页）

| 项 | 默认 | 说明 |
|---|---|---|
| PinyinLookupKey | Alt+`;` | **音查虎**（用拼音查虎码）触发键（⑧-1）|
| CharacterLookupKey | Alt+`'` | **字查音+虎**（查光标左侧汉字的拼音与虎码）触发键（⑧-2）|
| QuickInputKey | `;` | 快速输入触发键（按键录入；行为随 ⑨ 数据接线）|
| EarlyCommit / EarlyCommitToPreedit / AllowDuplicateSingle | 开/关/开 | 早提交三项 |
| FullShape / AsciiPunct | 关/关 | 全角标点 / ASCII 标点直通 |
| TabLearning | 开 | Tab 选字写学习库 |
| HighFreqLimit | 1500 | 高频字过滤上限（重启生效）|
| PanelPreedit | 关 | 候选窗口显示预编辑文本（仅宿主显示项，不经引擎）|

快捷键选项用 `fcitx5` 原生按键录入控件（`KeyConstrain(AllowModifierLess)`，允许 `` ` ``、`;`
这类无修饰键）。

未显式提供的项一律**跟随 fcitx5 全局设置**：候选列表方向不设布局提示（由全局
「候选词排列方向」决定）、客户端内联预编辑跟随全局「预编辑」开关（`isPreeditEnabled()`）。

预编辑串**按词/音节分码**：正常段用解码的 `segmented`（如输入 `shks` → 预编辑 `sh ks`，
输入 `shk` → `shk`）；音查虎段按音节切分（`` `zhongguo `` → `` `zhong guo ``，缩写/未完成
音节与后续合并，如 `` `zho `` → `` `zho ``）。

**音查虎 / 字查音+虎**（⑧-1/⑧-2）同机制：触发键推入组合（触发键可配置；**默认 Alt+`;`
与 Alt+`'`**）；**仅当触发键为单字符键（无 Ctrl/Alt/Super）时**给出默认可上屏候选（触发字符，
按标点表取半/全角，空格上屏），带修饰键的触发不给默认候选。

**字查音+虎**：取应用侧周边文本（fcitx5 surrounding text，需应用支持；不支持时上排提示
「应用不支持周边文本」）；**两排显示光标左侧 1 个字**——**上排（排头「咅」）= 拼音、下排
（排头「虍」）= 虎码**（如 `咅 zhong` / `虍 d/dg/dgs`；多音/多码以 `/` 连接，缺数据为 `?`，
空白字符跳过显示）；**←/→/↑/↓ 交应用处理**（应用光标随动，本层不消费；查码段**不下发预编辑**，
避免应用端 marked text 锁住光标；松开按键的 release 事件用于刷新两排）；Esc / 再次触发 /
其它任意键退出（打字照常输入）。展示面为输入面板辅助文本条（auxUp/auxDown）。

## 选项

`~/.local/share/fcitx5/hux/tiger_sentence.options.yaml` 为主存储（YAML，未知键保留）；
缺失键回退 `user.yaml` 的 `var/option/<name>`（只读）。保存失败写入属性
`tiger_sentence_options_error`。

## 学习

`~/.local/share/fcitx5/hux/tiger_sentence_learning_<hash(schema_id)>.userdb/`
（LevelDB：键 `e/%010d`、值 = frame 五元组；上限 1 万条 / 16 MiB；`refresh_scores` 60 秒节流，
与 Rime 同构、可直接迁移）。提交点的通知器序列由 core 负责，宿主排空
`LiveLearning::submitted` 落库。

## 状态

- K3a：注册（addon/输入法条目 conf）+ 按键回路；
- K3b：core 会话接线——fcitx5 状态 → core（Rime）掩码、`CompositionBuilder` 组合重建
  （提交或输入变化时重建）、提交 / preedit（字节光标）/ 候选与高亮、
  `activate/deactivate/reset` 生命周期、`_auto_commit`；
- K3d：选项持久化（`options.yaml` + legacy 回退 + 错误属性）；
- K3e：学习库（LevelDB 落库 + 节流刷新 + `_hide_candidate` / `ascii_mode` 确认）；
- K3f（⑥）：宿主编辑语义——core `host` 模块（librime `key_binder`/`selector`/`navigator`/
  `express_editor` 等价物）+ 组合重建随光标（`CompositionBuilder` 参照 `ConcreteEngine::Compose`）；
  2c 键序列金样扩到 36 例/200 步（编辑/导航键、缓冲/锁定态）；
- K3g（⑦a，**已移除**）：曾实现 ascii_composer 等价物（Shift 轻击/跟随系统 caps 的英文模式）——按设计取舍移除，英文输入交由 fcitx5 切换输入法。
- K3h（⑦b）：标点表（`symbols.yaml` half/full shape、`{commit}`/标量/`{pair}` 交替）；
  金样扩到 59 例/269 步；随后修正 editor `char_handler`（61/273）；移除英文模式后 **55 例/236 步（当前）**。

- K3i（配置，Rust 半）：`settings.rs` 配置模型（早提交三项/full_shape/ascii_punct/
  tab_learning/high_freq_limit；合并顺序 options.yaml > 设置 > 内建缺省）；
  图形配置：C++ `HuxConfig` schema（11 项：引擎 10 + 宿主显示 1）+ `getConfig/setConfig`，
  fcitx5-configtool 自动生成设置页（`~/.config/fcitx5/conf/hux.conf`），经 ABI `hux_engine_apply_settings` 生效。

已知限制（后续增量）：每引擎单会话（切换/重置即清空）；候选为展示型（点击不提交）；
音查虎（⑧-1）与字查音+虎（⑧-2）均已接线；状态菜单与打包（⑨）待做。
