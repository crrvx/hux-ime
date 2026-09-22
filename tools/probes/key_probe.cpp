// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

// 键金样探针：调用系统 librime 的键名表与 KeyEvent，输出 TSV 供差分测试。
//
// 为避免依赖 librime 构建期头文件（build_config.h/boost），此处自声明
// 最小接口；类布局与 `rime/key_event.h` 一致，方法符号由 librime 导出。
//
//   g++ -std=c++17 -O2 tools/probes/key_probe.cpp -lrime -o key_probe
//   key_probe <keyvals.txt> <cases.txt>
//
// 两个输入文件不可读、或其中之一为空（0 条）时 `exit 2`——残缺输出会污染入库金样。
//
// 记录（tab 分隔）：
//   name      <keyval> <name|->
//   repr      <keyval> <modifier> <repr>
//   parse     <repr> <ok|bad> <keycode> <modifier> <repr>
//   modifier  <index> <name|->
#include <cstdlib>
#include <fstream>
#include <iostream>
#include <string>

// librime `rime/key_table.h` 中的常量（自声明，值同源码）。
constexpr int kShiftMask = 1 << 0;
constexpr int kControlMask = 1 << 2;
constexpr int kAltMask = 1 << 3;

// librime 导出符号（C++ 链接名）。
const char* RimeGetKeyName(int keycode);
const char* RimeGetModifierName(int modifier);

namespace rime {

// 与 `rime/key_event.h` 同布局；Parse/repr 由 librime 提供。
class KeyEvent {
public:
    KeyEvent() = default;
    KeyEvent(int keycode, int modifier) : keycode_(keycode), modifier_(modifier) {}
    int keycode() const { return keycode_; }
    int modifier() const { return modifier_; }
    bool Parse(const std::string& repr);
    std::string repr() const;

private:
    int keycode_ = 0;
    int modifier_ = 0;
};

} // namespace rime

int main(int argc, char** argv) {
    if (argc != 3) {
        std::cerr << "usage: key_probe <keyvals.txt> <cases.txt>\n";
        return 2;
    }
    int keyvals_ = 0;
    std::ifstream keyvals(argv[1]);
    // 输入不可读即失败退出：否则只输出 32 条 `modifier`，会把入库金样静默覆盖成 32 行。
    if (!keyvals) {
        std::cerr << "cannot open keyvals file: " << argv[1] << "\n";
        return 2;
    }
    std::string line;
    while (std::getline(keyvals, line)) {
        if (line.empty()) continue;
        const int keyval = static_cast<int>(std::strtol(line.c_str(), nullptr, 0));
        const char* name = RimeGetKeyName(keyval);
        std::cout << "name\t" << keyval << '\t' << (name ? name : "-") << '\n';
        std::cout << "repr\t" << keyval << "\t0\t" << rime::KeyEvent(keyval, 0).repr() << '\n';
        std::cout << "repr\t" << keyval << "\t" << kShiftMask << "\t"
                  << rime::KeyEvent(keyval, kShiftMask).repr() << '\n';
        std::cout << "repr\t" << keyval << "\t" << (kControlMask | kAltMask) << "\t"
                  << rime::KeyEvent(keyval, kControlMask | kAltMask).repr() << '\n';
        ++keyvals_;
    }
    int cases_ = 0;
    std::ifstream cases(argv[2]);
    if (!cases) {
        std::cerr << "cannot open cases file: " << argv[2] << "\n";
        return 2;
    }
    while (std::getline(cases, line)) {
        if (line.empty() || line[0] == '#') continue;
        rime::KeyEvent event;
        const bool ok = event.Parse(line);
        std::cout << "parse\t" << line << '\t' << (ok ? "ok" : "bad") << '\t'
                  << (ok ? event.keycode() : 0) << '\t' << (ok ? event.modifier() : 0) << '\t'
                  << (ok ? event.repr() : "-") << '\n';
        ++cases_;
    }
    for (int index = 0; index < 32; ++index) {
        const char* name = RimeGetModifierName(1u << index);
        std::cout << "modifier\t" << index << '\t' << (name ? name : "-") << '\n';
    }
    std::cerr << "keyvals=" << keyvals_ << " cases=" << cases_ << "\n";
    // 空输入不是「成功的空金样」：报错退出，交由生成器决定是否写库（`gen_key_golden.sh`
    // 另有「至少 1 条 name / 1 条 parse」的写库前置断言）。
    if (keyvals_ == 0 || cases_ == 0) {
        std::cerr << "empty input: keyvals=" << keyvals_ << " cases=" << cases_ << "\n";
        return 2;
    }
    return 0;
}
