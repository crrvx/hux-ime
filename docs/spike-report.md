# K0 Spike 报告

> 2026-09-16 ｜ 结论：**通过**——按 K1–K4 推进
> 关联：[`rust-migration.md`](rust-migration.md)

## 1. 验证结果

### 1.1 fixture 全量（入库金样）

- 记录构成：29,616 条（另有 1 行注释头，共 29,617 行）= `logp` 18,928 + `obs` 10,676 + `status` 7 + `cfg` 2 + `trim`/`close`/`bytes` 各 1
- 生成器内以独立 float32 oracle 自检 29,605 项，全部通过
- Rust 重放（`cargo test -p hux-core`）：**逐位一致**

### 1.2 真实模型抽样（本地金样，不入库）

- 模型：`sentence-ngram-mobile.bin`（224 MB）
- 记录：62,776 条（另有 1 行注释头，共 62,777 行）= `logp` 36,384 + `obs` 26,384 + `status` 5 + `trim`/`close`/`bytes` 各 1
- Rust 重放：**逐位一致**；Lua 参照侧生成耗时 3.9 s

### 1.3 缓存语义

`cache_status` 快照（页字节、索引缓存字节/未命中、上下文条目、FIFO `#keys` 语义、命中/未命中计数）在
`configure_cache` 重配置与 `trim_caches` 前后全部一致 —— FIFO 淘汰序、LRU 页淘汰、
列式槽位复用与参照实现一致。

### 1.4 位级一致

`logp` 以 f64 位模式（hi/lo u32）比较；fixture 与真实模型的全部查询一致，
说明运算顺序与系统 libm 行为均已对齐。

## 2. 性能（同一转录重放 36,384 次 `logp`，真实模型）

| 实现 | 加载 | 查询 | 结果校验和 |
|---|---:|---:|---|
| Lua（参照） | 2.3 ms | 1162.9 ms | `0x7fe3970ae1d51e3c` |
| Rust（release，mmap） | 0.6 ms | 14.2 ms | `0x7fe3970ae1d51e3c` |

- 查询吞吐约 **82×**；加载约 4×（校验和一致，性能提升不是以行为偏差换来的）
- 说明：该查询分布含大量缓存命中，**不代表端到端解码收益**；K1 完成后按真实按键负载复测
- 内存：本次环境无 GNU time，未测 RSS；双方缓存字节与条目数已由 `cache_status` 对齐；
  Rust 使用 mmap，不整读 224 MB

## 3. 语义陷阱审计（K1 输入）

| 类别 | 位置（参照实现） | 结论 / 对策 |
|---|---|---|
| `pairs()` 遍历序 | 主模块 36 处、learning 19 处、ngram 3 处 | 多数为独立聚合（按 key 写入、`math.max`、计数）；lexicon 建索引（281/287/310/327/358）核对为顺序无关；Aho–Corasick 构建（533/558/565）**节点编号**受序影响但匹配结果无关——K1 显式排序并在差分中固化 |
| `table.sort` 非全序 | 主模块 1523/1553/1562（beam 排序） | Lua 排序不稳定，等值项次序未定义；K1 增加确定性 tie-breaker，差分快照兜底 |
| 整数 / 格式化 | learning 57（`%08x%08x`）、405（`e/%010d`）、各处 `tostring` | Rust 逐字节复刻格式化结果 |
| 时间依赖 | learning `os.time`（事件时间、衰减水位） | 注入 `now`（参照测试已支持），杜绝时钟抖动 |
| 字节串语义 | 全模块 | K0 已按字节实现（`scalar`、hex 编解码、字节索引） |
| libm 一致性 | `math.log` | 已实证与本机 Rust `f64::ln` 位级一致（62k 查询） |
| 正则解析 | 锁串 `(%d+),(%d+);` 等 | 手写解析，逐字节一致 |

## 4. 结论与 K1 纪律

**Spike 通过**：差分工具链可用、陷阱密度可控（且大多可提前显式化）、性能收益显著。
K1 起把三条纪律固化：

1. 凡排序必带全序 tie-breaker；
2. 凡 `pairs` 影响可观测结果处，显式排序或改用确定性结构；
3. 凡时间/随机，全部注入，不做隐式系统调用。

工期修正（业余节奏）：K1 计算核 1–2 周；K2 交互引擎 2–4 周；K3 fcitx5 集成 2–3 周；K4 验收打包 1 周。

## 5. 复跑

```sh
cargo test -p hux-core

# 基准（真实模型 + 本地抽样金样）
cargo run --release -q --example ngram_bench -- \
  ~/.local/share/fcitx5/rime/models/sentence-ngram-mobile.bin goldens/local/ngram_sample.tsv
lua tools/bench_ngram.lua --reference ../tiger-sentense-rime \
  --model ~/.local/share/fcitx5/rime/models/sentence-ngram-mobile.bin \
  --transcript goldens/local/ngram_sample.tsv
```

## 6. 收尾（自审）

- 补齐 `LICENSE`（GPL-3.0）与 `README.md`；
- CI（`.github/workflows/ci.yml`）：Rust fmt/clippy/差分 + 以固定参照提交重生成 fixture 金样比对（溯源校验）；
- 金样来源与 sha256 记入 `goldens/README.md`（K0 时期参照提交 `f3b30498`；其后 pin 多次前移，当前值以该文档为准）；
- `ngram.rs` 边界修复：非 TCSKNM02 模型明确报错、空 unigram 段返回错误而非 panic，并补单测；
- 模型路径探测（`try_load`/`candidate_paths`）随 K3；`external/` 加入 `.gitignore`。
