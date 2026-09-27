<!-- SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com> -->
<!-- SPDX-License-Identifier: GPL-3.0-or-later -->

# 构建 / 安装 / 卸载

- 快速上手（一键命令）见根 [`README.md`](../README.md)「快速指南」
- 本文：开发构建、安装与产物落点、卸载细则
- 使用（按键 / 反查 / 托盘）见 [`usage.md`](usage.md)

## 构建（开发）

依赖 Rust 1.85+（edition 2024）：

```sh
cargo test --workspace  # 逐位差分 + 配置 + 平台适配（本地抽样缺失自动跳过）
cargo clippy --all-targets -- -D warnings
cargo fmt --all --check
reuse lint              # 许可标注（CI 亦跑）
```

- 设计与测试纪律见 [`design.md`](design.md)（性能基准见 §10）
- 模块映射、数据与目录见 [`reference.md`](reference.md)
- 金样清单与格式见 [`../goldens/README.md`](../goldens/README.md)
- 金样再生见 [`../goldens/REGENERATE.md`](../goldens/REGENERATE.md)，校验和见 \
  [`../goldens/PROVENANCE.md`](../goldens/PROVENANCE.md)

调试用环境变量（查找顺序见 [`../platform/README.md`](../platform/README.md)）：

- `HUX_DATA_DIRS`：数据目录（冒号分隔），开发时指向 `data/`
- `HUX_MODEL`：模型文件
- 随包数据 `data/` 已含（模型除外）

```sh
HUX_DATA_DIRS=data \
HUX_MODEL="$HOME/.local/share/fcitx5/hux/models/sentence-ngram-mobile.bin" \
fcitx5 -r -d
```

## 安装

`install.sh` 分两种落点，**互斥**；另有 `--dry-run`（只打印命令）与 `-h` / `--help`：

| 模式 | 落点前缀 | 权限 | 生效范围 |
| --- | --- | --- | --- |
| `-s` / `--system`（缺省） | `/usr` | 需要 sudo | 全机 |
| `-u` / `--user` | `$HOME/.local` | 无需 sudo | 当前用户 |

- 脚本**不自动重启** fcitx5：按结尾提示自行重启

### 用户级（`-u`）的环境变量

`-u` 会写 `~/.config/environment.d/90-hux.conf`：

```sh
FCITX_ADDON_DIRS=$HOME/.local/lib/fcitx5:/usr/lib/fcitx5
```

- addon 目录无用户级缺省值，`FCITX_ADDON_DIRS` **取代**缺省搜索集 ⇒ 必须带上系统目录 \
  （依据见 [`resources.md`](resources.md) §10.1）
- 装后查 `systemctl --user show-environment`：已含该变量 ⇒ 重启 fcitx5 即可
- 未继承（或无 systemd 用户实例）⇒ 启动前手动 export： \
  `export FCITX_ADDON_DIRS=$HOME/.local/lib/fcitx5:/usr/lib/fcitx5`，或改用 `-s`（脚本会提示）

### 产物清单

- 落点见 [`../platform/README.md`](../platform/README.md)；资源见 [`resources.md`](resources.md)
- `<prefix>/lib/fcitx5/libhux.so`：插件库 \
  用户级固定 `lib/fcitx5`；系统级跟随发行版 addon 目录，可能是 `lib/fcitx5`、`lib64/fcitx5` \
  或 multiarch `lib/<triplet>/fcitx5`
- `<prefix>/share/fcitx5/{addon,inputmethod}/hux.conf`：插件与输入法条目
- `<prefix>/share/icons/hicolor/{scalable,48x48,22x22}/apps/hux.{svg,png}`：条目图标兼托盘图标
- `<prefix>/share/fcitx5/themes/hufu-*/`：共享主题 19 套 \
  （清单 `assets/themes/MANIFEST`）；选用见 \
  [`../assets/themes/README.md`](../assets/themes/README.md)
- `<prefix>/share/fcitx5/hux/`：随包数据——`data/MANIFEST` 的码表四件套、追加码表 \
  `tiger_sentence.codes.huma.txt`、词先验位图、音反查索引、标点表
- 两份 MANIFEST 是装 / 卸 / CMake 的**单一来源**：`install.sh` 核对、`uninstall.sh` 按它删除 \
  守卫见 `tools/checks/check_data_manifest.sh` 与 CI 的 `DESTDIR` 步；细节见 \
  [`../data/README.md`](../data/README.md)

手工安装（Linux 落点）依赖 CMake 3.20+ 与 fcitx5 开发文件（CMake 包 `Fcitx5Core`； \
Arch `fcitx5`、Fedora `fcitx5-devel`、Debian/Ubuntu `libfcitx5core-dev`）：

```sh
cmake -S platform/linux -B build/addon \
    -DCMAKE_BUILD_TYPE=Release \
    -DCMAKE_INSTALL_PREFIX=/usr
cmake --build build/addon -j
sudo cmake --install build/addon
fcitx5 -r -d  # 或以所在发行版的方式重启
```

- 用户级：前缀换 `$HOME/.local`，加 `-DHUX_RELATIVE_ADDON_DIR=ON`（addon 目录写相对路径），免 sudo
- 共用层 `platform/fcitx5/` 不能单独安装；Android 落点见 \
  [`../platform/android/README.md`](../platform/android/README.md)

数据也可单独放到用户级目录：

```sh
mkdir -p ~/.local/share/fcitx5/hux/models
cp data/tiger_sentence.* data/symbols.yaml ~/.local/share/fcitx5/hux/
```

- n-gram 模型不随包，需自取（[上游](https://github.com/lvyww/tiger-sentense-rime/releases/tag/model)）\
  放入用户级 `~/.local/share/fcitx5/hux/models/` 或系统级 `/usr/share/fcitx5/hux/models/`
- 不装也能用，仅整句排序略弱（见 [`resources.md`](resources.md) §2）
- 装完在配置工具里添加「虎虚」（hux）

## 卸载（无残留）

`./uninstall.sh` 交互式：探测系统级 / 用户级两处安装，只对存在的项提问，依次三问：

1. 是否卸载共享主题？`[Y/n]`——落点 `<prefix>/share/fcitx5/themes/hufu-*` \
   清单见 `assets/themes/MANIFEST`
2. 是否卸载模型？`[y/N]`——落点 `<prefix>/share/fcitx5/hux/models/*.bin` \
   **缺省保留**（体积大、可复用）
3. 是否删除用户数据？`[y/N]`——选项 / 学习库 / `conf/hux.conf`，**缺省保留**

- 其余（插件库、`{addon,inputmethod}/hux.conf`、图标、随包数据、用户级的 \
  `~/.config/environment.d/90-hux.conf`）**缺省都卸**
- 模型与用户数据要清除，就在第 2、3 问答 `y`
- 结束打印已卸载的逐项路径 / 套数，以及**未卸载**的逐项与原因 \
  （模型保留、用户数据保留、目录里还有清单之外的文件等）
- 脚本不自动重启，结尾给重启命令

`--dry-run` 只打印，不提问、不上色、不删除；其「计划删除清单」机器可解析 \
（守卫 `tools/checks/check_uninstall_clean.py` 取可卸载集合）：

- 每行 `<标记> <绝对路径>`：`-` 缺省删、`?` 回答 `y` 才删（模型 / 用户数据）
- 目录以 `/` 结尾、通配 `*`；路径按固定落点静态给出

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

- 手工 `rm -rf /usr/share/fcitx5/hux` 会连**用户自取**的 n-gram 模型一起删掉； \
  `./uninstall.sh` 缺省保留模型与用户数据
- 手工卸载又要保留模型时，只删 `data/MANIFEST` 的文件（即 `tiger_sentence.*` 与 `symbols.yaml`）：

```sh
while IFS= read -r entry; do
    case "$entry" in ''|'#'*) continue ;; esac
    sudo rm -f "/usr/share/fcitx5/hux/$(basename "$entry")"
done < data/MANIFEST
```
