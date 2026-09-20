<!-- SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com> -->
<!-- SPDX-License-Identifier: GPL-3.0-or-later -->

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

上游 [`tiger-sentense-rime`](https://github.com/lvyww/tiger-sentense-rime) 所作的变更：只保留虎句码表可编码的 2～4 字条目，
按上游权重、词长和 Unicode 顺序稳定排序，选取前 50,000 条；随后丢弃词文本和
权重，仅发布 1,200,000 bit、10 次散列的 TCSLEX01 Bloom filter（估算假阳性率
约 `2.11e-5`）。

本仓库（fcitx5 原生 Rust 移植）所作的变更：**数据文件原样沿用**（sha256 与上游
一致），仅将其由参照仓库根目录移至 `data/`，并在 Rust 侧实现读取与打分
（`crates/hux-scheme/tiger/src/lexical.rs`）。转换与移植均不表示上游作者认可本项目。

精确摘要：可编码 2–4 字条目 764,132 条 → 前 50,000 条；生成时码表输入 `tiger_sentence.codes.txt`
sha256 为 `1d3e9b0ce0e4a603be3f220c71acecad846f020e87a52723ecb3814f6b53ac0e`。

## 复现

取得上述上游版本的源词库后，可在参照检出内用其脚本复现（外部检出统一放本仓库
`external/`，见 [`../AGENTS.md`](../AGENTS.md)）：

```sh
git clone https://github.com/fcxxxz/rime-mohu external/rime-mohu
(
  cd external/tiger-sentense-rime
  python3 tools/build_lexical_prior.py \
    --source ../rime-mohu/mohu_flypy.base.dict.yaml \
    --source-repository https://github.com/fcxxxz/rime-mohu \
    --source-revision 9f43098cefdb450fe8dec0f3069fe8d9999b9d10 \
    --source-license CC-BY-4.0 \
    --codes tiger_sentence.codes.txt \
    --output tiger_sentence.lexical.bin \
    --manifest /tmp/lexical.manifest.json
)
```

产物应与本仓库 `data/tiger_sentence.lexical.bin`（sha256 见上）一致；`--manifest` 输出可留作对拍。

## 校验

```sh
sha256sum data/tiger_sentence.lexical.bin   # 应为 8dbc884b…（与清单同值）
```

CI 在 `rust` 作业中以同一 sha 校验该数据文件（防止替换或漂移）。
