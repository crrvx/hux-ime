<!-- SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com> -->
<!-- SPDX-License-Identifier: GPL-3.0-or-later -->

# wubi（五笔）

**骨架，未开工。**

- 形码族：同宇浩，差异在码表 / 选重与排序参数；
- 数据需求：86/98 版码表与字频（同虎句四件套格式）、码长与选重规则、方案标识。
- 依赖方向：`hux-scheme/<方案> → hux-core`（内核零方案依赖，CI 守卫见 `docs/refactor.md` §7）；
  `hux-cfg` 只提供角色词汇与设置，装配由平台（`platform/fcitx5`）构造，方案不反向依赖二者。
- 契约需求（`hux_core::scheme`，见 `docs/refactor.md` §5）：实现 `Scheme` 的
  `id` / `option_declarations()`（自报 4 个运行时角色 `SCHEME_OPTION_ROLES`，键自持）/
  `learning_mode(&self)`（据配置袋**自算**的不透明 mode 串）/ `apply_config`（按角色解析配置袋、
  逐角色回报诊断）/ `host_options`，以及 `new_session` / `free_session` / `reset_session` /
  `process_key` / `select_candidate` / `rebuild` / `take_learning_events` / `buffered_text` /
  `auxiliary_lookup_active` / `auxiliary_rows`；资产目录由平台解析后传入。
- 前置（已满足）：`hux_core::scheme` 契约已于 P4c 落地（见 `docs/refactor.md` §5）；本骨架待开工。