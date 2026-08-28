# Database Guidelines（rag 索引）

- rusqlite bundled（免系统 sqlite 依赖），版本对齐原 lockfile（0.32, features bundled+blob）
- 库文件位置：`<workspace>/.solomd/` 下（随 vault 走）；全局缓存用注入的 `data_dir`
- 连接即用即开（本地单文件，不池化）；嵌入计算在 `spawn_blocking`
- schema 变更走 index_version 字段判断重建（沿用 rag.rs 现状）
