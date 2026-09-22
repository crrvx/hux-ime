-- SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
-- SPDX-License-Identifier: GPL-3.0-or-later

-- 生成 ngram 差分金样 transcript（TSV），供 Rust 侧逐位重放比对。
--
-- 参照实现来自 tiger-sentense-rime 仓库（https://github.com/lvyww/tiger-sentense-rime）：
--   lua tools/generators/gen_ngram_golden.lua --reference <repo> --model <bin> --out <tsv> [--mode fixture|sample]
--
-- 模式：
--   fixture 生成 model_fixture.lua 的确定性小模型（写入 --model），并用独立
--           oracle 自检后写出金样。
--   sample  对真实模型抽样（前 128 个 unigram key 的二元组 + 2 万条随机三元组），
--          金样仅本地使用（不入库）。
--
-- transcript 记录（tab 分隔，`#` 为注释）：
--   bytes  <file_size>                     期望的模型字节数
--   logp   <hex a> <hex b> <hex c> <0x bits>  KN 概率的对数（f64 位模式，hi/lo）
--   obs    <hex a> <hex b> <0|1>            二元组是否有观测记录
--   status <k=v>...                         cache_status 规范快照
--   cfg    <page> <context> <bigram> <index>  执行 configure_cache
--   trim                                    执行 trim_caches
--   close                                   执行 close
-- 空串参数编码为 `-`；其余为 UTF-8 字节的小写十六进制。

local function parse_args(argv)
    local opts = { mode = "fixture" }
    local i = 1
    while i <= #argv do
        local key = argv[i]:match("^%-%-([%w_]+)$")
        if not key then error("unexpected argument: " .. argv[i]) end
        local value = argv[i + 1]
        if not value then error("missing value for --" .. key) end
        opts[key] = value
        i = i + 2
    end
    return opts
end

local opts = parse_args({ ... })
-- 默认参照检出：与仓库同级（相对脚本位置解析，不依赖调用时的 cwd）。
local script_dir = (arg and arg[0] or ""):match("^(.*)[/\\]") or "."
local reference = opts.reference or os.getenv("HUX_REFERENCE_REPO")
    or (script_dir .. "/../../_external/tiger-sentense-rime")
assert(opts.model, "missing --model")
assert(opts.out, "missing --out")

package.path = reference .. "/lua/?.lua;" .. package.path
local reader = require("tiger_sentence_ngram").new({ page_misses = 0, page_bytes = 0 })

local function hex(text)
    if text == "" then return "-" end
    return (text:gsub(".", function(c) return string.format("%02x", c:byte()) end))
end

local function bits(value)
    local lo, hi = string.unpack("<I4I4", string.pack("<d", value))
    return string.format("0x%08x%08x", hi, lo)
end

local out = assert(io.open(opts.out, "w"))
local emitted, checks = 0, 0

local function emit(...)
    out:write(table.concat({ ... }, "\t"), "\n")
    emitted = emitted + 1
end

local function check(ok, message)
    checks = checks + 1
    assert(ok, message)
end

local function emit_status(model)
    local st = model.cache_status()
    local order = {
        "page_bytes", "page_limit", "resident_index_bytes", "index_cache_bytes",
        "index_cache_limit", "index_misses", "index_bytes_read", "bigram_entries",
        "bigram_limit", "context_entries", "context_limit", "bigram_hits", "bigram_misses"
    }
    local parts = {}
    for _, key in ipairs(order) do parts[#parts + 1] = key .. "=" .. tostring(st[key]) end
    emit("status", table.concat(parts, "\t"))
end

local function should_emit(step, interval)
    return step % interval == 0
end

-- ---------------------------------------------------------------- fixture
local function run_fixture()
    local make = dofile(reference .. "/tools/model_fixture.lua")
    local oracle = make(opts.model)
    local model = reader.load(opts.model)
    check(model.bytes == oracle.bytes, "fixture size mismatch")

    local tokens = {}
    for _, cp in ipairs(oracle.tokens) do tokens[#tokens + 1] = utf8.char(cp) end
    tokens[#tokens + 1] = ""
    tokens[#tokens + 1] = ("不存在"):sub(1, 3)

    emit("# fixture transcript (model_fixture.lua)")
    emit("bytes", model.bytes)
    emit_status(model)

    -- 全三元组：logp 与独立 float32 oracle 逐位一致。
    for _, a in ipairs(tokens) do
        for _, b in ipairs(tokens) do
            for _, c in ipairs(tokens) do
                local value = model.logp(a, b, c)
                check(value == oracle.logp(a, b, c), "fixture logp mismatch")
                emit("logp", hex(a), hex(b), hex(c), bits(value))
            end
        end
    end
    -- 全二元组 observed-ness（零值记录 ≠ 缺失）。
    for _, a in ipairs(tokens) do
        for _, b in ipairs(tokens) do
            local observed = model.has_observed_bigram(a, b)
            check(observed == oracle.observed(a, b), "fixture observed mismatch")
            emit("obs", hex(a), hex(b), observed and 1 or 0)
        end
    end
    emit_status(model)

    -- 淘汰压力：10k 对未登录二元组 + 回访全部 token 对。
    for i = 1, 100 do
        for j = 1, 100 do
            local a, b = utf8.char(0x5000 + i), utf8.char(0x6000 + j)
            local observed = model.has_observed_bigram(a, b)
            check(observed == oracle.observed(a, b), "pair eviction mismatch")
            emit("obs", hex(a), hex(b), observed and 1 or 0)
        end
    end
    for _, a in ipairs(tokens) do
        for _, b in ipairs(tokens) do
            local value = model.logp("甲", a, b)
            check(value == oracle.logp("甲", a, b), "evicted pair probability mismatch")
            emit("logp", hex("甲"), hex(a), hex(b), bits(value))
        end
    end
    emit_status(model)

    -- 缓存上限重配置：小上限强制全缓存路径换血，再回访。
    model.configure_cache({ page_bytes = 65536, context_entries = 64, bigram_entries = 128, index_pages = 8 })
    emit("cfg", 65536, 64, 128, 8)
    emit_status(model)
    for _, a in ipairs(tokens) do
        for _, b in ipairs(tokens) do
            local value = model.logp(a, b, "甲")
            check(value == oracle.logp(a, b, "甲"), "configured cache probability mismatch")
            emit("logp", hex(a), hex(b), hex("甲"), bits(value))
        end
    end
    emit_status(model)
    model.trim_caches()
    emit("trim")
    emit_status(model)
    model.configure_cache({ page_bytes = 8388608, context_entries = 16384, bigram_entries = 8192, index_pages = 64 })
    emit("cfg", 8388608, 16384, 8192, 64)
    emit_status(model)
    model.close()
    emit("close")
    print(string.format('{"mode":"fixture","lua":"%s","emitted":%d,"oracle_checks":%d}',
        _VERSION, emitted, checks))
end

-- ----------------------------------------------------------------- sample
local function read_unigram_keys(path)
    local file = assert(io.open(path, "rb"))
    local header = assert(file:read(104), "truncated model")
    local _, _, _, _, _, uni_count, _, uni_off = string.unpack("<I4I4I8I4I4I4I4I8", header, 9)
    assert(file:seek("set", uni_off), "seek failed")
    local data = assert(file:read(uni_count * 8), "truncated unigrams")
    file:close()
    local keys = {}
    for position = 1, #data, 8 do
        local key = string.unpack("<i4", data, position)
        if key > 0 then keys[#keys + 1] = key end
    end
    return keys
end

local function run_sample()
    local keys = read_unigram_keys(opts.model)
    check(#keys > 128, "model has too few unigram keys")
    local step = math.max(1, math.floor(#keys / 128))
    local tokens = {}
    for i = 1, #keys, step do
        if #tokens >= 128 then break end
        tokens[#tokens + 1] = utf8.char(keys[i])
    end

    local model = reader.load(opts.model)
    emit("# sample transcript (local only)")
    emit("bytes", model.bytes)
    emit_status(model)

    for _, a in ipairs(tokens) do
        for _, b in ipairs(tokens) do
            emit("obs", hex(a), hex(b), model.has_observed_bigram(a, b) and 1 or 0)
        end
    end
    for _, a in ipairs(tokens) do
        for _, b in ipairs(tokens) do
            local value = model.logp(a, b, a)
            emit("logp", hex(a), hex(b), hex(a), bits(value))
        end
    end
    emit_status(model)

    -- 确定性伪随机三元组（LCG，种子固定）。
    local seed = 20260916
    local function next_index(limit)
        seed = (seed * 1103515245 + 12345) % 2147483648
        return seed % limit + 1
    end
    for _ = 1, 20000 do
        local a = tokens[next_index(#tokens)]
        local b = tokens[next_index(#tokens)]
        local c = tokens[next_index(#tokens)]
        emit("logp", hex(a), hex(b), hex(c), bits(model.logp(a, b, c)))
    end
    emit_status(model)
    for i = 1, 100 do
        for j = 1, 100 do
            local a, b = utf8.char(0x5000 + i), utf8.char(0x6000 + j)
            emit("obs", hex(a), hex(b), model.has_observed_bigram(a, b) and 1 or 0)
        end
    end
    emit_status(model)
    model.trim_caches()
    emit("trim")
    emit_status(model)
    model.close()
    emit("close")
    print(string.format('{"mode":"sample","lua":"%s","emitted":%d}', _VERSION, emitted))
end

local started = os.clock()
if opts.mode == "fixture" then
    run_fixture()
elseif opts.mode == "sample" then
    run_sample()
else
    error("unknown mode: " .. opts.mode)
end
out:close()
io.stderr:write(string.format("elapsed %.2fs\n", os.clock() - started))
