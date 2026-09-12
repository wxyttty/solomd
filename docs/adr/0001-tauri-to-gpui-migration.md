# ADR-0001：桌面端从 Tauri 2 迁移到 GPUI

- 状态：已采纳（2026-08-25 决策，2026-08-26 起执行，迁移已完成）
- 关联：`docs/gpui-migration-plan.md`（完整计划与证据索引）、`gpui/README.md`（进度与踩坑现场）

## 背景

桌面端原为 Tauri 2 + Vue 3 + CodeMirror 6。WebView 常驻内存、JS↔Rust IPC 序列化边界、GPU 直绘缺失。本仓库为自用 fork，范围按个人需求裁剪。

## 决策

迁移到 GPUI（zed）+ gpui-component（longbridge）+ zorite gpui-editor（编辑器底座），三 crate 拓扑：

- `gpui/crates/core`（solomd-core）：后端业务库，UI 无关，原 tauri command 壳全拆，经 `deps.rs` 的 EventSink/CorePaths 注入接缝驱动；
- `gpui/crates/app`（solomd 二进制）：GPUI 应用；
- `gpui/crates/spike`：实验室，验证过的增量才进 app。

## 真实权衡

- **为什么不用 gpui-component Editor 自研 caret-reveal**：原计划最高风险点（WYSIWYG 活编辑）。调研确认 zorite/gpui-editor 能力独占（caret-reveal + 可变行高标题 + 块级 providers + SelectionChanged 事件），双栈架构已被 zorite 本身验证；Orion-Editor 已死排除。代价：引入 GPL-3.0 依赖（自用项目可接受）。
- **为什么 git 依赖而非 crates.io**：gpui crates.io 发布管道停摆（0.2.2 停在 2025-10，`gpui_macros` 等 `publish = false` 无人能发，zed discussion #55271）。
- **砍掉的**：移动端、Vim mode、（原计划砍掉后又恢复的）数学公式——P4 经 ratex-gpui 原生恢复。
- **白板路线变化**：原计划 tldraw 经 gpui-wry webview 承载（复用前端资产）；P4 改为 vendor `gpui-whiteboard` 原生化（`crates/zorite/gpui-whiteboard`），tldraw webview 仅剩 spike 检查点④复用，围栏 `tldraw` → `whiteboard`（Scene JSON）。
- **代价**：单人 6–9 个月达到桌面功能对等（实际 P0→P4 于 2026-08-26~08-29 集中完成）；上游 API pre-1.0 churn 需集中升级窗口消化（见 ADR-0002）。

## 后果

- 5MB/20.9 万行笔记 debug 载入 41–91ms，进程稳定；去掉 WebView 内存与 IPC 边界。
- 旧 Tauri/Vue（`app/`）保留为参照与资产来源，不再演进。
- 中文斜体不可见（CJK 无斜体字面 + GPUI 不合成倾斜）成为长期已知边界，补偿方案（fork 叠色相）未启用。

## 迁移对应关系

| 旧（Tauri） | 新（GPUI） |
|---|---|
| `app/src-tauri/src/*.rs` | `gpui/crates/core/src/*.rs`（同名模块） |
| `app/src/lib/cm-live-*.ts` | zorite gpui-editor 内建 |
| `app/src/lib/tldraw-*.ts` | `gpui/board-webview/src/`（spike 用）；产品路径已原生化 |
| `app/src/components/*.vue` | `gpui/crates/app/src/*_panel.rs` + workspace.rs |
| `app/src/styles/tokens.css` | `gpui/crates/app/src/theme.rs` |
