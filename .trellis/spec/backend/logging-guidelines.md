# Logging Guidelines（solomd-core）

- `eprintln!` 为主（原壳迁移产物），`tracing` 已引入可逐步统一
- 前缀约定：core 内模块名打头（`[ipc]` `[fence]` `[mermaid]`）；UI 侧泵类
  `[sync]` `[poll]`——stdout 可 grep 是硬要求
- 只记关键路径与失败；循环内高频事件（每 chunk）不打
