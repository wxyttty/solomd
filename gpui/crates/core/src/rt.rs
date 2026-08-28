//! rt.rs — 进程级 tokio runtime。
//!
//! 迁移约定原为 `tauri::async_runtime::spawn_blocking` → `tokio::task::spawn_blocking`，
//! 但那有个隐含前提：Tauri 壳自带全局 runtime。core 解耦后宿主是 GPUI
//! executor（自有线程池，无 tokio 上下文），直接调 `tokio::spawn` /
//! `tokio::task::spawn_blocking` 会 panic（"must be called from the context
//! of a Tokio 1.x runtime"），reqwest 的连接池也需要 reactor。
//!
//! 统一从这里 spawn：`Handle::spawn*` 可在任意线程调用，不依赖线程局部
//! runtime 上下文。core 内禁止直接使用 `tokio::spawn` /
//! `tokio::task::spawn_blocking`，一律走本模块。
//!
//! 宿主侧桥接：返回的 `JoinHandle` 本身只是普通 future，可在任意
//! executor 上 await——GPUI 的 `cx.spawn` 里
//! `solomd_core::rt::spawn(async { … }).await` 即可把 tokio 依赖的
//! future（reqwest 等）放到本 runtime 执行。

use once_cell::sync::Lazy;

static RT: Lazy<tokio::runtime::Runtime> = Lazy::new(|| {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("global tokio runtime")
});

/// 进程级 runtime 句柄（任意线程可用）。
pub fn handle() -> tokio::runtime::Handle {
    RT.handle().clone()
}

/// 在进程级 runtime 上执行阻塞任务（原 `tokio::task::spawn_blocking`）。
pub fn spawn_blocking<F, R>(f: F) -> tokio::task::JoinHandle<R>
where
    F: FnOnce() -> R + Send + 'static,
    R: Send + 'static,
{
    RT.handle().spawn_blocking(f)
}

/// 在进程级 runtime 上执行异步任务（原 `tokio::spawn`）。
pub fn spawn<F>(f: F) -> tokio::task::JoinHandle<F::Output>
where
    F: std::future::Future + Send + 'static,
    F::Output: Send + 'static,
{
    RT.handle().spawn(f)
}
