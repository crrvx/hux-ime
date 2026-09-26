#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
# SPDX-License-Identifier: GPL-3.0-or-later
#
# run.sh —— AT-SPI 取字来源的端到端夹具（一键跑）
#
#   bash tools/atspi/run.sh            全绿则 exit 0；任一断言失败 exit 1；
#                                      环境缺依赖 / 编译失败 exit 2。
#   bash tools/atspi/run.sh --list     只列场景清单。
#   HUX_ATSPI_KEEP_RUN=1 bash tools/atspi/run.sh
#                                      保留 run 目录里的日志与 probe 二进制（默认清理）。
#
# 它在一个**私有** session bus + 私有无障碍总线里跑（不碰真实桌面会话）：
# 起 mock 可访问应用 → 用真 libatspi 编译的 probe 驱动 `hux::AtspiSource` → 逐项断言。
#
# 两个只属于夹具的环境条件（见 README「为什么这样起总线」）：
#   ① `XDG_RUNTIME_DIR` 必须在 `dbus-run-session` **之前**设好；
#   ② `ATSPI_DBUS_IMPLEMENTATION=dbus-daemon`（默认的 dbus-broker 把服务激活委派给
#      user systemd，私有会话里没有，registry 起不来）。
set -u

HERE=$(cd "$(dirname "$0")" && pwd)
ROOT=$(cd "$HERE/../.." && pwd)
# 全部产物都在 build/ 下（已被 .gitignore 忽略）：日志 + 私有的 XDG_RUNTIME_DIR。
# 别把 run 目录放进 tools/atspi/ —— 里面是套接字与 dbus-daemon 状态，不该被提交。
LOG_DIR="$ROOT/build/atspi-fixture"
RUN_DIR_SHELL="$LOG_DIR/run"

# 等待上限（秒）：都是轮询上限，不是固定 sleep。见 README「稳定性边界」。
MOCK_EMBED_TIMEOUT=25   # 无障碍总线 launcher + registry + mock 注册
MOCK_READY_TIMEOUT=25   # registry 看到 mock 应用
BUS_TIMEOUT=30          # 取无障碍总线地址（含服务激活）
COMPILE_TIMEOUT=240     # 编译 probe（冷启动 + libatspi 头）
PROBE_TIMEOUT=25        # 单次场景（probe 内部自带更小的等待上限）

SCENARIOS="basic caret_zero ascii emoji no_text long_text refresh no_focused no_source"
if [ "${1:-}" = "--list" ]; then
    printf '%s\n' $SCENARIOS
    exit 0
fi

# Unix 套接字路径有长度上限（内核 108 字节，含结尾 NUL），无障碍总线套接字就建在
# XDG_RUNTIME_DIR 下（`<XDG_RUNTIME_DIR>/at-spi/bus_0`）。仓库路径太深时**提前**说清楚，
# 别等 GetAddress 返回空串再去猜。
A11Y_SOCKET_EXPECTED=$(( ${#RUN_DIR_SHELL} + 16 ))   # + "/at-spi/bus_0"
if [ "$A11Y_SOCKET_EXPECTED" -gt 106 ]; then
    printf '工作区路径太深：无障碍总线套接字路径约 %d 字节（内核上限 108）。\n' \
        "$A11Y_SOCKET_EXPECTED" >&2
    printf '把仓库放到更浅的路径下再跑，例如 /home/<user>/hux-ime。\n' >&2
    exit 2
fi

# ---------------------------------------------------------------------------
# 外层：先清掉上一轮的私有时钟目录、设好 XDG_RUNTIME_DIR，再进 dbus-run-session。
#
# 坑（原型实测）：D-Bus 服务激活用的是 **dbus-daemon 自己的 activation environment**，
# 不是请求方进程的 env。只在 dbus-run-session 内部 export XDG_RUNTIME_DIR 的话，被激活的
# at-spi-bus-launcher 仍拿到真实会话的 /run/user/1000，于是去绑已被占用的
# /run/user/1000/at-spi/bus_0，报 `Failed to bind listening socket: Address already in use`，
# `GetAddress` 返回空串。
# ---------------------------------------------------------------------------
if [ "${HUX_ATSPI_INNER:-}" != "1" ]; then
    rm -rf "$RUN_DIR_SHELL"
    mkdir -p "$RUN_DIR_SHELL"
    chmod 700 "$RUN_DIR_SHELL"
    exec env \
        XDG_RUNTIME_DIR="$RUN_DIR_SHELL" \
        GSETTINGS_BACKEND=memory \
        ATSPI_DBUS_IMPLEMENTATION=dbus-daemon \
        HUX_ATSPI_INNER=1 \
        dbus-run-session -- bash "$0" "$@"
fi

# ---------------------------------------------------------------------------
# 内层：已在私有 session bus 里
# ---------------------------------------------------------------------------
mkdir -p "$LOG_DIR"
rm -f "$LOG_DIR"/*.log "$LOG_DIR"/control.txt "$LOG_DIR"/embed.conf 2>/dev/null

FAILED=0
MOCK_PID=""
MOCK_LOG=""

step() { printf '\n\033[1m=== %s\033[0m\n' "$*"; }
note() { printf '  %s\n' "$*"; }
ok()   { printf '  \033[32m[OK]\033[0m %s\n' "$*"; }
bad()  { printf '  \033[31m[FAIL]\033[0m %s\n' "$*"; FAILED=1; }
skip() { printf '  \033[33m[SKIP]\033[0m %s\n' "$*"; }

kill_mock() {
    if [ -n "$MOCK_PID" ]; then
        kill "$MOCK_PID" 2>/dev/null
        wait "$MOCK_PID" 2>/dev/null
        MOCK_PID=""
    fi
}
cleanup() {
    kill_mock
    if [ "${HUX_ATSPI_KEEP_RUN:-0}" != "1" ]; then
        rm -f "$RUN_DIR_SHELL/probe" "$RUN_DIR_SHELL/build.log" 2>/dev/null
    fi
}
trap cleanup EXIT

# 起一个 mock；等它打印 `MOCK_EMBEDDED`（轮询 + 上限），成功返回 0。
start_mock() {
    local name="$1"
    shift
    MOCK_LOG="$LOG_DIR/$name.mock.log"
    env "$@" python3 "$HERE/mock_app.py" >"$MOCK_LOG" 2>&1 &
    MOCK_PID=$!
    local waited=0
    while [ "$waited" -lt "$MOCK_EMBED_TIMEOUT" ]; do
        if grep -q 'MOCK_EMBEDDED' "$MOCK_LOG" 2>/dev/null; then
            return 0
        fi
        if grep -q 'MOCK_EMBED_FAILED' "$MOCK_LOG" 2>/dev/null; then
            return 1
        fi
        # mock 自己崩了就别等满上限
        if ! kill -0 "$MOCK_PID" 2>/dev/null; then
            return 1
        fi
        sleep 0.25
        waited=$((waited + 1))
    done
    return 1
}

# 等 registry 的根对象下出现 mock 应用（轮询 + 上限）。
wait_for_app() {
    local waited=0
    while [ "$waited" -lt "$MOCK_READY_TIMEOUT" ]; do
        local children
        children=$(gdbus call --address "$A11Y" --dest org.a11y.atspi.Registry \
            --object-path /org/a11y/atspi/accessible/root \
            --method org.a11y.atspi.Accessible.GetChildren 2>/dev/null)
        if printf '%s' "$children" | grep -q "$(mock_name)"; then
            note "registry 根对象下的应用：$children"
            return 0
        fi
        sleep 0.25
        waited=$((waited + 1))
    done
    return 1
}

mock_name() {
    awk '/^MOCK_EMBEDDED/ {print $2}' "$MOCK_LOG"
}

# 跑一次 probe：`run_probe <场景名> <参数...>`；返回 probe 的退出码。
run_probe() {
    local name="$1"
    shift
    local probe_log="$LOG_DIR/$name.probe.log"
    timeout "$PROBE_TIMEOUT" env AT_SPI_BUS_ADDRESS="$A11Y" \
        "$RUN_DIR_SHELL/probe" "$@" >"$probe_log" 2>&1
    local rc=$?
    sed 's/^/    /' "$probe_log"
    if [ "$rc" -eq 124 ]; then
        bad "probe 超时（${PROBE_TIMEOUT}s 未退出）"
        return 1
    fi
    if [ "$rc" -ne 0 ]; then
        bad "probe 退出码 $rc（参数：$*）"
        return 1
    fi
    return 0
}

# 通用：起 mock → 等应用可见 → 跑 probe → 收摊。
scenario() {
    local name="$1"
    local expect_app="$2"
    shift 2
    step "[$name]"
    if ! start_mock "$name" "$@"; then
        bad "mock 未能注册进无障碍总线（日志：$MOCK_LOG）"
        tail -5 "$MOCK_LOG" 2>/dev/null | sed 's/^/      /'
        kill_mock
        return 1
    fi
    note "mock 已注册：$(grep '^MOCK_EMBEDDED' "$MOCK_LOG")"
    if [ "$expect_app" = "1" ]; then
        if ! wait_for_app; then
            bad "registry 里看不到 mock 应用（上限 ${MOCK_READY_TIMEOUT}s）"
            kill_mock
            return 1
        fi
    fi
    return 0
}

# ---------------------------------------------------------------------------
step "0. 环境"
note "session bus     = ${DBUS_SESSION_BUS_ADDRESS:-(unset)}"
note "XDG_RUNTIME_DIR = ${XDG_RUNTIME_DIR:-(unset)}"
note "ATSPI_DBUS_IMPL = ${ATSPI_DBUS_IMPLEMENTATION:-(unset)}"
note "日志目录        = $LOG_DIR"
MISSING=""
for tool in g++ pkg-config gdbus dbus-run-session python3 timeout; do
    command -v "$tool" >/dev/null 2>&1 || MISSING="$MISSING $tool"
done
[ -n "$MISSING" ] && { bad "缺少命令：$MISSING"; exit 2; }
if ! pkg-config --exists atspi-2; then
    bad "缺少 atspi-2 开发包（装 libatspi2.0-dev）"
    exit 2
fi
if ! python3 -c 'import dbus, gi' >/dev/null 2>&1; then
    bad "缺少 python3-dbus / python3-gi"
    exit 2
fi
ok "依赖齐备：atspi-2 $(pkg-config --modversion atspi-2) / $(g++ --version | head -1)"

# ---- 1. 私有无障碍总线 -----------------------------------------------------
step "1. 私有无障碍总线（org.a11y.Bus.GetAddress → 激活 at-spi-bus-launcher）"
# 把这两个变量显式写进 dbus-daemon 的**激活环境**：被激活的 at-spi-bus-launcher 要用它决定
# 无障碍总线套接字落在哪个 XDG_RUNTIME_DIR 下。外层已经在 dbus-run-session 之前设过，
# 这里再显式声明一次，免得依赖「daemon 恰好继承了父进程环境」这一实现细节。
if command -v dbus-update-activation-environment >/dev/null 2>&1; then
    dbus-update-activation-environment XDG_RUNTIME_DIR ATSPI_DBUS_IMPLEMENTATION \
        GSETTINGS_BACKEND 2>/dev/null ||
        note "dbus-update-activation-environment 失败（继续用 daemon 继承来的环境）"
else
    note "无 dbus-update-activation-environment（用 daemon 继承来的环境）"
fi
A11Y=""
waited=0
while [ "$waited" -lt "$BUS_TIMEOUT" ]; do
    RAW=$(gdbus call --session --dest org.a11y.Bus --object-path /org/a11y/bus \
        --method org.a11y.Bus.GetAddress 2>/dev/null)
    A11Y=$(python3 -c "
import re,sys
m = re.search(r\"'(unix:[^']+)'\", sys.argv[1])
print(m.group(1) if m else '')
" "$RAW")
    [ -n "$A11Y" ] && break
    sleep 0.5
    waited=$((waited + 1))
done
if [ -n "$A11Y" ]; then
    ok "无障碍总线 = $A11Y"
else
    bad "拿不到无障碍总线地址（上限 ${BUS_TIMEOUT}s）：$RAW"
    exit 1
fi

# ---- 2. 编译 probe --------------------------------------------------------
step "2. 编译 probe.cpp + platform/fcitx5/shell/atspi_source.cpp"
if timeout "$COMPILE_TIMEOUT" g++ -O0 -g -std=c++17 -Wall -Wextra \
        -Wno-unused-parameter -DHUX_HAVE_ATSPI=1 \
        -I"$ROOT/platform/fcitx5/shell" \
        "$HERE/probe.cpp" "$ROOT/platform/fcitx5/shell/atspi_source.cpp" \
        -o "$RUN_DIR_SHELL/probe" \
        $(pkg-config --cflags --libs atspi-2 gobject-2.0) \
        -lpthread >"$LOG_DIR/build.log" 2>&1; then
    ok "编译成功：$(pkg-config --modversion atspi-2)"
    if [ -s "$LOG_DIR/build.log" ]; then
        note "编译告警："; sed 's/^/      /' "$LOG_DIR/build.log"
    fi
else
    bad "编译失败"; sed 's/^/      /' "$LOG_DIR/build.log"; exit 2
fi

# ---- 3. 断言场景 ----------------------------------------------------------
# 每个场景：起 mock（可带控制文件）→ 等 registry 看到它 → probe 断言 → 换下一个。

# 断言 1：焦点文本「中欧中兴」+ caret=2 ⇒ 左侧窗口「中欧」、光标 2 个**字符**（不是 6 字节）。
if scenario basic 1 MOCK_TEXT=中欧中兴 MOCK_CARET=2; then
    run_probe basic wait 中欧 2 --timeout=8000
fi
kill_mock

# 断言 2：caret=0 ⇒ 空窗口（实现按「无来源」退化），不崩。
if scenario caret_zero 1 MOCK_TEXT=中欧中兴 MOCK_CARET=0; then
    run_probe caret_zero null --expect=no-source --timeout=5000
fi
kill_mock

# 断言 3：纯 ASCII：caret 即字符数即字节数（对照组）。
if scenario ascii 1 MOCK_TEXT=hello MOCK_CARET=3; then
    run_probe ascii wait hel 3 --timeout=8000
fi
kill_mock

# 断言 4：多字节 + emoji：caret=5 时窗口「中欧ab🐯」，字符 5 / 字节 12。
if scenario emoji 1 'MOCK_TEXT=中欧ab🐯cd' MOCK_CARET=5; then
    run_probe emoji wait 中欧ab🐯 5 --timeout=8000
fi
kill_mock

# 断言 5：焦点对象**没有 Text 接口** ⇒ nullopt。
if scenario no_text 1 MOCK_ENTRY=button; then
    run_probe no_text null --expect=no-text --timeout=8000
fi
kill_mock

# 断言 6：窗口上限 64 字符：200 字符文本 + caret 在末尾 ⇒ 只取左侧 64 个字符。
LONG_TEXT=$(printf 'a%.0s' $(seq 1 200))
LONG_HEAD=${LONG_TEXT:0:136}
LONG_TAIL=${LONG_TEXT:136:64}
if scenario long_text 1 "MOCK_TEXT=$LONG_TEXT" "MOCK_CARET=200"; then
    run_probe long_text wait "$LONG_TAIL" 64 --timeout=8000
    EXPECTED_BYTES=$(printf '%s' "$LONG_HEAD" | wc -c)
    note "整串 200 字符 / 期望窗口 64 字符（左侧 136 字符 = ${EXPECTED_BYTES} 字节未取）"
fi
kill_mock

# 断言 7：时效：改文本后 requestRefresh() ⇒ 快照在数百毫秒内更新（轮询等待，不赌 sleep）。
# 期望值是**新光标左侧的窗口**（caret=6 ⇒ 「更新后的文本」；取的是窗口，不是整串新文本）。
CONTROL="$LOG_DIR/control.txt"
printf '%s\t%s\n' '中欧中兴' '2' >"$CONTROL"
if scenario refresh 1 "MOCK_CONTROL_FILE=$CONTROL" MOCK_LIFETIME=40; then
    run_probe refresh refresh "$CONTROL" '更新后的文本内容在这里' --caret=6
fi
kill_mock

# 断言 8：无障碍总线可用、但**总线上没有任何可访问应用** ⇒ nullopt，不崩。
# 这里用 `env -i` 隔离会话总线环境：libatspi 只能走注册表默认地址，
# 而本夹具里没有任何应用注册进去（标准做法是往总线发 events，本夹具刻意不发）。
step "[no_focused] 无障碍总线可用但没有可访问应用"
if timeout 30 env -i AT_SPI_BUS_ADDRESS="$A11Y" PATH="$PATH" HOME="$HOME" \
        "$RUN_DIR_SHELL/probe" null --expect=no-source --timeout=8000 \
        >"$LOG_DIR/no_focused.probe.log" 2>&1; then
    sed 's/^/    /' "$LOG_DIR/no_focused.probe.log"
    ok "总线上没有可访问应用时快照为空（nullopt），未崩"
else
    rc=$?
    sed 's/^/    /' "$LOG_DIR/no_focused.probe.log"
    bad "无应用场景失败（退出码 $rc）"
fi

# 断言 9：无障碍总线**不可用** ⇒ nullopt，且 snapshot() 与 requestRefresh() 都立即返回。
# 两个变体：① 总线地址指向不存在的套接字；② 连会话总线环境都没有（env -i）。
step "[no_source] 无障碍总线不可用 ⇒ 空快照 + 不阻塞"
A11Y_SAVE="$A11Y"
A11Y="unix:path=$RUN_DIR_SHELL/does-not-exist"
run_probe no_source fast --timeout=50
A11Y="$A11Y_SAVE"
if timeout 30 env -i \
        AT_SPI_BUS_ADDRESS="unix:path=$RUN_DIR_SHELL/does-not-exist" \
        PATH="$PATH" HOME="$HOME" \
        "$RUN_DIR_SHELL/probe" fast --timeout=50 \
        >>"$LOG_DIR/no_source.probe.log" 2>&1; then
    sed 's/^/    /' "$LOG_DIR/no_source.probe.log" | tail -8
    ok "无总线时 snapshot()/requestRefresh() 立即返回（< 50ms）且为空"
else
    rc=$?
    sed 's/^/    /' "$LOG_DIR/no_source.probe.log" | tail -12
    case "$rc" in
    2)  skip "该构建未编入 AT-SPI（compiled()=0），stub 退化由第 4 步覆盖" ;;
    *)  bad "无总线场景失败（退出码 $rc）" ;;
    esac
fi
# ---- 4. 无 AT-SPI 构建的退化（stub 路径，不连总线） -------------------------
# 不定义 HUX_HAVE_ATSPI ⇒ compiled() 必须为假，探针按约定以 2 退出（且必须在
# 立刻退出前不阻塞 —— 这就等价于「没有开发包也要能构建、能退化」的验证）。
step "4. stub 构建（HUX_ATSPI=OFF ⇔ 不定义 HUX_HAVE_ATSPI）"
STUB_LOG="$LOG_DIR/stub.log"
if timeout "$COMPILE_TIMEOUT" g++ -O0 -g -std=c++17 -Wall -Wextra \
        -Wno-unused-parameter -I"$ROOT/platform/fcitx5/shell" \
        "$HERE/probe.cpp" "$ROOT/platform/fcitx5/shell/atspi_source.cpp" \
        -o "$RUN_DIR_SHELL/probe-stub" $(pkg-config --cflags --libs gobject-2.0) \
        -lpthread >>"$LOG_DIR/build.log" 2>&1; then
    ok "stub 构建编译成功（未定义 HUX_HAVE_ATSPI，只链 gobject-2.0）"
    rc=0
    timeout 30 env AT_SPI_BUS_ADDRESS="unix:path=$RUN_DIR_SHELL/does-not-exist" \
        "$RUN_DIR_SHELL/probe-stub" fast --timeout=50 >"$STUB_LOG" 2>&1 || rc=$?
    sed 's/^/      /' "$STUB_LOG"
    case "$rc" in
    2)  ok "stub 构建：compiled()=0，探针立刻退出（无任何总线访问、不阻塞）" ;;
    0)  bad "stub 构建里 compiled() 竟为真（HUX_HAVE_ATSPI 泄漏进编译）" ;;
    *)  bad "stub 构建行为异常（退出码 $rc）" ;;
    esac
    [ "${HUX_ATSPI_KEEP_RUN:-0}" = "1" ] || rm -f "$RUN_DIR_SHELL/probe-stub"
else
    bad "stub 构建编译失败"; tail -20 "$LOG_DIR/build.log" | sed 's/^/      /'
fi

# ---- 结果 -----------------------------------------------------------------
step "结果"
if [ "$FAILED" -eq 0 ]; then
    printf '\033[32m全部断言通过\033[0m（日志：%s）\n' "$LOG_DIR"
    exit 0
fi
printf '\033[31m有失败项\033[0m（日志：%s）\n' "$LOG_DIR"
exit 1
