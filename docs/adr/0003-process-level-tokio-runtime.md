# ADR-0003：core 内建进程级 tokio runtime（rt.rs）

- 状态：已采纳（2026-08-29，P3 期间发现并修复）
- 关联：`gpui/crates/core/src/rt.rs`、`gpui/README.md`「tokio runtime 修复」行

## 背景

Tauri → GPUI 迁移时，`tauri::async_runtime::spawn_blocking` 被直译为 `tokio::task::spawn_blocking`。但这个直译有隐含前提：**Tauri 壳自带全局 tokio runtime**。core 解耦后宿主是 GPUI executor（自有线程池，无 tokio 上下文）：

- GPUI executor 线程上直调 `tokio::spawn` / `tokio::task::spawn_blocking` 会 panic（"must be called from the context of a Tokio 1.x runtime"）；
- reqwest 连接池也需要 reactor。

这是迁移埋下的**潜伏崩溃**：sync 面板按钮、AI 流式（reqwest）都中过招——不是必现，而是走到特定代码路径才炸。

## 决策

core 内建进程级 runtime（once_cell Lazy）：所有 spawn 一律走 `crate::rt` 的 `rt::spawn(fut)` / `rt::spawn_blocking(f)`。`Handle::spawn*` 可在任意线程调用，不依赖线程局部 runtime 上下文。

宿主桥接模式：`JoinHandle` 本身只是普通 future，可在任意 executor 上 await——GPUI 的 `cx.spawn` 里 `solomd_core::rt::spawn(async { … }).await` 即可让 tokio 依赖的任务在 rt 内执行、在 GPUI executor 上等待（`ai_panel.rs:301`、`info_panel.rs:122` 同款）。

## 真实权衡

- **为什么不要求宿主提供 runtime**：core 的定位是 UI 无关业务库，未来宿主可能是 CLI 或测试 harness；自带 runtime 让任何宿主零成本接入，代价只是常驻一个（惰性启动的）线程池。
- **纯同步 CPU 密集活不进 rt**：git2/rusqlite/pandoc 子进程走 `_inner` 同步函数 + GPUI `background_executor().spawn`（`history_panel.rs` 模式），避免不必要的 tokio 任务开销。
- 修复时 core 全部 ~40 处 spawn 统一替换；core 测试套件 131/133 通过（2 个失败为迁移前遗留 fixture 缺失，与本修复无关）。

## 后果

- core 内**禁止**直接 `tokio::spawn` / `tokio::task::spawn_blocking`（不变量已入 AGENTS.md）。
- dev_bridge 的 TCP 服务走 std 线程，不经 rt（有意的：调试通道不依赖 tokio）。
