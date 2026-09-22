-- SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
-- SPDX-License-Identifier: GPL-3.0-or-later

-- 生成 learning 金样（纯计算；持久化用内存 mock，不触碰系统 LevelDb）。
--
--   lua tools/generators/gen_learning_golden.lua --reference <repo> --out <tsv>
--
-- transcript（tab 分隔，`#` 注释，`-` 表示空串）：
--   hash <hex text> <value>
--   frame <hex p1> ... <hex pn-1> <hex out>
--   unframe <hex in> <hex p1> ... |bad
--   corpus <name> <n>            后接 n 条 event
--   event <time> <hex mode> <hex code> <hex text> <hex ctx>
--   index <name> <kind:full|runtime> <now> <corpus>
--   confirmed <index> <base-corpus> <accepted-corpus> <now>    # 经 M.open/M.confirm 的更新路径
--   codes <index> <n> <hex code>...
--   score / prefix / update / trim / chain / node / reward / maturity / contribution
--   diffcase / diffpath / diff / diffevent
--   fusionmode <hex mode> <hex out>                      # M.fusion_mode
--   paircode <hex raw> <hex direct> <hex composed> <hex code>   # M.fusion_pair_code
--   fusion <index> <hex mode> <hex raw> <hex direct> <hex composed> <bits score>
--   fusionnone <hex mode> <hex raw> <hex direct> <hex composed> <0|1> <raw_end>
--   fusionevent <hex mode> <hex raw> <hex direct> <hex composed> <0|1> <raw_end> <time>
--               <hex out-mode> <hex code> <hex text> <hex ctx> <raw_start> <text_start> <text_end>

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
    or (script_dir .. "/../../_external/tiger-sentense-rime")
assert(opts.out, "missing --out")

package.path = reference .. "/lua/?.lua;" .. package.path

-- 内存 LevelDb mock：open/close/query/update，行为参照测试桩。
local databases = {}
LevelDb = function(name)
    local data = databases[name] or {}
    databases[name] = data
    return {
        open = function() return true end,
        close = function() end,
        query = function()
            return { iter = function()
                local keys, n = {}, 0
                for k in pairs(data) do keys[#keys + 1] = k end
                table.sort(keys)
                return function()
                    n = n + 1
                    local k = keys[n]
                    if k then return k, data[k] end
                end
            end }
        end,
        update = function(_, k, v) data[k] = v; return true end,
    }
end

local learning = require("tiger_sentence_learning")
local NOW = 40 * 86400
local real_time = os.time
os.time = function() return NOW end

local out = assert(io.open(opts.out, "w"))
local emitted = 0
local function emit(...)
    out:write(table.concat({ ... }, "\t"), "\n")
    emitted = emitted + 1
end
local function hex(text)
    if text == nil or text == "" then return "-" end
    return (text:gsub(".", function(c) return string.format("%02x", c:byte()) end))
end
local function bits(value)
    local lo, hi = string.unpack("<I4I4", string.pack("<d", value))
    return string.format("0x%08x%08x", hi, lo)
end

-- ---------------------------------------------------------------- 工具面
for _, text in ipairs({ "", "tiger_sentence", "虎句", "a", "schema/虎" }) do
    emit("hash", hex(text), learning.hash(text))
end
-- ---------------------------------------------------------------- 语料
local function ev(time, mode, code, text, ctx)
    return { time = time, mode = mode, code = code, text = text, context = ctx }
end
local corpora = {
    base = {
        ev(1000, "m1", "a", "甲", ""),
        ev(1000, "m1", "a", "乙", ""),
        ev(2000, "m1", "a", "甲", ""),
        ev(2600, "m1", "ab", "甲乙", ""),
        ev(2600, "m1", "ab", "甲乙", "甲"),
        ev(3000, "m2", "ab", "甲乙", ""),
        ev(3000, "m2", "ab", "甲乙", "乙"),
        ev(3600, "m2", "abc", "甲乙丙", ""),
        ev(3600, "m3", "b", "丙", "甲"),
        ev(4000, "m3", "b", "丙", "甲乙"),
        ev(5000, "m4", "b", "丁", ""),
        ev(-5, "m5", "a", "戊", ""),
        ev(6000, "", "a", "己", ""),
        ev(6000, "m6", "", "己", ""),
        ev(6000, "m7", "a", "", ""),
        ev(6000, "m8", "a", "庚", "甲乙丙"),
        ev(7000, "m9", "b", "辛", ""),
        ev(9000, "m9", "b", "辛", ""),
    },
    accepted = {
        ev(8000, "m1", "a", "甲", ""),
        ev(8000, "m5", "z", "壬", ""),
    },
}
-- 人工纠错等级（7b220ce）：同一 (code, mode, context) 连记 12 次 ⇒ 等级在第 10 级封顶
-- （同上下文 27 / 跨上下文 24，而非 12 级的 31 / 28）；另一上下文组只有 1 次 ⇒ L1（9 / 6），
-- 用于反证「等级按 (mode, context) 组独立累计」。
corpora.levels = {}
for i = 1, 12 do
    corpora.levels[#corpora.levels + 1] = ev(1000 + i, "m1", "lv", "甲", "")
end
corpora.levels[#corpora.levels + 1] = ev(2000, "m1", "lv", "乙", "乙")
local function sorted_keys(table_)
    local keys = {}
    for key in pairs(table_) do keys[#keys + 1] = key end
    table.sort(keys)
    return keys
end
for _, name in ipairs(sorted_keys(corpora)) do
    local events = corpora[name]
    emit("corpus", name, tostring(#events))
    for _, e in ipairs(events) do
        emit("event", tostring(e.time), hex(e.mode), hex(e.code), hex(e.text), hex(e.context))
    end
end

local indexes = {}
local function emit_codes(name, index)
    local fields = { "codes", name, tostring(#index.codes) }
    for _, code in ipairs(index.codes) do fields[#fields + 1] = hex(code) end
    emit(table.concat(fields, "\t"))
end
local function build_named(name, corpus_name, events, kind, now)
    local index
    if kind == "full" then
        index = learning.build(events, now)
    else
        index = learning.runtime_index(events, now)
    end
    indexes[name] = index
    emit("index", name, kind, tostring(now), corpus_name)
    emit_codes(name, index)
end
build_named("full_base", "base", corpora.base, "full", NOW)
build_named("runtime_base", "base", corpora.base, "runtime", NOW)

-- 等级语义：full/runtime 两条路径 + 「时间推后 10 年分值不变」（无衰减）。
build_named("levels_full", "levels", corpora.levels, "full", NOW)
build_named("levels_runtime", "levels", corpora.levels, "runtime", NOW)
build_named("levels_aged", "levels", corpora.levels, "full", NOW + 3650 * 86400)
for _, name in ipairs({ "levels_full", "levels_runtime", "levels_aged" }) do
    local index = indexes[name]
    for _, text in ipairs({ "甲", "乙" }) do
        for _, ctx in ipairs({ "", "乙", "丙" }) do
            emit("score", name, hex("m1"), hex("lv"), hex(text), hex(ctx),
                bits(learning.score(index, "m1", "lv", text, ctx)))
            emit("prefix", name, hex("m1"), hex("lv"), hex(text), hex(ctx),
                bits(learning.prefix_score(index, "m1", "lv", text, ctx)))
        end
    end
end

local query_codes = { "a", "ab", "abc", "b", "z", "abcd", "" }
local query_modes = { "m1", "m2", "m3", "m9", "" }
local query_texts = { "甲", "乙", "甲乙", "甲乙丙", "辛", "" }
local query_ctxs = { "", "甲", "乙", "丙" }
local function emit_queries(index_name)
    local index = indexes[index_name]
    for _, code in ipairs(query_codes) do
        for _, mode in ipairs(query_modes) do
            for _, text in ipairs(query_texts) do
                for _, ctx in ipairs(query_ctxs) do
                    emit("score", index_name, hex(mode), hex(code), hex(text), hex(ctx),
                        bits(learning.score(index, mode, code, text, ctx)))
                    emit("prefix", index_name, hex(mode), hex(code), hex(text), hex(ctx),
                        bits(learning.prefix_score(index, mode, code, text, ctx)))
                end
            end
        end
    end
end
emit_queries("full_base")
emit_queries("runtime_base")

-- 更新路径（M.open/M.confirm；os.time 固定保证确定性）。
local function confirmed_index(name, base_events, accepted_events, now)
    os.time = function() return now end
    local store = learning.open(name)
    if not learning.confirm(store, base_events) then error("confirm base failed: " .. name) end
    if not learning.confirm(store, accepted_events) then error("confirm accepted failed: " .. name) end
    indexes[name] = store.index
    emit("confirmed", name, "base", "accepted", tostring(now), tostring(now))
    emit_codes(name, store.index)
end
confirmed_index("runtime_confirmed", corpora.base, corpora.accepted, NOW)

-- 时钟回退：confirm 的 now 小于索引 now → 参照触发全量重放。
os.time = function() return NOW end
local rollback_store = learning.open("rollback")
learning.confirm(rollback_store, corpora.base)
os.time = function() return NOW - 1000 end
learning.confirm(rollback_store, corpora.accepted)
os.time = function() return NOW end
indexes.runtime_rollback = rollback_store.index
emit("confirmed", "runtime_rollback", "base", "accepted", tostring(NOW), tostring(NOW - 1000))
emit_codes("runtime_rollback", rollback_store.index)

-- 未来时间：索引 now 早于事件时间 → confirm 触发全量重放。
os.time = function() return 5000 end
local future_store = learning.open("future")
learning.confirm(future_store, corpora.base)
os.time = function() return 6000 end
learning.confirm(future_store, corpora.accepted)
os.time = function() return NOW end
indexes.runtime_future = future_store.index
emit("confirmed", "runtime_future", "base", "accepted", tostring(5000), tostring(6000))
emit_codes("runtime_future", future_store.index)

emit_queries("runtime_confirmed")
emit_queries("runtime_rollback")
emit_queries("runtime_future")

-- frame/unframe 端到端：confirm 写入 → open 读回（编码键值一并记录）。
do
    os.time = function() return NOW end
    local source = learning.open("journal_src")
    learning.confirm(source, corpora.accepted)
    local written = databases["journal_src"]
    databases["journal_dst"] = {}
    for k, v in pairs(written) do databases["journal_dst"][k] = v end
    local destination = learning.open("journal_dst")
    local keys = {}
    for k in pairs(written) do keys[#keys + 1] = k end
    table.sort(keys)
    emit("journalrecords", tostring(#keys))
    for _, k in ipairs(keys) do
        emit("journalrecord", hex(k), hex(written[k]))
    end
    emit("journalevents", tostring(#destination.events))
    for _, e in ipairs(destination.events) do
        emit("journalevent", tostring(e.time), hex(e.mode), hex(e.code), hex(e.text), hex(e.context))
    end
end

emit("trim", "runtime_base")
emit_queries("runtime_base")

-- ---------------------------------------------------------------- 奖励链
local chains = {
    root = { text = "", text_length = 0, raw_length = 0, learning_score = 0 },
    one = { text = "甲乙", text_length = 6, raw_length = 4, learning_score = 2.5,
        learning_early_commit_bonus = 0.2,
        previous = { text = "甲", text_length = 3, raw_length = 2, learning_score = 1.0,
            learning_early_commit_bonus = 0.1,
            previous = { text = "", text_length = 0, raw_length = 0, learning_score = 0,
                learning_early_commit_bonus = 0.0 } } },
    compat = { text = "甲乙", text_length = 6, raw_length = 4, learning_score = 0.5,
        previous = { text = "甲", text_length = 99, raw_length = 2, learning_score = 0.25 } },
    sleep = { text = "甲乙丙", text_length = 9, raw_length = 6, learning_score = 0,
        learning_early_commit_bonus = 0.5,
        previous = { text = "甲乙", text_length = 6, raw_length = 4, learning_score = 0,
            previous = { text = "甲", text_length = 3, raw_length = 2, learning_score = 0 } } },
}
local function emit_chain(name, node)
    local nodes = {}
    while node do
        nodes[#nodes + 1] = node
        node = node.previous
    end
    emit("chain", name, tostring(#nodes))
    for _, item in ipairs(nodes) do
        emit("node", tostring(item.text_length), tostring(item.raw_length),
            bits(item.learning_score),
            bits(item.learning_early_commit_bonus or 0), hex(item.text))
    end
end
for _, name in ipairs(sorted_keys(chains)) do emit_chain(name, chains[name]) end

local reward_cases = {
    { "full_base", "one", "m1", "ab", "甲乙", 2 },
    { "full_base", "compat", "m1", "ab", "甲乙", 2 },
    { "full_base", "sleep", "m2", "abc", "甲乙丙", 4 },
    { "full_base", "root", "m2", "ab", "甲乙", 2 },
    { "full_base", "one", "", "ab", "甲乙", 2 },
    { "runtime_base", "one", "m1", "ab", "甲乙", 2 },
    { "runtime_confirmed", "sleep", "m2", "abc", "甲乙丙", 4 },
    -- 奖励分 > 9（首次稳定观测）：触发 early_commit_contribution 的正贡献，
    -- 并与种子节点的 learning_early_commit_bonus 取较大者。
    { "runtime_future", "root", "m1", "a", "甲", 1 },
    { "runtime_future", "one", "m9", "b", "辛", 1 },
}
for _, case in ipairs(reward_cases) do
    local index = indexes[case[1]]
    local best, potential, early_bonus =
        learning.reward(index, case[3], case[4], case[5], case[6], chains[case[2]])
    emit("reward", case[1], case[2], hex(case[3]), hex(case[4]), hex(case[5]), tostring(case[6]),
        bits(best), bits(potential), bits(early_bonus))
end

-- 早提交成熟度 / 单条奖励贡献（5ce1ca2 新增的纯计算）。
for _, score in ipairs({ 0, 8, 9, 9 + 2 * math.log(2), 9 + 2 * math.log(3), 9.5, 10,
    10.39, 11, 11.2, 12, 13, 20, 24, 27 }) do
    emit("maturity", bits(score), bits(learning.early_commit_maturity(score)))
    emit("contribution", bits(score), bits(learning.early_commit_contribution(score)))
end

-- ---------------------------------------------------------------- diff
local diffcases = {
    before1 = { text = "甲乙", path = { { 4, 6 }, { 2, 3 } } },
    selected1 = { text = "甲丙", path = { { 4, 6 }, { 2, 3 } } },
    before2 = { text = "甲乙", path = { { 4, 6 } } },
    selected2 = { text = "甲丙", path = { { 4, 6 } } },
    bad = { text = "甲乙", path = { { 6, 6 } } },
}
local function build_path(entries)
    local node = nil
    for i = #entries, 1, -1 do
        node = { raw_length = entries[i][1], text_length = entries[i][2], previous = node }
    end
    return node
end
for _, name in ipairs(sorted_keys(diffcases)) do
    local case = diffcases[name]
    emit("diffcase", name, hex(case.text), tostring(#case.path))
    for _, entry in ipairs(case.path) do
        emit("diffpath", tostring(entry[1]), tostring(entry[2]))
    end
    case.node = build_path(case.path)
end
local diff_runs = {
    { "abCD", 0, "m", "before1", "selected1" },
    { "abCD", 0, "m", "before2", "selected2" },
    { "abCD", 2, "m", "before1", "selected1" },
    { "abCD", 0, "m", "before1", "bad" },
    { "abCD", 0, "m", "before2", "before2" },
}
for _, run in ipairs(diff_runs) do
    local raw, floor, mode = run[1], run[2], run[3]
    local before = { text = diffcases[run[4]].text, path = diffcases[run[4]].node }
    local selected = { text = diffcases[run[5]].text, path = diffcases[run[5]].node }
    local events = learning.diff(raw, before, selected, floor, mode)
    emit("diff", hex(raw), tostring(floor), hex(mode), run[4], run[5], tostring(#events))
    for _, e in ipairs(events) do
        emit("diffevent", hex(e.text), hex(e.code), hex(e.context),
            tostring(e.raw_start), tostring(e.raw_end), tostring(e.text_start), tostring(e.text_end))
    end
end

-- ---------------------------------------------------------------- 融合偏好（24e633e / 59fc87a）
for _, mode in ipairs({ "", "m1", "sentence-v2|rules=|optimal=1500|dup=1" }) do
    emit("fusionmode", hex(mode), hex(learning.fusion_mode(mode)))
end
for _, pair in ipairs({
    { "ii", "C", "A" },
    { "", "", "" },
    { "ab", "疒否", "交否" },
    { "abab", "交交", "交疒" },
}) do
    emit("paircode", hex(pair[1]), hex(pair[2]), hex(pair[3]),
        hex(learning.fusion_pair_code(pair[1], pair[2], pair[3])))
end

-- 融合语料：同一 `(raw, direct, composed)` 三元组的 D/C 竞争 + 未记录的 pair。
local FUSION_TIME = 1700000000
local fusion_specs = {
    { "m1", "ii", "C", "A", true },
    { "m1", "ii", "C", "A", true },
    { "m1", "ii", "B", "A", true },
    { "m1", "ii", "D", "E", false },
}
os.time = function() return FUSION_TIME end
local fusion_events = {}
for _, spec in ipairs(fusion_specs) do
    local e = learning.fusion_event(spec[1], spec[2], spec[3], spec[4], spec[5], #spec[2])
    assert(e, "fusion_event 不应返回 nil")
    fusion_events[#fusion_events + 1] = e
end
-- 编码事件（落库只带五元组）与事件构造分开验证。
local fusion_corpus = {}
for _, e in ipairs(fusion_events) do
    fusion_corpus[#fusion_corpus + 1] =
        ev(e.time, e.mode, e.code, e.text, e.context)
end
corpora.fusion = fusion_corpus
os.time = function() return NOW end

emit("corpus", "fusion", tostring(#fusion_corpus))
for _, e in ipairs(fusion_corpus) do
    emit("event", tostring(e.time), hex(e.mode), hex(e.code), hex(e.text), hex(e.context))
end
build_named("fusion_base", "fusion", fusion_corpus, "full", NOW)
build_named("fusion_runtime", "fusion", fusion_corpus, "runtime", NOW)
for _, name in ipairs({ "fusion_base", "fusion_runtime" }) do
    for _, spec in ipairs({
        { "m1", "ii", "C", "A" },
        { "m1", "ii", "B", "A" },
        { "m1", "ii", "A", "C" },
        { "m1", "ii", "D", "E" },
        { "m1", "ii", "E", "D" },
        { "m1", "ii", "Z", "A" },
        { "", "ii", "C", "A" },
        { "m1", "", "C", "A" },
    }) do
        emit("fusion", name, hex(spec[1]), hex(spec[2]), hex(spec[3]), hex(spec[4]),
            bits(learning.fusion_score(indexes[name], spec[1], spec[2], spec[3], spec[4])))
    end
end

local fusion_runs = {
    { "m1", "ii", "C", "A", true, 2 },
    { "m1", "ii", "B", "A", false, 2 },
    { "m1", "abab", "交交", "交疒", true, 4 },
    { "m1", "", "", "", true, 0 },
    { "", "ii", "C", "A", true, 2 },
}
for _, run in ipairs(fusion_runs) do
    local mode, raw, direct, composed, wins, raw_end = run[1], run[2], run[3], run[4], run[5], run[6]
    os.time = function() return FUSION_TIME end
    local e = learning.fusion_event(mode, raw, direct, composed, wins, raw_end)
    os.time = function() return NOW end
    if e then
        emit("fusionevent", hex(mode), hex(raw), hex(direct), hex(composed),
            wins and 1 or 0, tostring(raw_end), tostring(FUSION_TIME),
            hex(e.mode), hex(e.code), hex(e.text), hex(e.context),
            tostring(e.raw_start), tostring(e.text_start), tostring(e.text_end))
    else
        emit("fusionnone", hex(mode), hex(raw), hex(direct), hex(composed),
            wins and 1 or 0, tostring(raw_end))
    end
end

out:close()
os.time = real_time
print(string.format('{"lua":"%s","emitted":%d}', _VERSION, emitted))
