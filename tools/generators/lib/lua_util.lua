-- SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
-- SPDX-License-Identifier: GPL-3.0-or-later

-- 金样生成/探针 Lua 脚本（tools/generators/gen_*.lua、tools/probes/bench_ngram.lua）
-- 共用的命令行与出库助手。用法：
--   local script_dir = (arg and arg[0] or ""):match("^(.*)[/\\]") or "."
--   package.path = script_dir .. "/lib/?.lua;" .. package.path   -- 探针改 ../generators/lib
--   local util = require("lua_util")
--
-- 约定：
--   * 选项一律 `--名 值`；名允许小写字母/数字/下划线/连字符（`^%-%-([%w_%-]+)$`）。
--   * 非选项参数与缺值的选项立即报错，不静默吞掉。
--   * 参照检出优先级：`--reference` > `HUX_REFERENCE_REPO` > `REFERENCE_REPO`（CI 用后者）
--     > `<仓库根>/_external/tiger-sentense-rime`（相对脚本位置解析，不依赖调用时的 cwd）。
--   * 空串/缺值统一编码为 `-`；文本按 UTF-8 字节转小写十六进制。

local util = {}

-- 解析 `--名 值` 参数对，返回 opts（defaults 提供缺省值，如 { mode = "fixture" }）。
function util.parse_args(argv, defaults)
    local opts = defaults or {}
    local i = 1
    while i <= #argv do
        local key = argv[i]:match("^%-%-([%w_%-]+)$")
        if not key then error("unexpected argument: " .. argv[i]) end
        local value = argv[i + 1]
        if not value then error("missing value for --" .. key) end
        opts[key] = value
        i = i + 2
    end
    return opts
end

-- 参照检出目录（优先级见文件头）。
function util.reference_dir(opts, script_dir)
    return opts.reference
        or os.getenv("HUX_REFERENCE_REPO")
        or os.getenv("REFERENCE_REPO")
        or (script_dir .. "/../../_external/tiger-sentense-rime")
end

-- 逐行 TSV 出库：返回 emit(...) 与累计行数读取器 count()（摘要里写 count()）。
function util.emitter(out)
    local emitted = 0
    return function(...)
        out:write(table.concat({ ... }, "\t"), "\n")
        emitted = emitted + 1
    end, function() return emitted end
end

-- 空串/缺值 → `-`，其余按 UTF-8 字节转小写十六进制。
function util.hex(text)
    if text == nil or text == "" then return "-" end
    return (text:gsub(".", function(c) return string.format("%02x", c:byte()) end))
end

-- 双精度浮点位模式：`0x` + 高 32 位 + 低 32 位。
function util.bits(value)
    local lo, hi = string.unpack("<I4I4", string.pack("<d", value))
    return string.format("0x%08x%08x", hi, lo)
end

-- 线性同余伪随机（种子参数化，同种子序列逐位不变）：返回 pick(limit) → 1..limit。
function util.lcg(seed)
    local state = seed
    return function(limit)
        state = (state * 1103515245 + 12345) % 2147483648
        return state % limit + 1
    end
end

return util
