// 键序列金样探针（2c）：真 librime + librime-lua 驱动 pin 版虎整句 Lua 核心。
//
// 用法：rime_sequence_probe <user_dir> <shared_dir> <lua_plugin> <cases_file>
// 输出：每步一行 TSV：
//   step <case> <index> <repr> <consumed 0/1> <input> <caret> <commit> <preedit>
//        <page> <highlight> <candidate_count> <candidates>
// 文本字段为 UTF-8 字节十六进制，空串为 "-"，候选以 "," 分隔、空为 "-"。
// 与 tools/gen_key_sequence_golden.sh 配套；探针依赖系统 librime/librime-lua，
// 故金样不在 CI 重生成（同 key.tsv.gz）。
#include <rime_api.h>

#include <dlfcn.h>

#include <cstddef>
#include <fstream>
#include <iostream>
#include <map>
#include <sstream>
#include <stdexcept>
#include <string>

namespace {

RimeApi* api = nullptr;
RimeSessionId session = 0;

void check(bool ok, const std::string& message) {
    if (!ok) throw std::runtime_error(message);
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
        {"Tab", {0xff09, 0}},        {"ISO_Left_Tab", {0xfe20, 0}},
        {"Shift+Tab", {0xff09, 1}},  {"Return", {0xff0d, 0}},
        {"KP_Enter", {0xff8d, 0}},   {"Escape", {0xff1b, 0}},
        {"BackSpace", {0xff08, 0}},  {"Delete", {0xffff, 0}},
        {"Left", {0xff51, 0}},       {"Up", {0xff52, 0}},
        {"Right", {0xff53, 0}},      {"Down", {0xff54, 0}},
        {"Page_Up", {0xff55, 0}},    {"Page_Down", {0xff56, 0}},
        {"Home", {0xff50, 0}},       {"End", {0xff57, 0}},
        {"KP_Decimal", {0xffae, 0}},
    };
    return table;
}

Key resolve(const std::string& repr) {
    if (repr.size() == 1) {
        const char ch = repr[0];
        if ((ch >= 'a' && ch <= 'z') || (ch >= '0' && ch <= '9')) return {ch, 0};
    }
    const auto found = key_table().find(repr);
    if (found == key_table().end()) throw std::runtime_error("unknown key repr: " + repr);
    return found->second;
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
    const int count = context.menu.num_candidates;
    for (int i = 0; i < count; ++i) {
        if (i) candidates << ',';
        candidates << hex(context.menu.candidates[i].text ? context.menu.candidates[i].text : "");
    }
    if (count == 0) candidates << "-";
    std::cout << "step\t" << name << '\t' << index << '\t' << repr << '\t' << (consumed ? 1 : 0)
              << '\t' << hex(input()) << '\t' << api->get_caret_pos(session) << '\t'
              << hex(commit) << '\t' << hex(preedit) << '\t' << context.menu.page_no << '\t'
              << context.menu.highlighted_candidate_index << '\t' << count << '\t'
              << candidates.str() << '\n';
    api->free_context(&context);
}

void apply_option(const std::string& assignment) {
    const auto equals = assignment.find('=');
    check(equals != std::string::npos, "bad option assignment: " + assignment);
    const std::string name = assignment.substr(0, equals);
    const std::string value = assignment.substr(equals + 1);
    api->set_option(session, name.c_str(), value == "1" ? True : False);
}

void reset(const std::string& options) {
    api->clear_composition(session);
    std::string discarded;
    drain_commit(discarded);
    api->set_option(session, "ascii_mode", False);
    api->set_option(session, "tiger_sentence_early_commit", True);
    api->set_option(session, "tiger_sentence_early_commit_to_preedit", False);
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
        if (api->start_maintenance(True)) api->join_maintenance_thread();
        session = api->create_session();
        check(session != 0, "Cannot create Rime session");
        check(api->select_schema(session, "tiger_sentence") != 0,
              "Cannot deploy/select tiger_sentence");

        std::ifstream cases(argv[4]);
        check(cases.good(), "Cannot read cases file");
        std::string line;
        while (std::getline(cases, line)) {
            if (line.empty() || line[0] == '#') continue;
            std::istringstream fields(line);
            std::string name;
            std::string options;
            std::string keys;
            check(static_cast<bool>(std::getline(fields, name, '\t')) &&
                      static_cast<bool>(std::getline(fields, options, '\t')) &&
                      static_cast<bool>(std::getline(fields, keys)),
                  "bad case line: " + line);
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
