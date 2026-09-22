<!-- SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com> -->
<!-- SPDX-License-Identifier: GPL-3.0-or-later -->

# 性能（P6）

纪律（`docs/refactor.md` §6）：**优化只允许「金样不变」的改动，且须有前后对比数据**。
故 P6 先建基准、留下基线，"按需"优化。

## 基准

本阶段新增两个 `--release` 示例（另有既存 `ngram_bench.rs`，见 `goldens/README.md` 的「校验」节）；
不引入新依赖，故不建 `hux-bench` crate（与 `docs/refactor.md` §6 的取舍一致）：

```sh
# decode 冷路径：重放 goldens/decode.tsv.gz 的 847 条输入（与差分测试同一批语料）
cargo run --release --example decode_bench
cargo run --release --example decode_bench -- --model goldens/ngram_fixture.bin \
    --lexical data/tiger_sentence.lexical.bin

# 整键路径：经方案契约驱动会话，测「process_key + rebuild」单键耗时
cargo run --release --example key_bench
cargo run --release --example key_bench -- --model goldens/ngram_fixture.bin
```

参数：`decode_bench` 支持 `--model <bin>` / `--lexical <bin>` / `--repeat N`；
`key_bench` 支持 `--codes N`（送入的码条数）/ `--repeat N` / `--model <bin>`。

输出：`decode_bench` 打一行 JSON（`corpus`/`repeat`/`ops`/`mean_us`/`p50_us`/`p95_us`/`max_us`/`checksum`）
再打按输入长度分桶的 5 行文本；`key_bench` 打一行 JSON（`codes`/`repeat`/`keys`/`model`/`mean_us`/…，无 `checksum`）。
`checksum`（`decode_bench`）用于确认测量期间计算真的发生了且结果稳定。

## 基线（2026-09-21，开发机 Arch + release，`--repeat 20` / `key_bench --repeat 10`）

| 场景 | p50 | p95 | max | 说明 |
| --- | --- | --- | --- | --- |
| decode 冷路径（无模型） | **1.00 µs** | 2.29 µs | 4779 µs | 16,940 次 |
| decode 冷路径（fixture 模型） | 1.06 µs | 2.56 µs | 2605 µs | 16,940 次 |
| decode 冷路径（+ 真实词先验位图） | 1.08 µs | 2.58 µs | 2671 µs | 16,940 次 |
| 整键路径（无模型） | **2.41 µs** | 181 µs | 2670 µs | 4,990 次 |
| 整键路径（fixture 模型） | 2.71 µs | 204 µs | 2871 µs | 4,990 次 |

按输入长度分桶（decode 冷路径，无模型）：

| 输入长度 | 样本 | p50 | p95 | max |
| --- | --- | --- | --- | --- |
| 1–2 字符 | 14,080 | 0.96 µs | 1.69 µs | 786 µs |
| 3–5 字符 | 2,820 | 1.63 µs | 3.38 µs | 10.3 µs |
| > 20 字符 | 40 | **2278 µs** | 2355 µs | 4779 µs |

## 结论（P6 决定）

1. **打字路径已经是微秒级**：1–5 字符（占语料 99.8%）单次 decode ≤3.4 µs（p95），
   整键（处理器 + 宿主链 + 重建）p50 约 2.4 µs。相对键盘输入间隔（数十毫秒）可忽略，
   **不构成优化理由**。
2. **全部尾部代价来自 >20 字符的长整句**（p50 ≈ 2.3 ms）：这是 beam 解码在长输入上的
   固有工作量，仍远低于交互预算（~10 ms），且该形态（超长整句）本就少见。
3. 因此 P6 **不做** 参照实现的「增量 / 锁解码缓存」：收益集中在长输入路径，
   而风险在于该缓存需与解码 arena 的路径下标生命周期绑定（见 `decode.rs`
   `Evaluated::path` 注释："仅对产生它的那次 `decode*` 返回值有效"），
   属于"改动语义边界"的一类优化，不符合"金样不变 + 按需"的前提。
4. **复核触发条件**：若将来出现下列任一情形，再做该项缓存并用本文基准给前后数据——
   ① Android 中低端机实测长整句出现可感卡顿；② 输入长度上限（`MAX_RAW_LENGTH`）放宽；
   ③ 模型从 mobile 换成更大模型。

## 维护约定

- 改动 decode / 交互路径后，跑一次上面四条命令并把数字与基线比对；**checksum 变化即为行为变化**，
  必须查清（差分金样也应当同时报警）。
- 新基准沿用 JSON 单行输出，便于脚本对拍；不引入 `criterion` 等新依赖（保持离线可构建）。
