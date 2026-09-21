-- SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
-- SPDX-License-Identifier: GPL-3.0-or-later

-- 生成 decode 金样（冷路径：include_early_commit=false；未接入学习）。
--
--   lua tools/generators/gen_decode_golden.lua --reference <repo> --data <dir> --out <tsv> [--model <bin>] [--every N] [--duplicate 0|1] [--early-commit 0|1] [--required 0|1] [--learning 0|1]
--
-- --required 1：对每 3 个输入追加一次“必需前缀”遍（前缀取该输入首候选的首字符），
-- 覆盖 build_early_commit_evidence 的 required_text_prefix 过滤路径。
-- --learning 1：用数据中的真实候选构造纠错事件，经 set_learning_for_test 接入解码，
-- 并在 transcript 头部输出 learningsetup + levent 使 Rust 侧可重建同一索引。
-- 两个开关**可并用**：`--early-commit 1 --required 1 --learning 1` 即
-- `goldens/decode_learning_evidence.tsv.gz`（遗留②，覆盖「学习 × 证据抑制」的交互：
-- 学习生效且截断的记录、`share`/`base_share` 双权重）。
--
-- 数据目录需含四个数据文件；--model 时把模型拷贝为临时用户目录的
-- models/sentence-ngram-mobile.bin 并启用（走参照的 try_load 路径）。
-- transcript 记录（tab 分隔，`#` 注释，`-` 表示空串）：
--   decode <hex input> count=<n> learning=<0|1> truncated=<0|1> required=<hex prefix|->
--   result <hex text> <hex segmented> <bits score> <bits confidence_score> <max_rank> <edge_count> <bits supplement_score> <bits learning_score> <bits early_commit_confidence_score>
--   evidence <hex proposal> <bits proposal_share> nit= mit= nlc= trunc= prefixes= raws=   （--early-commit 1）
--   prefix <hex text> <raw_length> <bits share> <bits base_share> <bits boundary_share> <closed> <chars>
--   rawlen <hex text> <raw_length>

local function parse_args(argv)
    local opts = {}
    local i = 1
    while i <= #argv do
        local key = argv[i]:match("^%-%-([%w_%-]+)$")
        if not key then error("unexpected argument: " .. argv[i]) end
        opts[key] = argv[i + 1]
        i = i + 2
    end
    return opts
end

local opts = parse_args({ ... })
-- 默认参照检出：与仓库同级（相对脚本位置解析，不依赖调用时的 cwd）。
local script_dir = (arg and arg[0] or ""):match("^(.*)[/\\]") or "."
local reference = opts.reference or os.getenv("HUX_REFERENCE_REPO")
    or (script_dir .. "/../../external/tiger-sentense-rime")
assert(opts.data, "missing --data")
assert(opts.out, "missing --out")
local every = tonumber(opts.every or "1") or 1

local base = os.getenv("TMPDIR") or "/tmp"
local work = base .. "/hux-decode-" .. tostring(os.time()) .. "-" ..
    tostring(math.floor(os.clock() * 1000000))
os.execute("rm -rf '" .. work .. "' && mkdir -p '" .. work .. "/models'")

local function copy_file(source, destination)
    local input = io.open(source, "rb")
    if not input then return false end
    local content = input:read("*a")
    input:close()
    local output = assert(io.open(destination, "wb"))
    output:write(content)
    output:close()
    return true
end

for _, name in ipairs({
    "tiger_sentence.codes.txt",
    "tiger_sentence.char_ranks.txt",
    "tiger_sentence.full_code_whitelist.txt",
    "tiger_sentence.supplement.txt",
}) do
    assert(copy_file(opts.data .. "/" .. name, work .. "/" .. name), "missing data file: " .. name)
end
if opts.model then
    assert(copy_file(opts.model, work .. "/models/sentence-ngram-mobile.bin"), "model copy failed")
end
if opts.lexical then
    assert(copy_file(opts.lexical, work .. "/tiger_sentence.lexical.bin"), "lexical copy failed")
end

package.path = reference .. "/lua/?.lua;" .. package.path
rime_api = { get_user_data_dir = function() return work end }
local sentence = require("tiger_sentence")
sentence.set_model_enabled(opts.model ~= nil)
sentence.ensure_lexicon(nil)
local duplicate = opts.duplicate ~= "0"
local early = opts["early-commit"] == "1"
local required_mode = opts.required == "1"
local learning_flag = opts.learning == "1"
if not duplicate then
    -- 参照测试同款：以假 context 关闭“单字重码组句”。
    sentence.set_allow_duplicate_single({ get_option = function() return false end })
end

local learning_events = {}
local learning_now = 0
if learning_flag then
    -- 纠错事件取自数据中的真实候选，保证学习评分非零。
    local module = require("tiger_sentence_learning")
    local view = sentence.lexicon_data_view()
    local function pick(code, index)
        local entries = view.codes[code]
        if not entries or not entries[index] then return nil end
        return entries[index].t
    end
    local first_a, second_a = pick("a", 1), pick("a", 2)
    local first_ab, second_ab = pick("ab", 1), pick("ab", 2)
    local second_abc = pick("abc", 2)
    local context = first_ab and utf8.char(utf8.codepoint(first_ab)) or ""
    local function learn_event(time, code, text, ctx)
        if text then
            learning_events[#learning_events + 1] = {
                time = time, mode = "t", code = code, text = text, context = ctx or "",
            }
        end
    end
    for _ = 1, 3 do learn_event(1000, "a", second_a, "") end
    learn_event(2000, "ab", second_ab, "")
    learn_event(2000, "ab", second_ab, context)
    learn_event(3000, "ab", first_ab, "")
    learn_event(4000, "abc", second_abc, "")
    -- 成对融合偏好（24e633e/59fc87a/a3fc009）：`zzzz` 同时有 Direct（整串单边）与
    -- Composed（多边）候选，记一条「Composed 胜」即可让后者越过前者，
    -- 使 `apply_fusion_ordering` 的归并分支在解码金样里可见（无偏好时保持原序）。
    -- 事件经参照 `learning.fusion_event` 构造，`time` 覆盖为定值以保证 transcript 确定。
    local fusion = module.fusion_event("t", "zzzz", "𨰻", "哥哥", false, 4)
    assert(fusion, "融合事件缺失")
    fusion.time = 1000
    learning_events[#learning_events + 1] = fusion
    learning_now = 40 * 86400
    local index = module.build(learning_events, learning_now)
    sentence.set_learning_for_test(index, "t")
    local _ = first_a
end

local inputs, seen = {}, {}
local function add(text, always)
    if seen[text] then return end
    seen[text] = true
    inputs[#inputs + 1] = { text = text, always = always or false }
end
for _, text in ipairs({
    "a", "b", "c", "aa", "ab", "abc", "ab1", "ab;", "ab'", "ab12", "ab0",
    "zz", "zzzz", "AB", "a b", "123", "", "ab cd", string.rep("ab", 13), string.rep("a", 30),
}) do
    add(text, true)
end
do
    local view = sentence.lexicon_data_view()
    local codes = {}
    for code in pairs(view.codes) do codes[#codes + 1] = code end
    table.sort(codes)
    for index, code in ipairs(codes) do
        if #code <= 2 or index % 97 == 0 then add(code, false) end
    end
end

-- 构造用例恒选；码表派生输入按 --every 抽样（长输入始终覆盖 beam 48 路径）。
local selected, derived = {}, 0
for _, entry in ipairs(inputs) do
    if entry.always then
        selected[#selected + 1] = entry.text
    else
        derived = derived + 1
        if (derived - 1) % every == 0 then selected[#selected + 1] = entry.text end
    end
end

local out = assert(io.open(opts.out, "w"))
local emitted = 0
local function emit(...)
    out:write(table.concat({ ... }, "\t"), "\n")
    emitted = emitted + 1
end
local function hex(text)
    if text == "" then return "-" end
    return (text:gsub(".", function(c) return string.format("%02x", c:byte()) end))
end
local function bits(value)
    local lo, hi = string.unpack("<I4I4", string.pack("<d", value))
    return string.format("0x%08x%08x", hi, lo)
end

local function emit_decode_pass(input, required)
    sentence.reset_decode_cache()
    local results = sentence.decode(input, early, required ~= "" and required or nil)
    emit("decode", hex(input), "count=" .. #results,
        "learning=" .. (results.learning_affected and 1 or 0),
        "truncated=" .. (results._completed_truncated and 1 or 0),
        "required=" .. hex(required or ""))
    for _, item in ipairs(results) do
        emit("result", hex(item.text), hex(item.segmented), bits(item.score),
            bits(item.confidence_score), tostring(item.max_rank), tostring(item.edge_count),
            bits(item.supplement_score or 0), bits(item.learning_score or 0),
            bits(item.early_commit_confidence_score or item.confidence_score))
    end
    if early then
        -- 空编码/无字母输入走参照的早退分支（无证据字段），按缺省证据处理。
        local evidence = results.early_commit_evidence or {
            prefixes = {}, proposal = "", proposal_share = 0.0, raw_lengths = {},
            neutral_incomplete_tail = false, merged_incomplete_tail = false,
            neutral_low_confidence = false, confidence_truncated = false,
        }
        local raw_keys = {}
        for text in pairs(evidence.raw_lengths) do raw_keys[#raw_keys + 1] = text end
        table.sort(raw_keys)
        emit("evidence", hex(evidence.proposal), bits(evidence.proposal_share),
            "nit=" .. (evidence.neutral_incomplete_tail and 1 or 0),
            "mit=" .. (evidence.merged_incomplete_tail and 1 or 0),
            "nlc=" .. (evidence.neutral_low_confidence and 1 or 0),
            "trunc=" .. (evidence.confidence_truncated and 1 or 0),
            "prefixes=" .. #evidence.prefixes,
            "raws=" .. #raw_keys)
        for _, prefix in ipairs(evidence.prefixes) do
            emit("prefix", hex(prefix.text), tostring(prefix.raw_length), bits(prefix.share),
                bits(prefix.base_share), bits(prefix.boundary_share),
                prefix.boundary_closed and 1 or 0,
                tostring(prefix.text_char_count))
        end
        for _, text in ipairs(raw_keys) do
            emit("rawlen", hex(text), tostring(evidence.raw_lengths[text]))
        end
    end
    -- has_complete_candidate：基础 / 排除文本（唯一性）/ 整段锁 / 局部锁（± 必需前缀）。
    if early then
        local function complete_case(excluded, group, lock, required_text)
            local value = sentence.has_complete_candidate(
                input, required_text, excluded, group, lock)
            emit("complete", "input=" .. hex(input), "required=" .. hex(required_text or ""),
                "excluded=" .. hex(excluded or ""), "group=" .. (group and 1 or 0),
                "lock=" .. (lock and (hex(lock.raw) .. "," .. hex(lock.text)) or "-"),
                "result=" .. (value and 1 or 0))
        end
        complete_case(nil, false, nil, "")
        local top = results[1]
        if top and top.text and top.text ~= "" then
            complete_case(top.text, true, nil, "")
            local node = top.path
            local function lock_at(link)
                if not link or not link.raw_length or link.raw_length <= 0 or not link.text_length then
                    return nil
                end
                return { raw = input:sub(1, link.raw_length),
                    text = top.text:sub(1, link.text_length),
                    boundaries = tostring(link.raw_length) .. "," .. tostring(link.text_length) .. ";" }
            end
            local full = lock_at(node)
            if full then
                complete_case(nil, false, full, "")
                complete_case(nil, false, full, full.text)
            end
            local partial = lock_at(node and node.previous)
            if partial then
                complete_case(nil, false, partial, "")
                complete_case(nil, false, partial, partial.text)
            end
        end
    end
end

emit("# decode transcript; model=" .. (opts.model and "fixture" or "off") ..
    " duplicate=" .. (duplicate and 1 or 0) .. " early=" .. (early and 1 or 0) ..
    " required=" .. (required_mode and 1 or 0) .. " learning=" .. (learning_flag and 1 or 0))
if learning_flag then
    emit("learningsetup", tostring(learning_now), hex("t"), tostring(#learning_events))
    for _, e in ipairs(learning_events) do
        emit("levent", tostring(e.time), hex(e.mode), hex(e.code), hex(e.text), hex(e.context))
    end
end
for index, input in ipairs(selected) do
    emit_decode_pass(input, "")
    -- 必需前缀遍：取该输入首候选的首字符（每 3 个输入一次，覆盖过滤路径）。
    if early and required_mode and input ~= "" and index % 3 == 1 then
        sentence.reset_decode_cache()
        local probe = sentence.decode(input, false)
        if #probe > 0 and probe[1].text ~= "" then
            local prefix = utf8.char(utf8.codepoint(probe[1].text))
            emit_decode_pass(input, prefix)
        end
    end
end
out:close()
os.execute("rm -rf '" .. work .. "'")
print(string.format('{"lua":"%s","inputs":%d,"emitted":%d,"model":%s,"duplicate":%s,"early":%s}',
    _VERSION, #selected, emitted, opts.model and "true" or "false", duplicate and "true" or "false",
    early and "true" or "false"))
