<!-- SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com> -->
<!-- SPDX-License-Identifier: GPL-3.0-or-later -->

# 开发 / 安装 / 使用 / 卸载

## 开发

依赖：Rust 1.85+（edition 2024）。

```sh
cargo test --workspace          # 逐位差分 + 配置 + 平台适配（本地抽样缺失自动跳过）
cargo clippy --all-targets -- -D warnings
cargo fmt --all --check
reuse lint                      # 许可标注（CI 亦跑）
```

设计、模块映射与测试说明见 [`design.md`](design.md)； \
差分金样的清单、重新生成命令与校验和见 [`../goldens/README.md`](../goldens/README.md)； \
性能基准与基线见 [`design.md`](design.md) §6（`cargo run --release --example {decode_bench,key_bench}`）。 \
**一键安装 / 卸载命令只写在根 [`README.md`](../README.md)「快速指南」**——本文只留手工步骤与产物清单。

开发可用环境变量覆盖数据目录与模型（目录冒号分隔；`data/` 已含全部随包数据）：

```sh
HUX_DATA_DIRS="data" \
HUX_MODEL="$HOME/.local/share/fcitx5/rime/models/sentence-ngram-mobile.bin" \
fcitx5 -r -d
```

运行时数据目录查找顺序（平台层解析）：`HUX_DATA_DIRS`（覆盖）→ `$XDG_DATA_HOME/fcitx5/hux`
（缺省 `~/.local/share/fcitx5/hux`）→ `$XDG_DATA_DIRS/*/fcitx5/hux`
（缺省 `/usr/local/share`、`/usr/share`）；选项 / 学习库 / 模型写入用户目录； \
模型另在 `models/sentence-ngram-mobile.bin` 查找。

## 安装

一键安装（①依赖检查 ②构建 ③安装 ④校验落盘 ⑤提示；`--dry-run` 预览）见根
[`README.md`](../README.md)「快速指南」——命令只留那处。脚本按落点分两种模式，**互斥**：

| 模式 | 落点前缀 | 权限 | 生效范围 |
| ---- | -------- | ---- | -------- |
| `-s` / `--system`（缺省） | `/usr` | 需要 sudo | 全机 |
| `-u` / `--user` | `$HOME/.local` | 无需 sudo | 当前用户 |

装完脚本**不自动重启** fcitx5：按结尾提示自行重启（`nohup fcitx5 -r -d >/dev/null 2>&1 &`），
再在配置工具里添加「虎虚（hux）」。

### 用户级（`-u`）的环境变量

fcitx5 的 addon 目录**没有用户级缺省值**：用户级安装靠环境变量 `FCITX_ADDON_DIRS` 指定，
且该变量**取代**缺省搜索集，故必须显式带上系统目录。`-u` 会写（已存在且内容相同则不改写）：

`~/.config/environment.d/90-hux.conf`

```sh
FCITX_ADDON_DIRS=$HOME/.local/lib/fcitx5:/usr/lib/fcitx5
```

生效条件是 systemd 用户实例在**登录时**读入 `environment.d`（值里的 `$HOME` 由 systemd 展开）：

- 脚本装完检测当前会话是否已继承（`systemctl --user show-environment` 里有 `FCITX_ADDON_DIRS`）。
- 已继承 ⇒ 重启 fcitx5 即可加载插件。
- **未继承（或没有 systemd 用户实例）⇒ 当前会话不会继承该变量**：需在启动 fcitx5 前
  `export FCITX_ADDON_DIRS=$HOME/.local/lib/fcitx5:/usr/lib/fcitx5`，或改用 `-s`——脚本会明确提示，
  不会假装成功。
- 重新登录后的新会话自然带上它。没带该变量时 fcitx5 找不到 `libhux.so`，输入法列表里没有「虎虚」。

### 产物清单

`<prefix>` = `/usr`（`-s`）或 `$HOME/.local`（`-u`）：

- `<prefix>/lib/fcitx5/libhux.so`：插件库。系统级跟随 fcitx5 自身的 addon 目录（Fedora 为
  `lib64/fcitx5`，Debian/Ubuntu 为 multiarch 的 `lib/<triplet>/fcitx5`）；用户级固定
  `lib/fcitx5`——CMake 开关 `HUX_RELATIVE_ADDON_DIR=ON` 把安装目标写成相对路径，
  否则 `FCITX_INSTALL_ADDONDIR` 的绝对路径无法随 `--prefix` 重定位。
- `<prefix>/share/fcitx5/{addon,inputmethod}/hux.conf`：插件与输入法条目。
- `<prefix>/share/icons/hicolor/{scalable,48x48,22x22}/apps/hux.{svg,png}`：输入法条目 / 托盘图标。
- `<prefix>/share/fcitx5/themes/hufu-*/`：**共享主题** 19 套（fcitx5 主题形态，取自虎符官方皮肤；
  清单 `assets/themes/MANIFEST`，说明见 [`../assets/themes/README.md`](../assets/themes/README.md)）。
  选用：`fcitx5-configtool` →「附加组件」→「经典界面」→ 主题，或改
  `~/.config/fcitx5/conf/classicui.conf` 的 `Theme=`（fcitx5 会合并系统级与用户级主题目录）。
- `<prefix>/share/fcitx5/hux/`：**随包数据**（`data/MANIFEST` 列出的码表四件套 + 追加码表
  `tiger_sentence.codes.huma.txt`（生僻字可打，10.3 万字）、词先验位图、音反查索引、标点表）
  ——由 `cmake --install` 按同一清单一并安装；缺了它，引擎的 `Lexicon` /
  `PunctTable` 静默降级（打字无输出 / 无标点）。追加码表按「主表 → 追加表」拼接，主表 rank 与
  简码分配不变；编排见 [`../data/README.md`](../data/README.md) 的「追加码表」。
- 仅用户级：`~/.config/environment.d/90-hux.conf`（见上）。

两份清单（`data/MANIFEST`、`assets/themes/MANIFEST`）是安装 / 卸载 / CMake 的**单一来源**：
`install.sh` 装后逐条核对落盘（缺任一即失败），`uninstall.sh` 按同一清单删除，
`tools/checks/check_data_manifest.sh` 与 CI 的 `DESTDIR` 步骤守护三处一致。

手工安装（自定义前缀或打包时参考）：

依赖：CMake 3.20+，以及提供 CMake 包 `Fcitx5Core` 的 fcitx5 开发文件。 \
（Arch：`fcitx5`；Fedora：`fcitx5-devel`；Debian/Ubuntu：`libfcitx5core-dev`）

```sh
cmake -S platform/fcitx5 -B build/addon \
    -DCMAKE_BUILD_TYPE=Release \
    -DCMAKE_INSTALL_PREFIX=/usr
cmake --build build/addon -j
sudo cmake --install build/addon
fcitx5 -r -d  # 或以所在发行版的方式重启
```

安装产物：

- `/usr/lib/fcitx5/libhux.so`（系统级跟随 fcitx5 的 addon 目录：Fedora 为 `lib64/fcitx5`，
  Debian/Ubuntu 为 multiarch 的 `lib/<triplet>/fcitx5`）
- `/usr/share/fcitx5/{addon,inputmethod}/hux.conf`
- `/usr/share/icons/hicolor/{scalable,48x48,22x22}/apps/hux.{svg,png}`（输入法条目 / 托盘图标）
- `/usr/share/fcitx5/themes/hufu-*/`：共享主题 19 套（清单 `assets/themes/MANIFEST`）
- `/usr/share/fcitx5/hux/`：随包数据（`data/MANIFEST`）

用户级前缀同理：前缀换成 `$HOME/.local`，并加 `-DHUX_RELATIVE_ADDON_DIR=ON`——缺省取 fcitx5 的
**绝对** addon 目录，`--prefix` 无法把它重定位到 `lib/fcitx5`：

```sh
cmake -S platform/fcitx5 -B build/addon \
    -DCMAKE_BUILD_TYPE=Release \
    -DCMAKE_INSTALL_PREFIX="$HOME/.local" \
    -DHUX_RELATIVE_ADDON_DIR=ON
cmake --build build/addon -j
cmake --install build/addon   # 无需 sudo
```

数据也可单独放到用户级目录（引擎按「用户目录 → 系统目录」查找）：

```sh
mkdir -p ~/.local/share/fcitx5/hux/models
cp data/tiger_sentence.* data/symbols.yaml ~/.local/share/fcitx5/hux/
```

[n-gram 模型](https://github.com/lvyww/tiger-sentense-rime/releases/tag/model) 不随包。
放入用户级或系统级目录即可。

- 用户级 `~/.local/share/fcitx5/hux/models/`
- 系统级 `/usr/share/fcitx5/hux/models/`

随后在配置工具中添加「虎虚」（hux）即可使用。

## 使用

- `空格` 高亮项上屏
- 鼠标点击候选：选中并上屏
- `Left/Right` 光标移动（字节偏移）
- `Up/Down` 或 `Tab / Shift+Tab` 高亮选择
- `PgUp/PgDn` 翻页；`-`/`=`、`[`/`]` 同为翻页键，**菜单可见时优先翻页**（`=` 下翻一页、
  翻过页后 `-` 上翻一页，都不上屏组合）——此处有意偏离上游 `abad411`：上游会让标点分支先上屏
  组合再落标点，从而遮蔽翻页绑定（见 [`upstream-deviations.md`](upstream-deviations.md) ①）；**未翻页的** `-` 仍按上游
  行为先上屏组合再落 `-`。其余 ASCII 标点维持「先上屏组合再落标点」（见 [`config.md`](config.md)）
- `Enter` 提交原文，`Esc` 取消。
- `` ` `` 音反查
- `~` 字反查

![虎句](images/虎句.png)

配置项见 [`config.md`](config.md)（可在配置工具的「虎虚」页修改）。

状态菜单「**虎虚**」子菜单可随时切换： \
提前上屏、提前上屏至预编辑、单字重码组句、全角标点、数字直选（与配置页「行为」分区是同一批项）、\
启用全字集、过滤非汉字（与配置页「字集」分区是同一批项）\
——改动即时生效，并双向同步 `tiger_sentence.options.yaml` 与 `conf/hux.conf`，重启后保持。 \
其中「启用全字集」关掉后只装主表码表（9,794 字），生僻字不再可打（省约 88 MB 常驻、启动快约 0.13 s）；\
「过滤非汉字」只作用于追加码表里落在 CJK 统一表意文字区段之外的符号（部首/笔画/注音/假名/兼容汉字等 939 条），主表自带的标点/假名不受影响。 \
装载结果（几张码表、条目/字数、两个开关的生效值）会随启动日志与「重新部署」各输出一行 `hux: data …`。 \
此外还有「候选窗口显示预编辑」\
（写入 `conf/hux.conf`，切换即时生效）、「重新部署」（重读配置与选项存储、重装数据与模型、\
重置全部会话）与一行模型信息（`模型：<文件名> — <状态>`）。

![虍](images/虍.png)

### 音反查

输入拼音（支持缩写），候选为字词，注释即虎码

- `Esc` / 再次触发：退出

```
`  zhongguo   →   `zhong guo〔拼音〕   候选：中国 …
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

### 图标与托盘显示

图标名是 `hux`（输入法条目 `[InputMethod] Icon=hux`；状态区菜单动作也由引擎显式 `setIcon("hux")`），
由桌面按图标主题解析到 `<prefix>/share/icons/hicolor/{scalable,48x48,22x22}/apps/hux.{svg,png}`
——托盘、右键菜单与配置工具的输入法列表都用它。两处常见困惑：

- **托盘显示「虍」而不是图标**：fcitx5 经典界面的「优先使用文字图标」（`~/.config/fcitx5/conf/classicui.conf`
  的 `PreferTextIcon`）为开时，托盘渲染输入法条目的 `Label`（本包为 `虍`）而非图标。要图标就在
  fcitx5-configtool →「附加组件」→「经典界面」里取消该勾选；想换文字则改 `conf/hux.inputmethod.conf`
  的 `Label`（改完重装或直接改 `<prefix>` 下已装的那份）。
- **重装后仍是旧图标**：Qt 系程序（面板、fcitx5）在启动时建立图标索引并缓存像素图，替换文件后不会
  重读；KDE 下右键菜单由桌面面板绘制，所以除了重启 fcitx5，还要重启桌面面板（KDE：
  `kquitapp6 plasmashell && kstart plasmashell`，或注销重登）。`install.sh` 已尽力刷新 hicolor 的
  `icon-theme.cache`；GTK 侧必要时再 `sudo gtk-update-icon-cache -f -t /usr/share/icons/hicolor`。
  核对落盘是否为当前图标：`sha256sum /usr/share/icons/hicolor/*/apps/hux.*` 与
  `assets/branding/` 下同名文件比对。

## 卸载（无残留）

一键卸载见根 [`README.md`](../README.md)「快速指南」——命令只留那处。`./uninstall.sh` 是**交互式**的：
先探测系统级与用户级两处安装，只对存在的项提问与操作，开始前依次问三件事：

1. 是否卸载共享主题？`[Y/n]`——`<prefix>/share/fcitx5/themes/hufu-*`（`assets/themes/MANIFEST`）。
2. 是否卸载模型？`[y/N]`——`<prefix>/share/fcitx5/hux/models/*.bin`，**缺省保留**（体积大、可复用）。
3. 是否删除用户数据？`[y/N]`——选项 / 学习库 / `conf/hux.conf`，**缺省保留**。

其余（插件库、`{addon,inputmethod}/hux.conf`、图标、随包数据、`-u` 写的
`~/.config/environment.d/90-hux.conf`）缺省都卸；要连模型与用户数据一起清除，就在第 2、3 问回答
`y`。开关只有两个（`--help` 里同样写明）：

| 开关 | 作用 |
| ---- | ---- |
| `--dry-run` | 只打印将执行的命令与「计划删除清单」：不提问、不上色、不删除 |
| `-h` / `--help` | 显示帮助 |

`--dry-run` 的「计划删除清单」是机器可解析的（守卫 `tools/checks/check_uninstall_clean.py` 按它取
可卸载集合）：每行 `<标记> <绝对路径>`，`-` = 按缺省会删、`?` = 回答 `y` 才删（模型 / 用户数据）；
绝对路径一行一条，整个目录以 `/` 结尾，通配用 `*`（如 multiarch 的 addon 目录）。清单按固定落点
静态给出，不随探测结果变化。卸载结束打印清单：已卸载的逐项路径 / 套数，以及**未卸载**的
逐项与原因（模型保留、用户数据保留、目录里还有清单之外的文件等）。脚本不自动重启 fcitx5，
结尾给重启命令。

手工步骤：

```sh
# 系统级
sudo rm -rf /usr/lib/fcitx5/libhux.so \
            /usr/share/fcitx5/addon/hux.conf \
            /usr/share/fcitx5/inputmethod/hux.conf \
            /usr/share/icons/hicolor/{scalable,48x48,22x22}/apps/hux.{svg,png} \
            /usr/share/fcitx5/themes/hufu-* \
            /usr/share/fcitx5/hux

# 用户级（-u 装的那套）
rm -rf ~/.local/lib/fcitx5/libhux.so \
       ~/.local/share/fcitx5/{addon,inputmethod}/hux.conf \
       ~/.local/share/icons/hicolor/{scalable,48x48,22x22}/apps/hux.{svg,png} \
       ~/.local/share/fcitx5/themes/hufu-* \
       ~/.local/share/fcitx5/hux \
       ~/.config/environment.d/90-hux.conf \
       ~/.config/fcitx5/conf/hux.conf

# 重启
fcitx5 -r -d  # 或以发行版所支持的方式
```

> 上面的 `rm -rf /usr/share/fcitx5/hux` 会连**用户自取**的 n-gram 模型一起删掉；`./uninstall.sh`
> 缺省只删随包数据与插件本身，模型与用户数据都保留（第 2、3 问都回答 `y` 才连它们一起删）。
> 若要手工卸载又保留模型，只删 `data/MANIFEST` 列出的文件（`tiger_sentence.*` 与 `symbols.yaml`）：
>
> ```sh
> while IFS= read -r entry; do
>     case "$entry" in ''|'#'*) continue ;; esac
>     sudo rm -f "/usr/share/fcitx5/hux/$(basename "$entry")"
> done < data/MANIFEST
> ```
>
> 插件库目录兼容 `lib64` 与 multiarch：`/usr/lib64/fcitx5/libhux.so`（Fedora 等）、
> `/usr/lib/<triplet>/fcitx5/libhux.so`（Debian/Ubuntu）。
