# Frontend Development Guidelines（GPUI 桌面应用）

> 前端 = `gpui/crates/app`（solomd 二进制）+ `crates/spike` 实验室。
> UI 框架：GPUI（zed）+ gpui-component@1505b14 + zorite gpui-editor。
> 全部中文硬编码（自用决策，无 i18n 层）。

---

## Guidelines Index

| Guide | Description | Status |
|-------|-------------|--------|
| [gpui-guidelines.md](./gpui-guidelines.md) | 依赖锁步、实体/面板模式、事件泵、主题、已知边界 | **Filled** |
| [editor-guidelines.md](./editor-guidelines.md) | zorite gpui-editor 集成约定（providers/events/diagnostics） | **Filled** |
| [directory-structure.md](./directory-structure.md) | crate 结构与模块职责 | Filled |
| [component-guidelines.md](./component-guidelines.md) | 右栏页/Dock 面板模式 | Filled |
| [quality-guidelines.md](./quality-guidelines.md) | 禁止模式与冒烟流程 | Filled |

其余模板文件（hook-guidelines / state-management / type-safety）为 Vue 时代遗留桩，
**已过时未删除**（避免误删授权），以本目录 GPUI 文件为准，勿参考旧桩。

---
