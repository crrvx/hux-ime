<!-- SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com> -->
<!-- SPDX-License-Identifier: GPL-3.0-or-later -->

# AT-SPI 取字来源的端到端夹具

这里放的是**「取字来源」在无障碍总线上的真机验证**：一个假的「可访问应用」+ 一个真的
libatspi 探针 + 一键脚本。它验证的不是内部函数，而是**整条链路** —— D-Bus 上对象的形状、
`atspi_*` 的取数语义、字符 / 字节的换算、失败时的退化行为。

```
bash tools/atspi/run.sh          # 全绿 exit 0 / 有断言失败 exit 1 / 环境或编译失败 exit 2
bash tools/atspi/run.sh --list   # 只列场景
HUX_ATSPI_KEEP_RUN=1 bash tools/atspi/run.sh   # 保留临时 run 目录（默认清理）
```

依赖：`libatspi2.0-dev`、`at-spi2-core`、`python3-dbus`、`python3-gi`、`dbus`（含
`dbus-run-session`）、`g++`、`pkg-config`。全部产物都在 `build/atspi-fixture/`（已被
`.gitignore` 忽略）：每个场景一份 mock 日志 + probe 日志，外加作为 `XDG_RUNTIME_DIR` 的
`run/` 子目录。失败时先看那里。

## 三个文件

- `mock_app.py`：假的「可访问应用」。一个带 `Text` 接口的文本框 + 一个没有 `Text` 接口的
  按钮，通过 `org.a11y.atspi.Socket.Embed` 注册进注册表；文本框内容与光标可由环境变量给定，
  也可由控制文件在运行期改写。
- `probe.cpp`：真探针。链真 libatspi，直接驱动 `hux::AtspiSource`，把每个断言跑成进程退出码。
- `run.sh`：一键。起私有 session bus + 私有无障碍总线 → 选一个 mock 形态 → 编译探针 →
  跑场景 → 汇总。

`probe.cpp` 只用**实现方的公开接口**（`snapshot()` / `requestRefresh()` /
`compiled()`），不含任何内部头文件 —— 夹具因此不会被实现的内部重构带走。

## 断言清单

- `basic`：文本「中欧中兴」，光标在第 2 字后 ⇒ `text` = 光标左侧窗口「中欧」，
  `cursorChars` = **2**（字符，而不是 6 字节）。
- `caret_zero`：光标在最前 ⇒ 空（`nullopt`），不崩。
- `ascii`：`hello`，光标 3 ⇒ `hel` / `3`（字符 = 字节的对照组）。
- `emoji`：`中欧ab🐯cd`，光标 5 ⇒ `中欧ab🐯` / **5** 字符（该窗口 12 字节 —— 两组数字必须
  能区分）。
- `no_text`：焦点对象是按钮（无 `Text` 接口）⇒ `nullopt`。
- `long_text`：200 个字符，光标在末尾 ⇒ 窗口 = **64** 字符上限（左侧 136 字符不取）。
- `refresh`：改控制文件后 `requestRefresh()` ⇒ 快照在数百毫秒内变成新窗口（实测约 205 ms）。
- `no_focused`：总线可用但没有任何应用 ⇒ `nullopt`，不崩。
- `no_source`：总线不可用（无效地址 / 无会话总线环境）⇒ `nullopt`，且 `snapshot()` 与
  `requestRefresh()` 单次都 **< 50 ms**。
- stub 构建：不定义 `HUX_HAVE_ATSPI` 编译 ⇒ `compiled()` 为假，探针立刻以 2 退出（不碰总线）。

## 为什么这样起总线

私有会话里让无障碍总线起来有两个**只属于夹具**的条件（生产代码不依赖它们）：

1. `XDG_RUNTIME_DIR` 要在 `dbus-run-session` **之前**设好，并在会话里再
   `dbus-update-activation-environment` 声明一次。否则被激活的 `at-spi-bus-launcher` 会去
   绑真实桌面会话已经占用的 `/run/user/<uid>/at-spi/bus_0`，报
   `Failed to bind listening socket: Address already in use`，`GetAddress` 返回空串。
2. `ATSPI_DBUS_IMPLEMENTATION=dbus-daemon`。默认的 dbus-broker 把服务激活委派给 user
   systemd，而私有会话里没有那个 systemd，注册表会以
   `Could not activate remote peer 'org.a11y.atspi.Registry': unit failed` 失败。

夹具自己连的地址**只**取自 `org.a11y.Bus.GetAddress`，不拼路径、不认 `/run/user/...`。
无障碍总线套接字建在 `XDG_RUNTIME_DIR` 下，而 Unix 套接字路径有 108 字节的内核上限，所以
脚本在开工前会按预期路径长度**提前**报错（工作区路径太深时），而不是让你对着一个空的
`GetAddress` 猜。

## 为什么 mock 长这样

`mock_app.py` 的形状不是随便写的，每一条都对应一个真实坑：

- **`Text` 的 `CharacterCount` / `CaretOffset` 必须是 D-Bus 属性**，不是同名方法。
  `atspi_text_get_character_count()` / `_get_caret_offset()` 读的是
  `org.freedesktop.DBus.Properties`，只实现同名方法会拿到 `0` / `-1` 且 `GError == NULL`
  —— 一个**不报错的错值**，最难查。`SelectionCount` 同理。
- **偏移是字符、文本是字节**：mock 的 `GetText` 按字符区间切片，回 UTF-8 字节串。
- **`Socket.Embed` 挂在 `/org/a11y/atspi/accessible/root`**，不是
  `/org/a11y/atspi/registry`（2.60 的布局）。
- **`Embed` 必须在主循环起来之后做**（`GLib.timeout_add` 的第一拍），否则注册表的回调会
  死在同一个进程上。
- **不用注册表缓存发现对象**：`org.a11y.atspi.Cache.GetItems` 在真注册表上实测回空表
  （children 明明非空）。探针走的是 `Accessible.GetChildren` / 逐层遍历这条真路径。
- **mock 不发事件**。实现方用的是加锁缓存的同步轮询（理由写在实现文件的头注释里：私有
  `GMainContext` 上收不到 focus / caret 事件）。夹具因此只断言轮询路径；发一堆事件反而会
  给「来源刚被事件作废、下一拍才补上」造出无关的 `nullopt` 窗口。
- **时效断言不靠 `sleep`**：`MOCK_CONTROL_FILE` 是一个文本文件，mock 在**每次 D-Bus 调用
  前**重读它（写入方用临时文件 + rename 原子替换）。所以「改了文本」这件事是**确定的**，
  剩下的只是轮询等结果。
- **控制文件只在「内容变了」时套用**：否则每次调用前的重读会把进程内的写入（比如
  `SetCaretOffset`）按文件里的旧值抹回去 —— 表现为「赋值不生效」。caret 因此有两个真源：
  文件（一变就赢）与 `SetCaretOffset`（文件没变时保持）；夹具只用前者驱动。

## 稳定性边界

所有等待都是**轮询 + 明确上限**，没有一处靠固定 `sleep` 赌时序：

- 无障碍总线地址：上限 30 s（含注册表服务激活）。
- mock 打印 `MOCK_EMBEDDED`：上限 25 s（同时检测 mock 是否已死，不等满）。
- 注册表里出现 mock 应用：上限 25 s（`GetChildren` 轮询）。
- 编译探针：上限 240 s（冷启动 + 头文件）。
- 单次场景：上限 25 s（外层 `timeout`；探针内部自带更小的上限）。
- 来源出现第一个快照：探针参数，默认 5 s（实测每个场景约 212 ms）。
- 刷新传播：探针参数，默认 2 s（实测约 205 ms）。
- 无来源时的单次调用：50 ms（实现是纯缓存读，实测 < 0.05 ms）。

阈值都留了**两个数量级**余量（212 ms 实测 vs 5 s 上限），所以「偶然失败」不是靠重试掩盖
的 —— 要么过，要么是回归。探针里的 `requestRefresh()` 每 10 ms 一次：它同时是实现的
「惰性起线程」触发点，只读 `snapshot()` 会永远看到空缓存。

## 为什么不放在 `tools/probes/`

`tools/probes/*.cpp` 会被既有的 `addon` CI 作业用 `g++ -fsyntax-only` 过一遍，而那个作业
**故意不装 AT-SPI 开发包**（它要证明「没有开发包也能构建」）。夹具在这里自成一个作业。
