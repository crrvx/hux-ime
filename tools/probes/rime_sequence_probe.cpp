// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

// 键序列金样探针（2c）：真 librime + librime-lua 驱动 pin 版虎句 Lua 核心。
//
// 用法：rime_sequence_probe <user_dir> <shared_dir> <lua_plugin> <cases_file>
// 输出：每步一行 TSV：
//   step <case> <index> <repr> <consumed 0/1> <input> <caret> <commit> <preedit>
//        <page> <highlight> <candidate_count> <candidates> <comments>
// 文本字段为 UTF-8 字节十六进制，空串为 "-"，候选/注释以 "," 分隔、空为 "-"。
// repr 支持：单字符可打印键、键名（space/comma/period/.../BackSpace/Left/...）、
// `<修饰>+<键名>`（Shift/Lock/Control/Alt/Super/Hyper/Meta/Release，如 Release+Shift_L）。
// 与 tools/generators/gen_key_sequence_golden.sh 配套；探针依赖系统 librime/librime-lua，
// 故金样不在 CI 重生成（同 key.tsv.gz）。
#include <rime_api.h>

#include <dlfcn.h>

#include <cstddef>
#include <fstream>
#include <iostream>
#include <map>
#include <set>
#include <sstream>
#include <stdexcept>
#include <string>

namespace {

RimeApi* api = nullptr;
RimeSessionId session = 0;
std::set<std::string> declared_switches;  // 已部署方案 `switches` 声明的选项名

void check(bool ok, const std::string& message) {
    if (!ok) throw std::runtime_error(message);
}

// 已部署方案的 `switches` 名单（方案选项名是否仍然存在的唯一可信来源）。
std::set<std::string> load_declared_switches(const std::string& schema_id) {
    std::set<std::string> names;
    RimeConfig config{};
    check(api->schema_open(schema_id.c_str(), &config),
          "Cannot open schema config: " + schema_id);
    RimeConfigIterator iterator{};
    if (api->config_begin_list(&iterator, &config, "switches")) {
        while (api->config_next(&iterator)) {
            char name[256] = {0};
            const std::string path = std::string(iterator.path ? iterator.path : "") + "/name";
            if (api->config_get_string(&config, path.c_str(), name, sizeof(name))) {
                names.insert(name);
            }
        }
        api->config_end(&iterator);
    }
    api->config_close(&config);
    return names;
}

// 设置选项。这里**不能**检查 `api->set_option` 的「返回值」：librime 1.17 的
// `RimeApi::set_option` 返回 `void`（`rime_api.h:333`），且 librime 不校验选项名——
// 任意名字 `set_option` 之后 `get_option` 都回读为真（实测 `tiger_sentence_bogus_zzz`
// 亦然），故「设完回读」是恒真检查，发现不了「方案改键名」。
// 方案选项（`tiger_sentence_` 前缀：探针内置的两个 + 用例第二列声明的赋值）改为断言
// 「已部署方案的 `switches` 里声明过该名字」——改键名即在此显式失败，而不是静默退回
// 默认选项、让金样与重放侧「一致地错」。
// 宿主/rime 标准选项（ascii_mode、full_shape、ascii_punct）不属本探针契约，不做断言。
void set_option_checked(const std::string& name, Bool value) {
    if (name.rfind("tiger_sentence_", 0) == 0) {
        check(declared_switches.count(name) != 0,
              "scheme option not declared in schema switches: " + name);
    }
    api->set_option(session, name.c_str(), value);
}

std::string hex(const std::string& text) {
    static const char* digits = "0123456789abcdef";
    std::string out;
    out.reserve(text.size() * 2);
    for (unsigned char byte : text) {
        out.push_back(digits[byte >> 4]);
        out.push_back(digits[byte & 0x0f]);
    }
    return out.empty() ? "-" : out;
}

struct Key {
    int code;
    int mask;
};

const std::map<std::string, Key>& key_table() {
    static const std::map<std::string, Key> table = {
        {"space", {0x20, 0}},        {"apostrophe", {0x27, 0}},
        {"semicolon", {0x3b, 0}},    {"period", {0x2e, 0}},
        {"comma", {0x2c, 0}},        {"minus", {0x2d, 0}},
        {"equal", {0x3d, 0}},        {"slash", {0x2f, 0}},
        {"backslash", {0x5c, 0}},    {"grave", {0x60, 0}},
        {"bracketleft", {0x5b, 0}},  {"bracketright", {0x5d, 0}},
        {"Tab", {0xff09, 0}},        {"ISO_Left_Tab", {0xfe20, 0}},
        {"Return", {0xff0d, 0}},     {"KP_Enter", {0xff8d, 0}},
        {"Escape", {0xff1b, 0}},     {"BackSpace", {0xff08, 0}},
        {"Delete", {0xffff, 0}},     {"Left", {0xff51, 0}},
        {"Up", {0xff52, 0}},         {"Right", {0xff53, 0}},
        {"Down", {0xff54, 0}},       {"Page_Up", {0xff55, 0}},
        {"Page_Down", {0xff56, 0}},  {"Home", {0xff50, 0}},
        {"End", {0xff57, 0}},        {"KP_Decimal", {0xffae, 0}},
        // 修饰键（ascii_composer 切换与标点用例）。
        {"Shift_L", {0xffe1, 0}},    {"Shift_R", {0xffe2, 0}},
        {"Control_L", {0xffe3, 0}},  {"Control_R", {0xffe4, 0}},
        {"Alt_L", {0xffe9, 0}},      {"Alt_R", {0xffea, 0}},
        {"Super_L", {0xffeb, 0}},    {"Super_R", {0xffec, 0}},
        {"Caps_Lock", {0xffe5, 0}},  {"Eisu_toggle", {0xff30, 0}},
    };
    return table;
}

int modifier_mask(const std::string& name) {
    static const std::map<std::string, int> table = {
        {"Shift", 1 << 0},   {"Lock", 1 << 1},    {"Control", 1 << 2},
        {"Alt", 1 << 3},     {"Super", 1 << 26},  {"Hyper", 1 << 27},
        {"Meta", 1 << 28},   {"Release", 1 << 30},
    };
    const auto found = table.find(name);
    return found == table.end() ? -1 : found->second;
}

Key resolve(const std::string& repr) {
    // 可打印单字符直接作键值（不足两字符的名字不存在）。
    if (repr.size() == 1) {
        const unsigned char ch = static_cast<unsigned char>(repr[0]);
        if (ch > 0x20 && ch < 0x7f) return {ch, 0};
    }
    // `<修饰>+<键名>`（与 librime `KeyEvent::Parse` 同构）。
    int mask = 0;
    std::string key_name = repr;
    const auto plus = repr.rfind('+');
    if (plus != std::string::npos) {
        key_name = repr.substr(plus + 1);
        const std::string rest = repr.substr(0, plus);
        std::size_t start = 0;
        while (start <= rest.size()) {
            const auto next = rest.find('+', start);
            const std::string name =
                rest.substr(start, next == std::string::npos ? std::string::npos : next - start);
            const int bit = modifier_mask(name);
            if (bit < 0) throw std::runtime_error("unknown modifier: " + name);
            mask |= bit;
            if (next == std::string::npos) break;
            start = next + 1;
        }
    }
    const auto found = key_table().find(key_name);
    if (found == key_table().end()) throw std::runtime_error("unknown key repr: " + repr);
    return {found->second.code, found->second.mask | mask};
}

std::string input() {
    const char* value = api->get_input(session);
    return value ? value : "";
}

void drain_commit(std::string& out) {
    RIME_STRUCT(RimeCommit, commit);
    if (api->get_commit(session, &commit)) {
        if (commit.text) out += commit.text;
        api->free_commit(&commit);
    }
}

void snapshot(const std::string& name,
              std::size_t index,
              const std::string& repr,
              bool consumed,
              const std::string& commit) {
    RIME_STRUCT(RimeContext, context);
    check(api->get_context(session, &context) != 0, "get_context failed");
    const std::string preedit =
        context.composition.preedit ? context.composition.preedit : "";
    std::ostringstream candidates;
    std::ostringstream comments;
    const int count = context.menu.num_candidates;
    for (int i = 0; i < count; ++i) {
        if (i) {
            candidates << ',';
            comments << ',';
        }
        candidates << hex(context.menu.candidates[i].text ? context.menu.candidates[i].text : "");
        comments << hex(context.menu.candidates[i].comment ? context.menu.candidates[i].comment : "");
    }
    if (count == 0) {
        candidates << "-";
        comments << "-";
    }
    std::cout << "step\t" << name << '\t' << index << '\t' << repr << '\t' << (consumed ? 1 : 0)
              << '\t' << hex(input()) << '\t' << api->get_caret_pos(session) << '\t'
              << hex(commit) << '\t' << hex(preedit) << '\t' << context.menu.page_no << '\t'
              << context.menu.highlighted_candidate_index << '\t' << count << '\t'
              << candidates.str() << '\t' << comments.str() << '\n';
    api->free_context(&context);
}

void apply_option(const std::string& assignment) {
    const auto equals = assignment.find('=');
    check(equals != std::string::npos, "bad option assignment: " + assignment);
    const std::string name = assignment.substr(0, equals);
    const std::string value = assignment.substr(equals + 1);
    set_option_checked(name, value == "1" ? True : False);
}

void reset(const std::string& options) {
    api->clear_composition(session);
    std::string discarded;
    drain_commit(discarded);
    set_option_checked("ascii_mode", False);
    set_option_checked("full_shape", False);
    set_option_checked("ascii_punct", False);
    set_option_checked("tiger_sentence_early_commit", True);
    set_option_checked("tiger_sentence_early_commit_to_preedit", False);
    std::istringstream stream(options);
    std::string item;
    while (std::getline(stream, item, ',')) {
        if (!item.empty()) apply_option(item);
    }
}

}  // namespace

int main(int argc, char** argv) {
    try {
        check(argc == 5,
              "Usage: rime_sequence_probe <user_dir> <shared_dir> <lua_plugin> <cases_file>");
        api = rime_get_api();
        check(dlopen(argv[3], RTLD_NOW | RTLD_GLOBAL) != nullptr,
              "Cannot load librime-lua plugin");
        const char* modules[] = {"default", "lua", nullptr};
        RIME_STRUCT(RimeTraits, traits);
        traits.shared_data_dir = argv[2];
        traits.user_data_dir = argv[1];
        traits.log_dir = argv[1];
        traits.app_name = "rime.tiger.keyseq";
        traits.modules = modules;
        api->setup(&traits);
        api->initialize(&traits);
        // 维护失败必须显式报错；先 join 再 check：失败路径也不留后台线程。
        const Bool maintenance_started = api->start_maintenance(True);
        api->join_maintenance_thread();
        check(maintenance_started, "Cannot start maintenance");
        session = api->create_session();
        check(session != 0, "Cannot create Rime session");
        check(api->select_schema(session, "tiger_sentence") != 0,
              "Cannot deploy/select tiger_sentence");
        declared_switches = load_declared_switches("tiger_sentence");

        std::ifstream cases(argv[4]);
        check(cases.good(), "Cannot read cases file");
        std::string line;
        std::map<std::string, int> seen_lines;  // 用例名 -> 行号
        int line_no = 0;
        while (std::getline(cases, line)) {
            ++line_no;
            if (line.empty() || line[0] == '#') continue;
            std::istringstream fields(line);
            std::string name;
            std::string options;
            std::string keys;
            check(static_cast<bool>(std::getline(fields, name, '\t')) &&
                      static_cast<bool>(std::getline(fields, options, '\t')) &&
                      static_cast<bool>(std::getline(fields, keys)),
                  "bad case line: " + line);
            // 重名用例会让金样出现重复 `case`，重放侧可能只取其一。
            const auto inserted = seen_lines.emplace(name, line_no);
            check(inserted.second,
                  "duplicate case name: " + name + " (line " + std::to_string(line_no) +
                      ", first at line " + std::to_string(inserted.first->second) + ")");
            reset(options);
            std::cout << "case\t" << name << '\t' << options << '\n';
            std::istringstream key_stream(keys);
            std::string repr;
            std::size_t index = 0;
            while (key_stream >> repr) {
                const Key key = resolve(repr);
                const bool consumed = api->process_key(session, key.code, key.mask) != 0;
                std::string commit;
                drain_commit(commit);
                snapshot(name, index++, repr, consumed, commit);
            }
        }
        api->destroy_session(session);
        session = 0;
        api->finalize();
        api = nullptr;
        return 0;
    } catch (const std::exception& e) {
        std::cerr << "probe error: " << e.what() << '\n';
        if (session && api) api->destroy_session(session);
        if (api) api->finalize();
        return 1;
    }
}
