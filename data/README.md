# data/

运行时数据（随包安装到 `…/tigerclaw/`，K3 打包）。

- `tiger_sentence.lexical.bin`：紧凑词先验（TCSLEX01 Bloom filter）。
  来源、许可（CC BY 4.0）与变更说明见
  [`../docs/LEXICAL_PRIOR_ATTRIBUTION.md`](../docs/LEXICAL_PRIOR_ATTRIBUTION.md)，
  参数与校验和见 [`../docs/LEXICAL_PRIOR_MANIFEST.json`](../docs/LEXICAL_PRIOR_MANIFEST.json)。
- `symbols.yaml`：标点表（`punctuator/half_shape|full_shape`；`{commit}`/标量/`{pair}`）。
  取自参照 `symbols.yaml`（pin `35a10b9`），仅覆盖一处默认：half_shape 的 `"/"` 提交 `"/"`
  （参照原表为 `、`）；full_shape 不变。测试夹具 `goldens/key_sequence/symbols.yaml` 保持参照原样。
