//! solomd-core — SoloMD 后端业务逻辑（自 tauri 壳迁移，UI 无关）。
//!
//! 迁移约定：
//! - 原 `#[tauri::command] fn xxx(app: AppHandle, ...)` → `pub fn xxx(sink, paths, ...)`
//!   或拆成 `_inner` 纯函数 + 薄入口；
//! - `app.emit("solomd://topic", payload)` → `sink.emit("topic", json)`；
//! - `tauri::async_runtime::spawn_blocking` → `tokio::task::spawn_blocking`；
//! - 模块名与原文件一一对应，方便对照上游 diff。

pub mod deps;

// 迁移批次 1（AI + agent 栈）：
// 已接入: ai_keystore, agent_run, crypto, cost_meter, git_history, ollama, pricing, search, trace
pub mod ai_proxy;
pub mod agent_run;
pub mod agent_tools;
pub mod ai_keystore;
pub mod convert;
// dev_bridge 暂不编译：其本质是"外部 eval 驱动 Vue UI"的调试通道，
// GPUI 无 JS 求值面，需原生重设计（P3）。源文件保留备查。
// pub mod dev_bridge;
pub mod github_sync;
pub mod integrations;
pub mod watcher;
pub mod cost_meter;
pub mod crypto;
pub mod git_history;
pub mod ollama;
pub mod cjk_proofread;
pub mod pricing;
pub mod rag;
pub mod recipes;
pub mod recipe_runner;
pub mod workspace_index;
pub mod search;
pub mod trace;

pub use deps::{ChannelSink, CorePaths, EventSink, NullSink};
