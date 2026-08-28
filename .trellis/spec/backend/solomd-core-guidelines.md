# solomd-core 开发规范

> 适用：`gpui/crates/core` 全部模块。核心原则：**core 不认识 UI**。

---

## 1. 注入接缝（必须）

core 与外界的唯一接缝是 `deps.rs` 的两个类型，禁止绕过：

```rust
// 事件出口：原 tauri app.emit 的替代。UI 侧实现 = mpsc channel + 异步泵。
pub trait EventSink: Send + Sync {
    fn emit(&self, topic: &str, payload: String);
}
// 便捷扩展是独立 trait（泛型方法会破坏 dyn 兼容）：
pub trait EventSinkExt: EventSink { fn emit_json(&self, topic: &str, payload: impl serde::Serialize); }
// Arc<dyn EventSink> 已实现转发，组合层直接 clone 传参。

// 路径注入：原 app_config_dir / app_data_dir 的替代。
pub struct CorePaths { pub config_dir: PathBuf }
```

**禁止**：
- core 内出现 `tauri`、`gpui`、任何 UI crate 依赖
- core 内直接 `std::process::Command::new("open")` 打开 URL/文件（属 UI 职责，
  ollama.rs 的 `open_ollama_install_page` 是历史例外，勿新增）
- 用全局可变单例传递本应注入的上下文（`RECIPES_STATE` 是历史迁移产物，新代码用参数注入）

## 2. 事件协议

- topic 用短形式（原 `solomd://ai-chunk` → `ai-chunk`），payload 为序列化 JSON
- 事件是**通知**不是请求——需要返回值的走函数返回值
- 组合层解耦模式：core 内不再互相调用触发链（例：`git_auto_commit` 发
  `git-committed` 事件，recipes 的 on-commit 由 UI 组合层监听后调
  `dispatch_on_commit`），保持模块零横向依赖

## 3. 模块迁移模式（从上游 tauri 壳同步时）

原 `#[tauri::command] fn xxx(app: AppHandle, ...)` 的标准改写：

| 原形态 | 改写 |
|---|---|
| `app.emit("solomd://t", p)` | `sink.emit_json("t", p)` |
| `app.path().app_config_dir()` | `paths.config_dir`（或 CorePaths 参数） |
| `tauri::async_runtime::spawn_blocking` | `rt::spawn_blocking`（进程级 runtime，见下）；**禁止**直用 `tokio::spawn` / `tokio::task::spawn_blocking` |
| `app.state::<X>()` | Lazy 全局（存量）或参数注入（新代码） |
| `#[tauri::command]` 属性 | 删除；前端直调壳不迁移 |

同步时**保持模块名与函数名一一对应**，方便对照上游 diff。

## 3.5 进程级 tokio runtime（rt.rs，必须）

宿主是 GPUI executor（自有线程池，**无 tokio 上下文**）。直调
`tokio::spawn` / `tokio::task::spawn_blocking` 会 panic（"must be called from
the context of a Tokio 1.x runtime"），reqwest 连接池也要 reactor——这是
Tauri 迁移时 `tauri::async_runtime` 直译埋下的潜伏崩溃（sync 面板按钮、
AI 流式都中过招）。

- core 内所有 spawn 一律走 `crate::rt`：`rt::spawn_blocking(f)` / `rt::spawn(fut)`
  （`Handle::spawn*` 可在任意线程调用，不依赖线程局部上下文）
- 宿主侧桥接 tokio 依赖 future（reqwest 等）：
  `cx.spawn(async move { solomd_core::rt::spawn(async { … }).await })` ——
  JoinHandle 是普通 future，可在 GPUI executor 上 await，任务在 rt 内执行
- 同步 CPU 密集活（git2/rusqlite/pandoc 子进程）优先 `_inner` 函数 +
  GPUI `background_executor().spawn`，不必进 rt

## 4. 错误处理

- 沿用原壳约定：对外 API 返回 `Result<T, String>`（错误直接给 UI 展示）
- 内层工具函数可用 `anyhow`，在入口处 `.map_err(|e| e.to_string())`
- 不 panic：解码/IO 失败返回 None/Err（例：图片解码带 512MB limits 防解压炸弹）

## 5. 数据库（rag 索引）

- rusqlite bundled feature（免系统依赖），库文件放 `<workspace>/.solomd/` 下
- 解码/嵌入在 `spawn_blocking` 中做；缓存目录来自注入的 `data_dir`

## 6. 日志

- core 内用 `eprintln!`（迁移自原壳，量少且都是关键路径）或 `tracing`
- 事件泵/重载类 UI 侧日志带 `[sync]`/`[poll]`/`[ipc]` 前缀，stdout 可 grep

## 7. 升级上游流程（tauri 壳同步）

1. 对照 `app/src-tauri/src/<module>.rs` 的 diff
2. 只搬**业务逻辑**变化；command 壳/emit/AppHandle 改动按 §3 重写
3. 跑 `cargo check -p solomd-core` + app 冒烟
