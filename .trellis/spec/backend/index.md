# Backend Development Guidelines（solomd-core）

> 后端 = `gpui/crates/core`（solomd-core crate），UI 无关的纯 Rust 业务库。
> 自 Tauri 壳迁移而来，tauri 代码依赖为零。

---

## Overview

solomd-core 承载全部业务逻辑：AI 代理（14 家供应商流式）、agent 运行/工具、
Git 历史、GitHub/Gitea 同步、语义索引（rag）、recipes、中文校对、加密、成本计量。
UI 层（GPUI / 未来 CLI / 测试）通过两个注入 trait 驱动它。

---

## Guidelines Index

| Guide | Description | Status |
|-------|-------------|--------|
| [solomd-core Guidelines](./solomd-core-guidelines.md) | 注入接缝、模块迁移模式、事件协议 | **Filled** |
| [Directory Structure](./directory-structure.md) | 模块组织（沿用原文件名一一对应） | Filled |
| [Error Handling](./error-handling.md) | `Result<_, String>` 约定（沿用原壳） | Filled |
| [Database Guidelines](./database-guidelines.md) | rusqlite（rag 索引）本地库约定 | Filled |
| [Quality Guidelines](./quality-guidelines.md) | 禁止模式与升级流程 | Filled |
| [Logging Guidelines](./logging-guidelines.md) | eprintln/tracing 约定 | Filled |

---

**Language**: 中文（自用 fork 决策，2026-08-26；模板默认英文已覆盖）。
