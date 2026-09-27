#!/usr/bin/env python3
# SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
# SPDX-License-Identifier: GPL-3.0-or-later
"""假的「可访问应用」：在一个私有总线会话里冒充真实应用的 AT-SPI2 树。

用途：端到端验证「客户端不上报周边文本时，从无障碍总线取焦点对象的文本与光标」这条链路。
本脚本只提供**被取字的对象**，取字的一侧是 `probe.cpp`（驱动真正的取字来源）。

暴露的最小树（路径与 libatspi 的发现路径一致）：

    /org/a11y/atspi/accessible/root         Application + Accessible（应用根，role=application）
      └ /org/a11y/atspi/accessible/entry1   Accessible + Text + Component + EditableText
                                            （`MOCK_ENTRY=button` 时换成**没有 Text 接口**的按钮）

环境变量（都可选）：

    MOCK_TEXT           文本框内容，默认「中欧中兴」
    MOCK_CARET          光标偏移，**字符**偏移，默认 2
    MOCK_ENTRY          `entry`（默认）或 `button`（焦点对象不带 Text 接口）
    MOCK_LIFETIME       存活秒数，默认 30
    MOCK_CONTROL_FILE   运行期改写用的控制文件；文件内容为 `新文本<TAB>新光标`（光标可省）。
                        每次 D-Bus 调用前重读，故写入即生效 —— 时效断言靠它，不靠固定 sleep。
    AT_SPI_BUS_ADDRESS  直接指定无障碍总线地址；未设置时走会话总线上的
                        `org.a11y.Bus.GetAddress`（与 libatspi 自己拿到的是同一个地址）。

退出码：`Embed` 成功前失败即非零退出；成功注册的行 `MOCK_EMBEDDED` 打印到 stdout，
脚本据此退出 `--wait-for-embed` 等待（见 `run.sh`）。

坑（原型实测，别再摸一遍）：
  * `org.a11y.atspi.Socket.Embed` 在 2.60 挂在 registry 的**根可访问对象**
    `/org/a11y/atspi/accessible/root` 上，不是 `/org/a11y/atspi/registry`。
  * `atspi_text_get_character_count()` / `_get_caret_offset()` 读的是
    `org.a11y.atspi.Text` 的 **D-Bus 属性** `CharacterCount` / `CaretOffset`，
    不是同名方法；只实现方法会拿到 0 / -1 且 `GError == NULL`（静默错值）。
  * Text 的 offset / caret 是**字符**偏移，`GetText` 返回 **UTF-8 字节**串。
  * 本夹具**不**发事件（取字来源走同步轮询），故不实现 `Event.Object` 信号。
"""

from __future__ import annotations

import os
import sys

import dbus
import dbus.mainloop.glib
import dbus.service
from gi.repository import GLib

# ---- 常量（序号与 atspi-constants.h 对齐） ---------------------------------
ROLE_APPLICATION = 75
ROLE_ENTRY = 79
ROLE_PUSH_BUTTON = 46

STATE_EDITABLE = 7
STATE_ENABLED = 8
STATE_FOCUSABLE = 11
STATE_FOCUSED = 12
STATE_SENSITIVE = 24
STATE_SHOWING = 25
STATE_VISIBLE = 30

IFACE_ACCESSIBLE = "org.a11y.atspi.Accessible"
IFACE_APPLICATION = "org.a11y.atspi.Application"
IFACE_TEXT = "org.a11y.atspi.Text"
IFACE_COMPONENT = "org.a11y.atspi.Component"
IFACE_EDITABLE_TEXT = "org.a11y.atspi.EditableText"
IFACE_CACHE = "org.a11y.atspi.Cache"
IFACE_SOCKET = "org.a11y.atspi.Socket"
PROP_IFACE = "org.freedesktop.DBus.Properties"

REGISTRY_BUS = "org.a11y.atspi.Registry"
REGISTRY_SOCKET_PATH = "/org/a11y/atspi/accessible/root"
CACHE_PATH = "/org/a11y/atspi/cache"
ROOT_PATH = "/org/a11y/atspi/accessible/root"
ENTRY_PATH = "/org/a11y/atspi/accessible/entry1"
NULL_PATH = "/org/a11y/atspi/null"

ROLE_NAMES = {
    ROLE_APPLICATION: "application",
    ROLE_ENTRY: "entry",
    ROLE_PUSH_BUTTON: "push button",
}


def log(*args):
    print(*args, file=sys.stderr, flush=True)


def state_array(states):
    """把状态序号集合编成 AT-SPI 的状态字数组（au，32 位一组）。"""
    words = [0, 0]
    for state in states:
        words[state // 32] |= 1 << (state % 32)
    return dbus.Array(words, signature="u")


class Ref:
    """`(so)` 对象引用（总线名 + 对象路径）。"""

    def __init__(self, conn):
        self.bus_name = conn.get_unique_name()

    def __call__(self, path):
        return dbus.Struct((dbus.String(self.bus_name), dbus.ObjectPath(path)),
                           signature="so")


class ControlFile:
    """控制文件：`新文本<TAB>新光标`；内容一变即最新值（写入用临时文件 + rename）。

    读失败（写入竞态 / 文件被删）时保留上一次的值 —— 夹具不因为一次读失败而炸。
    **只认「内容变了」**：文件没变就不重新套用，否则每次 D-Bus 调用前的重读会把进程内的
    写入（`SetCaretOffset`）按文件里的旧值抹掉 —— 一个「赋值不生效」的假象。
    """

    def __init__(self, path, text, caret):
        self.path = path
        self.text = text
        self.caret = caret
        self._last_raw = None

    def refresh(self):
        if not self.path:
            return
        try:
            with open(self.path, "r", encoding="utf-8") as handle:
                raw = handle.read()
        except OSError:
            return
        if raw == self._last_raw:
            return
        self._last_raw = raw
        line = raw.split("\n", 1)[0].rstrip("\r")
        text, _, caret = line.partition("\t")
        if text:
            self.text = text
        if caret.strip():
            try:
                # 字符偏移，且允许等于文本长度（光标在末尾）。
                self.caret = max(0, min(int(caret), len(self.text)))
            except ValueError:
                pass


class AccessibleObject(dbus.service.Object):
    """所有可访问对象的公共基类（含 `org.freedesktop.DBus.Properties`）。"""

    def __init__(self, bus, path, ref, name, role, states, ifaces, parent_path):
        super().__init__(bus, path)
        self.ref = ref
        self.obj_name = name
        self.role = role
        self.states = set(states)
        self.ifaces = list(ifaces)
        self.parent_path = parent_path

    # -- 子类覆盖 ----------------------------------------------------------
    def child_paths(self):
        return []

    def extra_props(self):
        return {}

    def touch(self):
        """每次 D-Bus 调用前的「同步控制文件」钩子。"""

    # -- org.freedesktop.DBus.Properties -----------------------------------
    def _props(self):
        return {
            "version": dbus.UInt32(2),
            "Name": dbus.String(self.obj_name),
            "Description": dbus.String(""),
            "Parent": self.ref(self.parent_path),
            "ChildCount": dbus.Int32(len(self.child_paths())),
            "Locale": dbus.String("zh_CN.UTF-8"),
            "AccessibleId": dbus.String(""),
            "HelpText": dbus.String(""),
        }

    @dbus.service.method(PROP_IFACE, in_signature="ss", out_signature="v")
    def Get(self, iface, prop):
        self.touch()
        props = dict(self._props())
        props.update(self.extra_props())
        if prop not in props:
            raise dbus.exceptions.DBusException(
                "Unknown property %s" % prop,
                name="org.freedesktop.DBus.Error.UnknownProperty")
        return props[prop]

    @dbus.service.method(PROP_IFACE, in_signature="s", out_signature="a{sv}")
    def GetAll(self, iface):
        self.touch()
        props = dict(self._props())
        props.update(self.extra_props())
        return dbus.Dictionary(props, signature="sv")

    @dbus.service.method(PROP_IFACE, in_signature="ssv")
    def Set(self, iface, prop, value):
        return

    # -- org.a11y.atspi.Accessible -----------------------------------------
    @dbus.service.method(IFACE_ACCESSIBLE, in_signature="i", out_signature="(so)")
    def GetChildAtIndex(self, index):
        paths = self.child_paths()
        if index < 0 or index >= len(paths):
            return self.ref(NULL_PATH)
        return self.ref(paths[index])

    @dbus.service.method(IFACE_ACCESSIBLE, out_signature="a(so)")
    def GetChildren(self):
        return dbus.Array([self.ref(p) for p in self.child_paths()], signature="(so)")

    @dbus.service.method(IFACE_ACCESSIBLE, out_signature="i")
    def GetIndexInParent(self):
        paths = self.child_paths() if self.parent_path == ROOT_PATH else []
        for index, path in enumerate(paths):
            if path == self._object_path:
                return dbus.Int32(index)
        return dbus.Int32(-1)

    @dbus.service.method(IFACE_ACCESSIBLE, out_signature="a(ua(so))")
    def GetRelationSet(self):
        return dbus.Array([], signature="(ua(so))")

    @dbus.service.method(IFACE_ACCESSIBLE, out_signature="u")
    def GetRole(self):
        return dbus.UInt32(self.role)

    @dbus.service.method(IFACE_ACCESSIBLE, out_signature="s")
    def GetRoleName(self):
        return dbus.String(ROLE_NAMES.get(self.role, "unknown"))

    @dbus.service.method(IFACE_ACCESSIBLE, out_signature="s")
    def GetLocalizedRoleName(self):
        return self.GetRoleName()

    @dbus.service.method(IFACE_ACCESSIBLE, out_signature="au")
    def GetState(self):
        return state_array(self.states)

    @dbus.service.method(IFACE_ACCESSIBLE, out_signature="a{ss}")
    def GetAttributes(self):
        return dbus.Dictionary({}, signature="ss")

    @dbus.service.method(IFACE_ACCESSIBLE, out_signature="(so)")
    def GetApplication(self):
        return self.ref(ROOT_PATH)

    @dbus.service.method(IFACE_ACCESSIBLE, out_signature="as")
    def GetInterfaces(self):
        return dbus.Array(self.ifaces, signature="s")


class TextEntry(AccessibleObject):
    """文本框：`Accessible` + `Text` + `Component` + `EditableText`。"""

    def __init__(self, bus, ref, control, focused):
        states = {STATE_EDITABLE, STATE_ENABLED, STATE_FOCUSABLE, STATE_SENSITIVE,
                  STATE_SHOWING, STATE_VISIBLE}
        if focused:
            states.add(STATE_FOCUSED)
        super().__init__(bus, ENTRY_PATH, ref, "mock entry", ROLE_ENTRY, states,
                         [IFACE_ACCESSIBLE, IFACE_TEXT, IFACE_COMPONENT,
                          IFACE_EDITABLE_TEXT], ROOT_PATH)
        self.control = control

    def touch(self):
        self.control.refresh()

    @property
    def text(self):
        return self.control.text

    @property
    def caret(self):
        return self.control.caret

    def extra_props(self):
        # 关键：libatspi 的 atspi_text_get_character_count / _get_caret_offset 读的是
        # **属性** org.a11y.atspi.Text.CharacterCount / .CaretOffset，不是同名方法。
        return {
            "CharacterCount": dbus.Int32(len(self.text)),
            "CaretOffset": dbus.Int32(self.caret),
            "SelectionCount": dbus.Int32(0),
        }

    # -- org.a11y.atspi.Text -----------------------------------------------
    def _slice(self, start, end):
        """按**字符**区间切片；`end == -1` 表示到末尾，越界夹取。"""
        count = len(self.text)
        if end == -1 or end > count:
            end = count
        start = max(0, min(start, count))
        return self.text[start:max(start, end)]

    @dbus.service.method(IFACE_TEXT, out_signature="i")
    def GetCharacterCount(self):
        self.touch()
        return dbus.Int32(len(self.text))

    @dbus.service.method(IFACE_TEXT, in_signature="ii", out_signature="s")
    def GetText(self, start_offset, end_offset):
        self.touch()
        return dbus.String(self._slice(int(start_offset), int(end_offset)))

    @dbus.service.method(IFACE_TEXT, in_signature="iu", out_signature="(suii)")
    def GetStringAtOffset(self, offset, granularity):
        self.touch()
        index = int(offset)
        char = self.text[index:index + 1]
        return dbus.Struct((dbus.String(char), dbus.UInt32(int(granularity)),
                            dbus.Int32(index), dbus.Int32(index + 1)),
                           signature="suii")

    @dbus.service.method(IFACE_TEXT, in_signature="iu", out_signature="(sii)")
    def GetTextAtOffset(self, offset, boundary_type):
        self.touch()
        index = int(offset)
        return dbus.Struct((dbus.String(self._slice(index, index + 1)),
                            dbus.Int32(index), dbus.Int32(index + 1)), signature="sii")

    @dbus.service.method(IFACE_TEXT, in_signature="iu", out_signature="(sii)")
    def GetTextBeforeOffset(self, offset, boundary_type):
        self.touch()
        index = int(offset)
        return dbus.Struct((dbus.String(self._slice(max(0, index - 1), index)),
                            dbus.Int32(max(0, index - 1)), dbus.Int32(index)),
                           signature="sii")

    @dbus.service.method(IFACE_TEXT, in_signature="iu", out_signature="(sii)")
    def GetTextAfterOffset(self, offset, boundary_type):
        self.touch()
        index = int(offset)
        return dbus.Struct((dbus.String(self._slice(index, index + 1)),
                            dbus.Int32(index), dbus.Int32(index + 1)), signature="sii")

    @dbus.service.method(IFACE_TEXT, in_signature="i", out_signature="i")
    def GetCharacterAtOffset(self, offset):
        self.touch()
        index = int(offset)
        if 0 <= index < len(self.text):
            return dbus.Int32(ord(self.text[index]))
        return dbus.Int32(0)

    @dbus.service.method(IFACE_TEXT, in_signature="i", out_signature="s")
    def GetAttributeValue(self, offset):
        return dbus.String("")

    @dbus.service.method(IFACE_TEXT, in_signature="i", out_signature="a{ss}")
    def GetAttributes(self, offset):
        return dbus.Dictionary({}, signature="ss")

    @dbus.service.method(IFACE_TEXT, out_signature="a{ss}")
    def GetDefaultAttributes(self):
        return dbus.Dictionary({}, signature="ss")

    @dbus.service.method(IFACE_TEXT, in_signature="iu", out_signature="(iiii)")
    def GetCharacterExtents(self, offset, coord_type):
        x = 10 * int(offset)
        return dbus.Struct((dbus.Int32(x), dbus.Int32(0), dbus.Int32(10),
                            dbus.Int32(20)), signature="iiii")

    @dbus.service.method(IFACE_TEXT, in_signature="iiu", out_signature="(iiii)")
    def GetRangeExtents(self, start_offset, end_offset, coord_type):
        return dbus.Struct((dbus.Int32(10 * int(start_offset)), dbus.Int32(0),
                            dbus.Int32(10 * (int(end_offset) - int(start_offset))),
                            dbus.Int32(20)), signature="iiii")

    @dbus.service.method(IFACE_TEXT, in_signature="i", out_signature="b")
    def SetCaretOffset(self, offset):
        self.touch()
        self.control.caret = max(0, int(offset))
        return True

    @dbus.service.method(IFACE_TEXT, out_signature="i")
    def GetCaretOffset(self):
        self.touch()
        return dbus.Int32(self.caret)

    @dbus.service.method(IFACE_TEXT, in_signature="ii", out_signature="b")
    def SetSelection(self, start_offset, end_offset):
        return True

    @dbus.service.method(IFACE_TEXT, in_signature="ii", out_signature="b")
    def AddSelection(self, start_offset, end_offset):
        return True

    @dbus.service.method(IFACE_TEXT, in_signature="i", out_signature="b")
    def RemoveSelection(self, selection_num):
        return True

    @dbus.service.method(IFACE_TEXT, in_signature="i", out_signature="(ii)")
    def GetSelection(self, selection_num):
        self.touch()
        return dbus.Struct((dbus.Int32(self.caret), dbus.Int32(self.caret)),
                           signature="ii")

    @dbus.service.method(IFACE_TEXT, out_signature="i")
    def GetNSelections(self):
        return dbus.Int32(0)

    @dbus.service.method(IFACE_TEXT, in_signature="iiu", out_signature="b")
    def ScrollSubstringTo(self, start_offset, end_offset, scroll_type):
        return True

    @dbus.service.method(IFACE_TEXT, in_signature="iiiiu", out_signature="b")
    def ScrollSubstringToPoint(self, start_offset, end_offset, coord_type, x, y):
        return True

    @dbus.service.method(IFACE_TEXT, in_signature="iiu", out_signature="i")
    def GetOffsetAtPoint(self, x, y, coord_type):
        return dbus.Int32(int(x) // 10)

    # -- org.a11y.atspi.Component ------------------------------------------
    @dbus.service.method(IFACE_COMPONENT, in_signature="u", out_signature="(iiii)")
    def GetExtents(self, coord_type):
        return dbus.Struct((dbus.Int32(0), dbus.Int32(0), dbus.Int32(200),
                            dbus.Int32(20)), signature="iiii")

    @dbus.service.method(IFACE_COMPONENT, in_signature="u", out_signature="(ii)")
    def GetPosition(self, coord_type):
        return dbus.Struct((dbus.Int32(0), dbus.Int32(0)), signature="ii")

    @dbus.service.method(IFACE_COMPONENT, out_signature="(ii)")
    def GetSize(self):
        return dbus.Struct((dbus.Int32(200), dbus.Int32(20)), signature="ii")

    @dbus.service.method(IFACE_COMPONENT, in_signature="iiu", out_signature="b")
    def Contains(self, x, y, coord_type):
        return True

    @dbus.service.method(IFACE_COMPONENT, in_signature="iiu", out_signature="(so)")
    def GetAccessibleAtPoint(self, x, y, coord_type):
        return self.ref(ENTRY_PATH)

    @dbus.service.method(IFACE_COMPONENT, out_signature="b")
    def GrabFocus(self):
        return True

    @dbus.service.method(IFACE_COMPONENT, out_signature="u")
    def GetLayer(self):
        return dbus.UInt32(0)

    @dbus.service.method(IFACE_COMPONENT, out_signature="n")
    def GetMDIZOrder(self):
        return dbus.Int16(0)

    @dbus.service.method(IFACE_COMPONENT, out_signature="d")
    def GetAlpha(self):
        return dbus.Double(1.0)

    @dbus.service.method(IFACE_COMPONENT, in_signature="u", out_signature="b")
    def ScrollTo(self, scroll_type):
        return True

    @dbus.service.method(IFACE_COMPONENT, in_signature="uii", out_signature="b")
    def ScrollToPoint(self, coord_type, x, y):
        return True

    @dbus.service.method(IFACE_COMPONENT, in_signature="iiiiu", out_signature="b")
    def SetExtents(self, x, y, w, h, coord_type):
        return True

    @dbus.service.method(IFACE_COMPONENT, in_signature="iu", out_signature="b")
    def SetPosition(self, x, coord_type):
        return True

    @dbus.service.method(IFACE_COMPONENT, in_signature="iu", out_signature="b")
    def SetSize(self, w, h):
        return True

    # -- org.a11y.atspi.EditableText ---------------------------------------
    @dbus.service.method(IFACE_EDITABLE_TEXT, in_signature="sii", out_signature="b")
    def SetTextContents(self, new_contents, start, end):
        return True


class FocusedButton(AccessibleObject):
    """没有 `Text` 接口的焦点对象（「按钮 / 列表项」这类控件）。"""

    def __init__(self, bus, ref, focused):
        states = {STATE_ENABLED, STATE_FOCUSABLE, STATE_SENSITIVE, STATE_SHOWING,
                  STATE_VISIBLE}
        if focused:
            states.add(STATE_FOCUSED)
        super().__init__(bus, ENTRY_PATH, ref, "mock button", ROLE_PUSH_BUTTON, states,
                         [IFACE_ACCESSIBLE, IFACE_COMPONENT], ROOT_PATH)


class AppRoot(AccessibleObject):
    """应用根对象：`Socket.Embed` 的提供者，也是 `org.a11y.atspi.Application`。"""

    def __init__(self, bus, ref, child):
        super().__init__(bus, ROOT_PATH, ref, "mock-app", ROLE_APPLICATION,
                         {STATE_ENABLED, STATE_SENSITIVE, STATE_SHOWING, STATE_VISIBLE},
                         [IFACE_ACCESSIBLE, IFACE_APPLICATION, IFACE_SOCKET], NULL_PATH)
        self.child = child

    def child_paths(self):
        return [ENTRY_PATH]

    @dbus.service.method(IFACE_APPLICATION, out_signature="s")
    def GetApplicationBusName(self):
        return dbus.String(self.ref.bus_name)

    @dbus.service.method(IFACE_SOCKET, in_signature="(so)", out_signature="(so)")
    def Embed(self, plug):
        return self.ref(ROOT_PATH)

    @dbus.service.method(IFACE_SOCKET, in_signature="(so)")
    def Unembed(self, plug):
        return

    @dbus.service.method(IFACE_SOCKET, in_signature="s")
    def Embedded(self, path):
        return


class Cache(dbus.service.Object):
    """`org.a11y.atspi.Cache`（2.54+ 的应用侧缓存）。

    `CacheItem` 签名：`((so)(so)(so)a(so)assusau)`。
    注意：registry 的全局缓存被取字来源**有意忽略**（实测返回空表），此处只为贴近真实应用。
    """

    def __init__(self, bus, ref, root, child):
        super().__init__(bus, CACHE_PATH)
        self.ref = ref
        self.root = root
        self.child = child

    def _item(self, obj, interfaces):
        return dbus.Struct((
            self.ref(obj._object_path),
            self.ref(obj.parent_path),
            self.ref(obj.parent_path),
            dbus.Array([self.ref(p) for p in obj.child_paths()], signature="(so)"),
            dbus.Array(interfaces, signature="s"),
            dbus.String(obj.obj_name),
            dbus.UInt32(obj.role),
            dbus.String(""),
            state_array(obj.states),
        ), signature="(so)(so)(so)a(so)assusau")

    @dbus.service.method(IFACE_CACHE, out_signature="a((so)(so)(so)a(so)assusau)")
    def GetItems(self):
        return dbus.Array([self._item(self.root, self.root.ifaces),
                           self._item(self.child, self.child.ifaces)],
                          signature="((so)(so)(so)a(so)assusau)")


class MockApp:
    def __init__(self):
        dbus.mainloop.glib.DBusGMainLoop(set_as_default=True)
        self.text = os.environ.get("MOCK_TEXT", "中欧中兴")
        self.caret = int(os.environ.get("MOCK_CARET", "2"))
        self.lifetime = float(os.environ.get("MOCK_LIFETIME", "30"))
        self.entry_kind = os.environ.get("MOCK_ENTRY", "entry")
        self.control = ControlFile(os.environ.get("MOCK_CONTROL_FILE"), self.text,
                                   self.caret)
        # 控制文件的初值以文件为准：probe 改文件后 mock 每次调用都读到最新值。
        self.control.refresh()
        self.bus = dbus.bus.BusConnection(self._a11y_address())
        self.ref = Ref(self.bus)
        if self.entry_kind == "button":
            self.child = FocusedButton(self.bus, self.ref, True)
        else:
            self.child = TextEntry(self.bus, self.ref, self.control, True)
        self.root = AppRoot(self.bus, self.ref, self.child)
        self.cache = Cache(self.bus, self.ref, self.root, self.child)
        log("[mock] a11y bus name = %s / entry=%s" % (self.ref.bus_name,
                                                      self.entry_kind))

    @staticmethod
    def _a11y_address():
        address = os.environ.get("AT_SPI_BUS_ADDRESS")
        if address:
            log("[mock] 用 AT_SPI_BUS_ADDRESS 指定的无障碍总线")
            return address
        session = dbus.SessionBus()
        obj = session.get_object("org.a11y.Bus", "/org/a11y/bus")
        address = str(obj.GetAddress(dbus_interface="org.a11y.Bus"))
        log("[mock] org.a11y.Bus.GetAddress -> %s" % address)
        return address

    def do_embed(self):
        """注册进 registry。必须在主循环起来之后做：registry 会**回调**本进程的对象。"""
        try:
            registry = self.bus.get_object(REGISTRY_BUS, REGISTRY_SOCKET_PATH)
            socket = registry.Embed(self.ref(ROOT_PATH), dbus_interface=IFACE_SOCKET,
                                    timeout=10)
            log("[mock] Embed ok, socket=%s" % (socket,))
            print("MOCK_EMBEDDED %s %s" % (self.ref.bus_name, ROOT_PATH), flush=True)
        except Exception as exc:  # noqa: BLE001 - 任何 D-Bus 失败都只报不抛
            log("[mock] Embed FAILED: %r" % (exc,))
            print("MOCK_EMBED_FAILED %r" % (exc,), flush=True)
        return False

    def run(self):
        loop = GLib.MainLoop()
        GLib.timeout_add(200, self.do_embed)
        if self.lifetime > 0:
            GLib.timeout_add(int(self.lifetime * 1000), loop.quit)
        loop.run()
        log("[mock] exiting")


if __name__ == "__main__":
    MockApp().run()
