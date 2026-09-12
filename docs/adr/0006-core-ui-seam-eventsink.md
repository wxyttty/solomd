# ADR-0006：core 与 UI 的接缝 = EventSink + CorePaths 注入，模块零横向依赖

- 状态：已采纳（2026-08-26，迁移批次 1）
- 关联：`gpui/crates/core/src/deps.rs`、`gpui/crates/core/src/git_history.rs:438`、`gpui/README.md` 后端迁移批次表

## 背景

原 Tauri 后端的 `AppHandle` 有三种用途：emit 事件、拿 `app_config_dir`、窗口管理。去 Tauri 化时需要决定 core（业务库）与 UI 的边界形态。同时原壳存在模块间内联触发链（`git_auto_commit` 直接调 recipe_runner），迁移后若保留会让 core 内部产生横向依赖。

## 决策

1. core 与外界的**唯一接缝**是 `deps.rs` 的两个注入类型：
   - `EventSink` trait（事件出口）+ 独立的 `EventSinkExt`（`emit_json` 泛型便捷方法——泛型方法会破坏 dyn 兼容，所以拆成两个 trait）；
   - `CorePaths`（路径注入）。
   窗口管理属 UI 层，不进 core。
2. **事件是通知不是请求**：需要返回值走函数返回值。
3. **跨模块触发改为事件解耦**：core 内模块不互相调用触发链。例：`git_auto_commit` 只发 `git-committed` 事件；recipes 的 on-commit 语义由组合层（UI）监听该事件后再调 `recipe_runner::dispatch_on_commit`。

## 真实权衡

- **为什么不用回调注入跨模块逻辑**：事件 + 组合层让每个 core 模块保持零横向依赖，可以单独编译、单独测试、按任意顺序迁移；回调注入则把模块拓扑固化在 core 内部。
- **代价**：组合层（UI）必须显式接线事件 → 目标模块。目前 `git-committed` 的 UI 侧接线尚未完成（core 侧事件已发、`dispatch_on_commit` 就绪），on-commit 类 recipe 在 GPUI 版暂不生效——这是有意的欠账，接线点在 Workspace 的事件泵。
- **历史例外**（不扩散）：`RECIPES_STATE` Lazy 全局（迁移产物，新代码用参数注入）；`ollama.rs::open_ollama_install_page` 在 core 内开 URL（属 UI 职责的历史例外）。

## 后果

- UI 侧标准形态：`ChannelSink`(mpsc) + 实体内轮询泵（timer 80~300ms + `try_recv`），四处复用（workspace / ai_panel / info_panel / recipes_panel）。
- topic 用短形式（`ai-chunk`，去掉原 `solomd://` 前缀），payload 为序列化 JSON。
- 从上游 tauri 壳同步代码的改写表固化在 `core/src/lib.rs` 头注释。
