<!-- SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com> -->
<!-- SPDX-License-Identifier: GPL-3.0-or-later -->

# 配置项

配置入口：`fcitx5-configtool` → 「hux」页。设置页分「行为」「快捷键」两个分区（与 fcitx5
全局设置同款；选项详情可悬浮查看），保存写入 `~/.config/fcitx5/conf/hux.conf`。

## 行为（布尔项在前，值选项在后）

| 项 | 默认 | 说明 |
|---|---|---|
| EarlyCommit / EarlyCommitToPreedit / AllowDuplicateSingle | 开/关/开 | 早提交三项 |
| FullShape / AsciiPunct | 关/关 | 全角标点 / ASCII 标点直通 |
| TabLearning | 开 | Tab 选字写学习库 |
| DigitSelect | 关 | 数字直选：`1`–`9` 上屏当前页候选，`0`=第 10 个 |
| PanelPreedit | 关 | 候选窗口显示预编辑文本（仅宿主显示项，不经引擎）|
| HighFreqLimit | 1500 | 高频字过滤上限（重启生效）|
| PageSize | 5 | 每页候选个数（1–10）|

## 快捷键（`KeyList`，均可多项）

| 项 | 默认 | 说明 |
|---|---|---|
| SoundToCharShapeKey | Alt+`:` | 音反查（拼音 → 虎码）|
| CharToSoundShapeKey | Alt+`"` | 字反查（光标左侧汉字的拼音与虎码）|
| PageUpKey / PageDownKey | `-`、`[` / `=`、`]` | 翻页（有候选时生效）|

快捷键为 fcitx5 `KeyList`（配置工具与「全局设置」同款，可配置多项；`AllowModifierLess`
允许 `` ` ``、`;` 等无修饰键）。
未显式提供的项跟随 fcitx5 全局设置（候选列表方向、客户端内联预编辑）。

## 选项与学习存储

- 选项：`~/.local/share/fcitx5/hux/tiger_sentence.options.yaml` 为主存储（YAML，未知键保留），
  缺失键回退 `user.yaml` 的 `var/option/<name>`（只读）；合并顺序 **options.yaml > 设置 > 内建缺省**；
  保存失败写属性 `tiger_sentence_options_error`。
- 学习库：`~/.local/share/fcitx5/hux/tiger_sentence_learning_<hash(schema_id)>.userdb/`
  （LevelDB 同构、可直接迁移；1 万条 / 16 MiB，60 秒节流刷新）；提交点通知器由 core 负责，
  宿主排空 `LiveLearning::submitted` 落库。
