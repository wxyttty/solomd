//! single_instance.rs — 单实例（自用足够，跨平台无依赖）。
//!
//! 进程启动时占住固定本地端口；第二个实例 bind 失败即退出。
//! 不做激活转发（把 vault 路径传给首实例）——单 vault 场景下意义有限。

use std::net::TcpListener;

const ACTIVATE_PORT: u16 = 45917;

/// 启动早期调用：非首实例直接退出进程。
pub fn ensure_single_instance() {
    match TcpListener::bind(("127.0.0.1", ACTIVATE_PORT)) {
        // listener 泄漏到进程结束——socket 保持占用即可，无需 accept。
        Ok(listener) => std::mem::forget(listener),
        Err(_) => {
            eprintln!("solomd 已在运行（127.0.0.1:{ACTIVATE_PORT} 被占用），本次启动退出。");
            std::process::exit(0);
        }
    }
}
