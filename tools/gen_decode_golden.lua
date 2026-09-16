-- 生成 decode 金样（冷路径：include_early_commit=false；未接入学习）。
--
--   lua tools/gen_decode_golden.lua --reference <repo> --data <dir> --out <tsv> [--model <bin>] [--every N]
--
-- 数据目录需含四个数据文件；--model 时把模型拷贝为临时用户目录的
-- models/sentence-ngram-mobile.bin 并启用（走参照的 try_load 路径）。
-- transcript 记录（tab 分隔，`#` 注释，`-` 表示空串）：
--   decode <hex input> count=<n> learning=<0|1> truncated=<0|1>
--   result <hex text> <hex segmented> <bits score> <bits confidence_score> <max_rank> <edge_count> <bits supplement_score> <bits learning_score>

local function parse_args(argv)
    local opts = {}
    local i = 1
    while i <= #argv do
        local key = argv[i]:match("^%-%-([%w_]+)$")
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

emit("# decode transcript; model=" .. (opts.model and "fixture" or "off"))
for _, input in ipairs(selected) do
    sentence.reset_decode_cache()
    local results = sentence.decode(input, false)
    emit("decode", hex(input), "count=" .. #results,
        "learning=" .. (results.learning_affected and 1 or 0),
        "truncated=" .. (results._completed_truncated and 1 or 0))
    for _, item in ipairs(results) do
        emit("result", hex(item.text), hex(item.segmented), bits(item.score),
            bits(item.confidence_score), tostring(item.max_rank), tostring(item.edge_count),
            bits(item.supplement_score or 0), bits(item.learning_score or 0))
    end
end
out:close()
os.execute("rm -rf '" .. work .. "'")
print(string.format('{"lua":"%s","inputs":%d,"emitted":%d,"model":%s}',
    _VERSION, #selected, emitted, opts.model and "true" or "false"))
