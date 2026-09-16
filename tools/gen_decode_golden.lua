-- 生成 decode 金样（冷路径：include_early_commit=false；未接入学习）。
--
--   lua tools/gen_decode_golden.lua --reference <repo> --data <dir> --out <tsv> [--model <bin>] [--every N] [--duplicate 0|1] [--early-commit 0|1] [--required 0|1]
--
-- --required 1：对每 3 个输入追加一次“必需前缀”遍（前缀取该输入首候选的首字符），
-- 覆盖 build_early_commit_evidence 的 required_text_prefix 过滤路径。
--
-- 数据目录需含四个数据文件；--model 时把模型拷贝为临时用户目录的
-- models/sentence-ngram-mobile.bin 并启用（走参照的 try_load 路径）。
-- transcript 记录（tab 分隔，`#` 注释，`-` 表示空串）：
--   decode <hex input> count=<n> learning=<0|1> truncated=<0|1> required=<hex prefix|->
--   result <hex text> <hex segmented> <bits score> <bits confidence_score> <max_rank> <edge_count> <bits supplement_score> <bits learning_score>
--   evidence <hex proposal> <bits proposal_share> nit= mit= nlc= trunc= prefixes= raws=   （--early-commit 1）
--   prefix <hex text> <raw_length> <bits share> <bits boundary_share> <closed> <chars>
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
local reference = opts.reference or os.getenv("TIGERCLAW_REFERENCE_REPO") or "../tiger-sentense-rime"
assert(opts.data, "missing --data")
assert(opts.out, "missing --out")
local every = tonumber(opts.every or "1") or 1

local base = os.getenv("TMPDIR") or "/tmp"
local work = base .. "/tigerclaw-decode-" .. tostring(os.time()) .. "-" ..
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

package.path = reference .. "/lua/?.lua;" .. package.path
rime_api = { get_user_data_dir = function() return work end }
local sentence = require("tiger_sentence")
sentence.set_model_enabled(opts.model ~= nil)
sentence.ensure_lexicon(nil)
local duplicate = opts.duplicate ~= "0"
local early = opts["early-commit"] == "1"
local required_mode = opts.required == "1"
if not duplicate then
    -- 参照测试同款：以假 context 关闭“单字重码组句”。
    sentence.set_allow_duplicate_single({ get_option = function() return false end })
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
            bits(item.supplement_score or 0), bits(item.learning_score or 0))
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
                bits(prefix.boundary_share), prefix.boundary_closed and 1 or 0,
                tostring(prefix.text_char_count))
        end
        for _, text in ipairs(raw_keys) do
            emit("rawlen", hex(text), tostring(evidence.raw_lengths[text]))
        end
    end
end

emit("# decode transcript; model=" .. (opts.model and "fixture" or "off") ..
    " duplicate=" .. (duplicate and 1 or 0) .. " early=" .. (early and 1 or 0) ..
    " required=" .. (required_mode and 1 or 0))
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
