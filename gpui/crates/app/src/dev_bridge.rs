//! dev_bridge.rs — GPUI 版调试桥（**仅 debug 构建编译**，release 零引用）。
//!
//! 原 Tauri 版（core/src/dev_bridge.rs，未编译）是 localhost JSON-RPC：
//! 接收 JS 字符串在 WebView 里 eval，以此驱动 Vue UI。GPUI 无 JS 求值面，
//! 重设计为**原生命令 API**——同一套 port/token 文件约定，语义从
//! "eval 任意 JS" 收窄为三个白名单动作：
//!
//!   GET  /ping           → {"ok":true,"app":"solomd-gpui"}
//!   GET  /state          → {"vault","tabs","active_note"}
//!   POST /open_note      {"path": "..."}   （vault 相对或绝对路径）
//!   POST /run_recipe     {"slug": "..."}
//!
//! 鉴权沿用原版：Bearer token；`dev-bridge.port` / `dev-bridge.token`
//! 写入 config_dir（macOS: `~/Library/Application Support/solomd-gpui/`，
//! token 0600）。服务端是 std 线程 + 阻塞 TCP（不经 rt，无 tokio 依赖），
//! UI 动作经 channel 交给 Workspace 的事件泵执行。

use std::io::{Read as _, Write as _};
use std::net::{TcpListener, TcpStream};
use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Mutex, OnceLock};

use solomd_core::deps::CorePaths;

/// UI 侧要执行的动作（server 线程 → Workspace 泵）。
pub enum DevAction {
    State,
    OpenNote(String),
    RunRecipe(String),
}

pub struct DevRequest {
    pub action: DevAction,
    pub reply: Sender<serde_json::Value>,
}

struct Channel {
    tx: Sender<DevRequest>,
    rx: Mutex<Receiver<DevRequest>>,
}

static CHAN: OnceLock<Channel> = OnceLock::new();

fn chan() -> &'static Channel {
    CHAN.get_or_init(|| {
        let (tx, rx) = std::sync::mpsc::channel();
        Channel { tx, rx: Mutex::new(rx) }
    })
}

/// Workspace 事件泵轮询。
pub fn try_recv() -> Option<DevRequest> {
    chan().rx.lock().ok()?.try_recv().ok()
}

/// 启动 debug 桥（进程内只调一次）。
pub fn start(paths: CorePaths) {
    let listener = match TcpListener::bind("127.0.0.1:0") {
        Ok(l) => l,
        Err(e) => {
            eprintln!("[dev_bridge] bind 失败: {e}");
            return;
        }
    };
    let port = match listener.local_addr() {
        Ok(a) => a.port(),
        Err(_) => return,
    };
    let token = random_token();
    if let Err(e) = write_port_token(&paths, port, &token) {
        eprintln!("[dev_bridge] {e}");
        return;
    }
    eprintln!("[dev_bridge] listening on http://127.0.0.1:{port}");
    std::thread::Builder::new()
        .name("dev-bridge".into())
        .spawn(move || {
            for stream in listener.incoming() {
                match stream {
                    Ok(s) => handle_conn(s, &token),
                    Err(_) => continue,
                }
            }
        })
        .expect("spawn dev-bridge thread");
}

// ---------------------------------------------------------------------------
// server 线程侧
// ---------------------------------------------------------------------------

fn handle_conn(mut stream: TcpStream, token: &str) {
    let _ = stream.set_read_timeout(Some(std::time::Duration::from_secs(5)));
    let mut buf = Vec::new();
    let mut chunk = [0u8; 4096];
    // 读到 header 结束 + content-length 的 body 齐全为止。
    let header_end = loop {
        match stream.read(&mut chunk) {
            Ok(0) => return,
            Ok(n) => buf.extend_from_slice(&chunk[..n]),
            Err(_) => return,
        }
        if let Some(pos) = find_header_end(&buf) {
            let headers = String::from_utf8_lossy(&buf[..pos]);
            let len = headers
                .lines()
                .find(|l| l.to_lowercase().starts_with("content-length:"))
                .and_then(|l| l.split(':').nth(1))
                .and_then(|v| v.trim().parse::<usize>().ok())
                .unwrap_or(0);
            let body_start = pos + 4;
            if buf.len() >= body_start + len {
                break (pos, len);
            }
        }
        if buf.len() > 1 << 20 {
            return; // 防炸弹
        }
    };
    let (pos, len) = header_end;
    let body = String::from_utf8_lossy(&buf[pos + 4..pos + 4 + len]).to_string();
    let head = String::from_utf8_lossy(&buf[..pos]).to_string();
    let mut lines = head.lines();
    let request_line = lines.next().unwrap_or_default();
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or_default().to_string();
    let path = parts.next().unwrap_or_default().to_string();
    let auth_ok = lines
        .filter_map(|l| {
            let (k, v) = l.split_once(':')?;
            Some((k.trim().to_lowercase(), v.trim().to_string()))
        })
        .any(|(k, v)| k == "authorization" && v == format!("Bearer {token}"));

    let payload = if !auth_ok {
        Some(json_error("unauthorized"))
    } else {
        dispatch(&method, &path, &body)
    };

    let (status, json) = match payload {
        Some(json) => ("200 OK", json),
        None => ("404 Not Found", json_error("not found")),
    };
    let body = serde_json::to_string(&json).unwrap_or_else(|_| json_error("serialize failed").to_string());
    let resp = format!(
        "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nAccess-Control-Allow-Origin: *\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let _ = stream.write_all(resp.as_bytes());
    let _ = stream.flush();
}

/// None = 404；Some = 200 响应 JSON。
fn dispatch(method: &str, path: &str, body: &str) -> Option<serde_json::Value> {
    let reply = |action: DevAction| -> Option<serde_json::Value> {
        let (tx, rx) = std::sync::mpsc::channel();
        let _ = chan().tx.send(DevRequest { action, reply: tx });
        match rx.recv_timeout(std::time::Duration::from_secs(5)) {
            Ok(v) => Some(v),
            Err(_) => Some(json_error("UI 无响应（超时）")),
        }
    };
    match (method, path) {
        ("GET", "/ping") => Some(serde_json::json!({"ok": true, "app": "solomd-gpui"})),
        ("GET", "/state") => reply(DevAction::State),
        ("POST", "/open_note") => {
            let v: serde_json::Value = serde_json::from_str(body).ok()?;
            let path = v.get("path")?.as_str()?.to_string();
            reply(DevAction::OpenNote(path))
        }
        ("POST", "/run_recipe") => {
            let v: serde_json::Value = serde_json::from_str(body).ok()?;
            let slug = v.get("slug")?.as_str()?.to_string();
            reply(DevAction::RunRecipe(slug))
        }
        _ => None,
    }
}

fn find_header_end(buf: &[u8]) -> Option<usize> {
    buf.windows(4).position(|w| w == b"\r\n\r\n")
}

fn json_error(msg: &str) -> serde_json::Value {
    serde_json::json!({"ok": false, "error": msg})
}

fn random_token() -> String {
    // 32 hex chars；debug 桥够用，不引 rand。
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.subsec_nanos() as u64)
        .unwrap_or(0);
    let pid = std::process::id() as u64;
    let mut x = nanos ^ (pid << 32) | 1;
    let mut out = String::with_capacity(32);
    for _ in 0..32 {
        // xorshift64
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        out.push_str(&format!("{:x}", (x >> 60) & 0xf));
    }
    out
}

fn write_port_token(paths: &CorePaths, port: u16, token: &str) -> Result<(), String> {
    let dir = &paths.config_dir;
    std::fs::create_dir_all(dir).map_err(|e| format!("mkdir {}: {e}", dir.display()))?;
    let port_path = dir.join("dev-bridge.port");
    let token_path = dir.join("dev-bridge.token");
    std::fs::write(&port_path, port.to_string()).map_err(|e| format!("write port file: {e}"))?;
    std::fs::write(&token_path, token).map_err(|e| format!("write token file: {e}"))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&token_path, std::fs::Permissions::from_mode(0o600));
    }
    eprintln!("[dev_bridge] port -> {}", port_path.display());
    eprintln!("[dev_bridge] token -> {}", token_path.display());
    Ok(())
}
