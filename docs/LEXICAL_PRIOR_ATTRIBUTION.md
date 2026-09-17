# 紧凑词先验：来源与署名

`data/tiger_sentence.lexical.bin`（TCSLEX01 Bloom filter，150,032 字节，
sha256 `8dbc884b6cb719d07e4cef153c8048db19a11f8224f75a4ed87853e688a27393`）
是「紧凑排序先验」所用的词存在性位图，**不含词文本与权重**。

## 来源

派生自 [`fcxxxz/rime-mohu`](https://github.com/fcxxxz/rime-mohu) 的
`mohu_flypy.base.dict.yaml`：

- 上游版本：`9f43098cefdb450fe8dec0f3069fe8d9999b9d10`
- 原文件 SHA-256：`877c6dacb4d5bb6738e230ce2d9235f3ac0f48404959c2db26fb18c7ddd31cb6`
- 原文件声明许可：**CC BY 4.0**
- 作者/贡献者：rime-mohu contributors
- 原文件列明的数据来源：Rime 八股文词库、THUOCL（依其原许可再发行）、
  雾凇拼音词库补充数据及人工补充词
- 原项目说明：完整方案按 GPL-3.0 发布；文件另有声明时以文件声明为准
- 许可文本：**CC BY 4.0**，正文见 [`LICENSES/CC-BY-4.0.txt`](../LICENSES/CC-BY-4.0.txt)
  （SPDX 官方镜像副本）；亦可访问 <https://creativecommons.org/licenses/by/4.0/>

## 变更说明

上游 `tiger-sentense-rime` 所作的变更：只保留虎句码表可编码的 2～4 字条目，
按上游权重、词长和 Unicode 顺序稳定排序，选取前 50,000 条；随后丢弃词文本和
权重，仅发布 1,200,000 bit、10 次散列的 TCSLEX01 Bloom filter（估算假阳性率
约 `2.11e-5`）。

本仓库（fcitx5 原生 Rust 移植）所作的变更：**数据文件原样沿用**（sha256 与上游
一致），仅将其由参照仓库根目录移至 `data/`，并在 Rust 侧实现读取与打分
（`crates/hux-core/src/lexical.rs`）。转换与移植均不表示上游作者认可本项目。

精确参数与输入/输出摘要见 [`LEXICAL_PRIOR_MANIFEST.json`](LEXICAL_PRIOR_MANIFEST.json)。

## 复现

取得上述上游版本的源词库后，可用上游脚本复现（参数见清单）：

```sh
python3 tools/build_lexical_prior.py \
  --source /path/to/rime-mohu/mohu_flypy.base.dict.yaml \
  --source-repository https://github.com/fcxxxz/rime-mohu \
  --source-revision 9f43098cefdb450fe8dec0f3069fe8d9999b9d10 \
  --source-license CC-BY-4.0 \
  --codes tiger_sentence.codes.txt \
  --output tiger_sentence.lexical.bin \
  --manifest docs/LEXICAL_PRIOR_MANIFEST.json
```

## 校验

```sh
sha256sum data/tiger_sentence.lexical.bin   # 应为 8dbc884b…（与清单同值）
```

CI 在 `rust` 作业中以同一 sha 校验该数据文件（防止替换或漂移）。
