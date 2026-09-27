-- SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
-- SPDX-License-Identifier: GPL-3.0-or-later

-- 生成 lexical 金样（TCSLEX01 读取 / Bloom / 最大权词覆盖打分）。
--
--   lua tools/generators/gen_lexical_golden.lua --reference <repo> --model <bin> --out <tsv>
--
-- 注意：词先验模块自上游 main `35a10b9` 起提供；`--reference` 需指向该修订或更新。
--
-- transcript（tab 分隔，`#` 注释，`-` 表示空串，文本为 UTF-8 字节十六进制）：
--   header bytes=<n> entries=<n> bits=<n> hashes=<n> min=<n> max=<n>
--   hashes <hex text> <first> <second>
--   contains <hex text> <0/1>
--   score <hex text> <bit pattern>
-- 语料为确定性扫描/拼接（不依赖外部词表）：正例取自扫描命中，负例取自未命中，
-- 另含词长越界与长串打分。

-- 共享助手（parse_args / reference_dir / emitter / hex / bits / lcg）：见 lib/lua_util.lua 头注。
local script_dir = (arg and arg[0] or ""):match("^(.*)[/\\]") or "."
package.path = script_dir .. "/lib/?.lua;" .. package.path
local util = require("lua_util")
local opts = util.parse_args({ ... })
local reference = util.reference_dir(opts, script_dir)
assert(opts.model, "missing --model")
assert(opts.out, "missing --out")

package.path = reference .. "/lua/?.lua;" .. package.path
local lexical = require("tiger_sentence_lexical")

local model, load_error = lexical.load(opts.model)
assert(model, load_error or "cannot load lexical model")

local out = assert(io.open(opts.out, "w"))
local emit, emitted = util.emitter(out)
local hex, bits = util.hex, util.bits

emit("# lexical transcript; magic=" .. lexical.magic)
emit("header", "bytes=" .. model.bytes, "entries=" .. model.entry_count,
    "bits=" .. model.bit_count, "hashes=" .. model.hash_count,
    "min=" .. model.minimum_length, "max=" .. model.maximum_length)

for _, text in ipairs({ "", "a", "ab", "汉字", "虎句", string.rep("a", 30), "我们的" }) do
    local first, second = lexical.hashes(text)
    emit("hashes", hex(text), tostring(first), tostring(second))
end

-- 语料源：码表文本（位图的构建域：只有码表可编码的条目才可能命中）。
-- 默认取 `--reference` 仓库根目录的 `tiger_sentence.codes.txt`。
local codes_path = opts.codes or (reference .. "/tiger_sentence.codes.txt")
local codes_file = assert(io.open(codes_path, "rb"), "cannot read " .. codes_path)
local texts, seen = {}, {}
for line in codes_file:lines() do
    if line:sub(1, 1) ~= "#" then
        local text = line:match("^([^\t]+)\t")
        if text and not seen[text] then
            seen[text] = true
            texts[#texts + 1] = text
        end
    end
end
codes_file:close()

local positives, pool = {}, {}
for _, text in ipairs(texts) do
    local length = utf8.len(text) or 0
    if length == 1 then pool[#pool + 1] = text end
    if length >= 2 and length <= 4 and lexical.contains(model, text) then
        positives[#positives + 1] = text
    end
end
assert(#positives > 0, "code table has no positive entries; wrong model?")
assert(#pool >= 5, "code table lacks single characters")

-- 负例：单字池的确定性随机组合（码表词条几乎全部命中位图）。
local pick = util.lcg(12345)
local negatives = {}
local attempts = 0
while #negatives < 512 and attempts < 200000 do
    attempts = attempts + 1
    local length = 2 + pick(3) - 1
    local chars = {}
    for index = 1, length do chars[index] = pool[pick(#pool)] end
    local word = table.concat(chars)
    if not lexical.contains(model, word) then negatives[#negatives + 1] = word end
end
assert(#negatives >= 256, "not enough negative samples")

for _, word in ipairs(positives) do emit("contains", hex(word), 1) end
for _, word in ipairs(negatives) do emit("contains", hex(word), 0) end
-- 词长越界（< min 与 > max）必须为假。
emit("contains", hex(pool[1]), 0)
emit("contains", hex(pool[1] .. pool[2] .. pool[3] .. pool[4] .. pool[5]), 0)

-- 打分语料：正/负例拼接的长串与随机码表文本。
local scores = {}
for index = 1, 64 do scores[#scores + 1] = texts[1 + (index * 137) % #texts] end
for index = 1, 32 do
    local parts = {}
    for part = 1, 1 + (index % 4) do parts[part] = positives[1 + (index * 31 + part * 7) % #positives] end
    scores[#scores + 1] = table.concat(parts)
end
for index = 1, 32 do
    local parts = {}
    for part = 1, 1 + (index % 3) do parts[part] = negatives[1 + (index * 17 + part * 5) % #negatives] end
    scores[#scores + 1] = table.concat(parts)
end
for _, text in ipairs(scores) do
    emit("score", hex(text), bits(lexical.score(model, text, {})))
end

out:close()
print(string.format('{"lua":"%s","texts":%d,"positives":%d,"negatives":%d,"emitted":%d}',
    _VERSION, #texts, #positives, #negatives, emitted()))
