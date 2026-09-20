<!-- SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com> -->
<!-- SPDX-License-Identifier: GPL-3.0-or-later -->

# 重构：核心引擎化 / 平台无关 / 码表无关 / 测试正式化

目标：把现状（虎句 + fcitx5 桌面）整理为可承载**多方案、多平台**的引擎结构。 \
**本轮范围**：双端（linux / android）+ 虎码（字 / 词 / 句）；其他方案与平台仅留 README 骨架。

## 1. 结构正义（硬规则）

1. **依赖单向**：`platform/* → hux-ffi → hux-core → hux-cfg`；`hux-scheme/* → hux-core`。
   内核不依赖任何方案、不依赖任何平台。
2. **core 零平台**：不得出现 `std::env`、XDG / 绝对数据路径、`SystemTime::now`、`eprintln!`、
   平台文件 API；路径 / 时钟 / 日志由平台构造并注入（`Paths` / `Clock` / `Log`）。
3. **职责归位**：只读数据（码表 / 模型 / 索引）属方案；可写数据（选项 / 学习库）属 `hux-cfg` 与平台存储实现；
   UI 快照、C++ 壳、打包属 `platform`；C ABI 是平台边界（`hux-ffi`）。
4. 模块名即职责；单元测试随模块、集成 / 差分测试独立目录（二者分离）。

## 2. 目标结构

```
crates/                       # 平台无关的 Rust 库
  hux-core/                   # 引擎内核：key/session/composition/处理器管线/宿主链/学习机制
                              #   + 方案契约（hux_core::scheme）
  hux-cfg/                    # hux 自身可配置项：设置项定义与默认值、选项存储与合并顺序
                              #   （options.yaml > 设置 > 内建）、状态菜单开关白名单、持久化接口
  hux-ffi/                    # C ABI：C 布局类型 + 导出函数（桌面 / Android 共用）
  hux-scheme/
    tiger/                    # 虎码（字/词/句）——本轮唯一全量实现
    yuhao/  wubi/             # init：README 骨架（形码族，复用 tiger 框架）
    shuangpin/  quanpin/      # init：README 骨架（拼音族，接口预留）
  hux-test-support/           # 测试助手（dev 依赖；克制版，P5 视需要）
platform/                     # 平台适配
  fcitx5/                     # 共享 fcitx5 适配：Rust 组装（Engine/UI 快照/存储实现/Paths）
                              #   + C++ 壳 + CMake（linux 与 android 共用）
  linux/                      # 桌面：构建入口 / 安装脚本 / 打包（PKGBUILD 等）
  android/                    # Android：构建接线（对接 fcitx5-android fork 的 plugin/hux）
  windows/  macos/  ios/      # init：README 骨架
```

平台层分工：`platform/fcitx5` 是**共用适配**（两端都是 fcitx5，环境变量与路径解析同一套）；
`platform/linux`、`platform/android` 只管各自的**构建与分发**。

## 3. 现 → 新 迁移映射

| 现位置 | 去向 |
| --- | --- |
| `hux-core` 通用部分（key / session / punct / 处理器骨架 / host / 学习机制 / cache） | `crates/hux-core` |
| `hux-core` 方案部分（lexicon / decode / lexical / ngram / sound_to_char_shape / char_to_sound_shape / 虎码规则） | `crates/hux-scheme/tiger` |
| `hux-addon/src/settings.rs`、`options.rs` | `crates/hux-cfg` |
| `hux-addon` 的 C ABI 段 | `crates/hux-ffi` |
| `hux-addon` 的 Engine / Session / UI 快照 / 存储实现（learning_store） | `platform/fcitx5`（Rust crate） |
| `crates/hux-addon/shell`（C++ 壳）与 `CMakeLists.txt` | `platform/fcitx5`（C++）+ `platform/linux`（构建） |
| 根目录 `install.sh` / `uninstall.sh` | `platform/linux`（P3 时决定是否移动） |

## 4. 批次

| 批 | 内容 | 验收 |
| --- | --- | --- |
| P0 | 本文档 | 评审通过 |
| P1 | 机械解耦：`hux-addon` 拆模块、`interaction.rs` 拆目录 | 行为 / API 不变；255 用例 + 金样全绿 |
| P2 | 平台注入 `Paths` / `Clock` / `Log`，清 core 的 env / XDG 硬编码 | 同一批验收；桌面与 Android 路径均由平台构造 |
| P3 | 拆 crate 与 `platform/` 目录；init 骨架 README | workspace 编译通过；金样全绿 |
| P4 | `hux_core::scheme` 最小契约 + 虎码 profile | 契约落地；不实现新方案 |
| P5 | 测试正式化：`hux-test-support`（克制版）+ 单元 / 集成分离 + CI 分层 | CI 全绿 |
| P6 | 性能（按需）：新建 `hux-bench`、增量解码缓存等 | 金样不变 + 有对比数据 |

## 5. 方案契约（`hux_core::scheme`）

- **放 `hux-core`**：方案无论如何要依赖 core 的类型（`KeyEvent` / `Context` / `Candidate`…），
  单开 interface crate 只多一跳、无净收益；将来若接口变大或需对外提供「方案作者 SDK」，
  再拆 crate（纯移动 + `pub use` 兜底）。
- **最小契约**：只定义内核必须回调的动作（`id` / 数据资产 / `translate` / 证据 / 学习策略 / 按键策略钩子）；
  **不把虎码特有语义**（缓冲态、锁、早提交启发式）泛化进契约——先留在 `tiger` profile，
  等第二个同族方案落地后再抽象。
- 形码族（虎码 / 宇浩 / 五笔）优先；拼音族（双拼 / 全拼）只留接口。
- **内核不 import 任何 `hux-scheme/*`**（CI 校验）。

## 6. 测试与性能

- 单元测试随模块；集成 / 差分测试独立 `tests/`；金样只读，持续作为行为 oracle。
- `hux-test-support`（P5，视需要）：只放金样 / 夹具路径定位、临时目录、`TestEngine` 包装与少量断言助手；
  若超过约 200 行或开始承载业务逻辑，立即停手、退回各 crate 内 `#[cfg(test)]` 助手。
- `hux-bench` **延后**（P6 按需新建，或各 crate `benches/`）；临时量数据可用 `--release` example。
- 优化只允许「金样不变」的改动，且须有前后对比数据。

## 7. 依赖校验（拟入 CI）

- `crates/hux-core` 源码不得出现 `std::env` / `SystemTime` / `eprintln!` / `/usr/share` 等平台痕迹（脚本检查）；
- `cargo tree` 校验：`hux-core` 不依赖 `hux-scheme/*` 与 `platform/*`；`hux-scheme/*` 不依赖 `platform/*`；
- 后续可选 `cargo-deny`。

## 8. 未做（骨架）

- 方案骨架：`crates/hux-scheme/{yuhao,wubi,shuangpin,quanpin}/README.md`；
- 平台骨架：`platform/{windows,macos,ios}/README.md`。

每个骨架 README 写明：目标、与 tiger / fcitx5 的差异、数据与 API 需求、依赖方向；
**仅 README，不进 workspace**，避免空壳与死代码。
