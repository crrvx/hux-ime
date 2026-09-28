// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

// 键序列金样探针（2c）：真 librime + librime-lua 驱动 pin 版虎句 Lua 核心。
//
// 用法：rime_sequence_probe <user_dir> <shared_dir> <lua_plugin> <cases_file>
// 输出：每步一行 TSV：
//   step <case> <index> <repr> <consumed 0/1> <input> <caret> <commit> <preedit>
//        <page> <highlight> <candidate_count> <candidates> <comments>
// 文本字段为 UTF-8 字节十六进制，空串为 "-"，候选/注释以 "," 分隔、空为 "-"。
// repr 由上游 `KeyEvent::Parse` 解析（librime 自己的键名/修饰表）：单字符、键名
// （space/comma/period/.../BackSpace/Left/...）、`<修饰>+<键名>`；解析失败即显式报错。
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

namespace rime {

// 与 `rime/key_event.h` 同布局；Parse 由 librime 提供（头文件不随 librime-dev 分发）。
class KeyEvent {
public:
    KeyEvent() = default;
    int keycode() const { return keycode_; }
    int modifier() const { return modifier_; }
    bool Parse(const std::string& repr);

private:
    int keycode_ = 0;
    int modifier_ = 0;
};

}  // namespace rime

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

// 键表示 → 键值/修饰掩码。交给上游 `KeyEvent::Parse`：探针曾自带一份「与 librime 同构」
// 的键名与修饰表（46 键 + 8 修饰），那份副本一旦与上游漂移，金样会**静默**改变；改为直调
// 上游后，未识别的键名/修饰由 Parse 返回 false（librime 记 ERROR 日志），此处显式失败。
Key resolve(const std::string& repr) {
    rime::KeyEvent event;
    check(event.Parse(repr), "unknown key repr: " + repr);
    return {event.keycode(), event.modifier()};
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
