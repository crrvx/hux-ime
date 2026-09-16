-- 生成 lexicon 差分金样 transcript（TSV），供 Rust 侧重放比对。
--
--   lua tools/gen_lexicon_golden.lua --reference <repo> --data <dir> --out <tsv> [--mode present|missing]
--
-- present 用 --data 目录中的四个数据文件；missing 模拟数据文件缺失（--data 传不存在目录）。
--
-- transcript 记录（tab 分隔，`#` 注释）：
--   status  <k=v>...                 data_status 规范快照（空串分隔）
--   lengths <n,n,...>                 lexicon_lengths
--   probe   <hex code> <items|->      lexicon_probe：hex(text):rank:optimal，逗号分隔
--   limit   <n>                       执行 apply_high_freq_limit
--   supp    count=<n> error=<0|1>     supplement_status（路径不入样）
-- 空串参数编码为 `-`；字符串为 UTF-8 字节的小写十六进制。

local function parse_args(argv)
    local opts = { mode = "present" }
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

package.path = reference .. "/lua/?.lua;" .. package.path
rime_api = { get_user_data_dir = function() return opts.data end }
local sentence = require("tiger_sentence")
sentence.set_model_enabled(false)

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

local function status_payload()
    local st = sentence.data_status()
    return table.concat({
        "built=" .. (st.built and 1 or 0),
        "high_freq_limit=" .. tostring(st.high_freq_limit),
        "codes_entries=" .. tostring(st.codes_entries),
        "codes_count=" .. tostring(st.codes_count),
        "ranks_count=" .. tostring(st.ranks_count),
        "whitelist_count=" .. tostring(st.whitelist_count),
        "isolation_enabled=" .. (st.isolation_enabled and 1 or 0),
        "errors=" .. tostring(#st.errors),
    }, " ")
end

local function emit_lengths()
    emit("lengths", table.concat(sentence.lexicon_lengths(), ","))
end

local function emit_supplement()
    local st = sentence.supplement_status()
    emit("supp", "count=" .. tostring(st.count or 0) .. " error=" .. (st.error and 1 or 0))
end

local function emit_probe(view, code)
    local entries = view.codes[code]
    if not entries then
        emit("probe", hex(code), "-")
        return
    end
    local parts = {}
    for _, entry in ipairs(entries) do
        parts[#parts + 1] = hex(entry.t) .. ":" .. entry.r .. ":" .. (entry.optimal_single and 1 or 0)
    end
    emit("probe", hex(code), table.concat(parts, ","))
end

local function sorted_codes(view)
    local codes = {}
    for code in pairs(view.codes) do codes[#codes + 1] = code end
    table.sort(codes)
    return codes
end

local mode = opts.mode
if mode == "present" then
    sentence.ensure_lexicon(nil)
elseif mode == "missing" then
    -- 空目录同样触发惰性装载；数据文件缺失走错误路径。
    sentence.ensure_lexicon(nil)
else
    error("unknown mode: " .. mode)
end

emit("# lexicon transcript (" .. mode .. ")")
emit("status", status_payload())
emit_lengths()
emit_supplement()

local view = sentence.lexicon_data_view()
local codes = sorted_codes(view)

if mode == "missing" then
    emit_probe(view, "aa")
    emit_probe(view, "vpa")
else
    for _, code in ipairs(codes) do emit_probe(view, code) end

    -- 关闭高频限制：全部放开非最优码（大数据集按序每 3 个抽样；小数据集全量复核）。
    emit("limit", "0")
    sentence.apply_high_freq_limit(0)
    emit("status", status_payload())
    emit_lengths()
    view = sentence.lexicon_data_view()
    codes = sorted_codes(view)
    local step = #codes <= 32 and 1 or 3
    for i = 1, #codes, step do emit_probe(view, codes[i]) end

    -- 恢复默认限制。
    emit("limit", "1500")
    sentence.apply_high_freq_limit(1500)
    emit("status", status_payload())
end

out:close()
print(string.format('{"mode":"%s","lua":"%s","codes":%d,"emitted":%d}', mode, _VERSION, #codes, emitted))
