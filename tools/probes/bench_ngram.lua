-- SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
-- SPDX-License-Identifier: GPL-3.0-or-later

-- 与 Rust 侧 `examples/ngram_bench.rs` 对齐的基准：加载模型后重放 transcript 中的
-- 全部 logp 查询，输出加载/查询耗时与结果位模式校验和（xor）。
--
--   lua tools/probes/bench_ngram.lua --reference <repo> --model <bin> --transcript <tsv>

-- 共享助手（parse_args / reference_dir）：见 tools/generators/lib/lua_util.lua 头注。
local script_dir = (arg and arg[0] or ""):match("^(.*)[/\\]") or "."
package.path = script_dir .. "/../generators/lib/?.lua;" .. package.path
local util = require("lua_util")
local opts = util.parse_args({ ... })
local reference = util.reference_dir(opts, script_dir)
assert(opts.model and opts.transcript, "missing --model or --transcript")

package.path = reference .. "/lua/?.lua;" .. package.path
local reader = require("tiger_sentence_ngram").new({ page_misses = 0, page_bytes = 0 })

local function unhex(text)
    if text == "-" then return "" end
    return (text:gsub("%x%x", function(pair) return string.char(tonumber(pair, 16)) end))
end

local started = os.clock()
local model = reader.load(opts.model)
local loaded = os.clock()

local queries, checksum = 0, 0
for line in io.lines(opts.transcript) do
    local a, b, c = line:match("^logp\t(%S+)\t(%S+)\t(%S+)\t")
    if a then
        local value = model.logp(unhex(a), unhex(b), unhex(c))
        local bits = string.unpack("<I8", string.pack("<d", value))
        checksum = checksum ~ bits
        queries = queries + 1
    end
end
local finished = os.clock()
print(string.format(
    '{"load_ms":%.1f,"query_ms":%.1f,"queries":%d,"checksum":"0x%016x"}',
    (loaded - started) * 1000, (finished - loaded) * 1000, queries, checksum))
