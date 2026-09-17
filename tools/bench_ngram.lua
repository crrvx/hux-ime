-- 与 Rust 侧 `examples/ngram_bench.rs` 对齐的基准：加载模型后重放 transcript 中的
-- 全部 logp 查询，输出加载/查询耗时与结果位模式校验和（xor）。
--
--   lua tools/bench_ngram.lua --reference <repo> --model <bin> --transcript <tsv>

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
local reference = opts.reference or os.getenv("HUX_REFERENCE_REPO") or "../tiger-sentense-rime"
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
