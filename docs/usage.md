<!-- SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com> -->

<!-- SPDX-License-Identifier: GPL-3.0-or-later -->

# 开发 / 安装 / 使用 / 卸载

## 开发

依赖：Rust 1.85+（edition 2024）。

```sh
cargo test -p hux-core -p hux-addon  # 逐位差分 + addon 测试（本地抽样缺失自动跳过）
cargo clippy --all-targets -- -D warnings
cargo fmt --all --check
```

设计、模块映射与测试说明见 [`rust-migration.md`](rust-migration.md)； \
差分金样与复现命令见 [`../goldens/README.md`](../goldens/README.md)。

开发可用环境变量覆盖数据目录与模型（目录冒号分隔；`data/` 已含全部随包数据）：

```sh
HUX_DATA_DIRS="data" \
HUX_MODEL="$HOME/.local/share/fcitx5/rime/models/sentence-ngram-mobile.bin" \
fcitx5 -r -d
```

运行时数据目录查找顺序：`~/.local/share/fcitx5/hux` → `/usr/share/fcitx5/hux`； \
模型另在 `models/sentence-ngram-mobile.bin` 查找。

## 安装

一键安装（构建 → 装插件与随包数据 → 重启 fcitx5）：

```sh
./install.sh            # 预览将执行的命令：./install.sh --dry-run
```

脚本结尾会提示自行获取 n-gram 模型（见下）。\
手工安装（自定义前缀或打包时参考）：

依赖：CMake 3.20+，以及提供 CMake 包 `Fcitx5Core` 的 fcitx5 开发文件。 \
（Arch：`fcitx5`；Fedora：`fcitx5-devel`；Debian/Ubuntu：`libfcitx5core-dev`）

```sh
cmake -S crates/hux-addon -B build/addon \
    -DCMAKE_BUILD_TYPE=Release \
    -DCMAKE_INSTALL_PREFIX=/usr
cmake --build build/addon -j
sudo cmake --install build/addon
fcitx5 -r -d  # 或以所在发行版的方式重启
```

安装产物：

- `/usr/lib/fcitx5/libhux.so`
- `/usr/share/fcitx5/{addon,inputmethod}/hux.conf`

随包数据安装到 fcitx5 数据目录： \
（data/：码表四件套、词先验位图、音反查索引、标点表）

```sh
mkdir -p ~/.local/share/fcitx5/hux/models
cp data/tiger_sentence.* data/symbols.yaml ~/.local/share/fcitx5/hux/
```

也可装到系统级 `/usr/share/fcitx5/hux/`。

[n-gram 模型](https://github.com/lvyww/tiger-sentense-rime/releases/tag/model) 不随包。
放入用户级或系统级目录即可。

- 用户级 `~/.local/share/fcitx5/hux/models/`
- 系统级 `/usr/share/fcitx5/hux/models/`

随后在配置工具中添加「虎虚」（hux）即可使用。

## 使用

- `空格` 高亮项上屏
- 鼠标点击候选：选中并上屏
- `Left/Right` 码标移动
- `Up/Down` 或 `Tab / Shift+Tab` 高亮选择
- `-/=` 或 `[/]` 或 `PgUp/PgDn` 翻页
- `Enter` 提交原文，`Esc` 取消。
- `Alt+:` 音反查
- `Alt+"` 字反查

![虎句](images/虎句.png)

配置项见 [`config.md`](config.md)（可在配置工具的「虎虚」页修改）。

状态菜单「**虎虚**」子菜单可随时切换： \
提前上屏、提前上屏至预编辑、单字重码组句、全角标点、数字直选 \
（写入 `tiger_sentence.options.yaml`，重启后保持）。

![虍](images/虍.png)

### 音反查

输入拼音（支持缩写），候选为字词，注释即虎码

- `Esc` / 再次触发：退出

```
Alt+:  zhongguo   →   :zhong guo〔拼音〕   候选：中国 …
```

![音反查](images/音反查.png)

### 字反查

依赖于应用提供周边文本，不可用时信息为空（比如终端） \
输入面板显示光标左侧 1 个字的信息：（缺数据为 '?'） \
上排拼音（排头「**咅**」）、下排虎码（排头「**虍**」）

- `方向键` 移动应用光标（信息随光标刷新）
- `Esc` / 再次触发 / 输入其它键：退出

```
咅 zhong
虍 d/dg/dgs
```

![字反查](images/字反查.png)

## 卸载（无残留）

一键卸载：

```sh
./uninstall.sh          # 移除系统文件，保留用户数据（选项/学习库/模型）
./uninstall.sh --purge  # 连用户数据一起清除
```

手工步骤：

```sh
# 系统级
sudo rm -rf /usr/lib/fcitx5/libhux.so \
            /usr/share/fcitx5/addon/hux.conf \
            /usr/share/fcitx5/inputmethod/hux.conf \
            /usr/share/fcitx5/hux

# 用户级
rm -rf ~/.local/share/fcitx5/hux \
       ~/.config/fcitx5/conf/hux.conf

# 重启
fcitx5 -r -d  # 或以发行版所支持的方式
```
