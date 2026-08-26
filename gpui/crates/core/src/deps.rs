//! deps.rs — solomd-core 与 UI 层的唯一接缝。
//!
//! 原 tauri 后端的 AppHandle 只有三种用途：emit 事件、拿 app_config_dir、
//! 窗口管理。前两者抽象为 EventSink + CorePaths 注入；窗口管理属 UI 层，
//! 不进 core。

use std::path::PathBuf;

/// 事件出口：core 向 UI 推送异步通知（AI 流式 chunk、索引更新等）。
///
/// GPUI 侧实现 = std::sync::mpsc channel + Workspace 泵（与白板 webview IPC
/// 同款已验证模式）；旧 Tauri 壳的实现是 app.emit。
pub trait EventSink: Send + Sync {
    /// `topic` 为原事件名的短形式（如 "ai-chunk"），payload 为已序列化 JSON。
    fn emit(&self, topic: &str, payload: String);
}

/// 便捷扩展（原 tauri Emitter::emit 的 Serialize 形态）。
/// 独立 trait 以保持 EventSink 的 dyn 兼容（泛型方法会让 trait 不成对象）。
pub trait EventSinkExt: EventSink {
    fn emit_json(&self, topic: &str, payload: impl serde::Serialize) {
        self.emit(topic, serde_json::to_string(&payload).unwrap_or_default());
    }
}

impl<T: EventSink + ?Sized> EventSinkExt for T {}

impl EventSink for std::sync::Arc<dyn EventSink> {
    fn emit(&self, topic: &str, payload: String) {
        (**self).emit(topic, payload);
    }
}

/// 无操作出口（测试/不需要事件的调用点）。
pub struct NullSink;

impl EventSink for NullSink {
    fn emit(&self, _topic: &str, _payload: String) {}
}

/// channel 出口：UI 侧持 Receiver 泵消费。
pub struct ChannelSink {
    tx: std::sync::mpsc::Sender<(String, String)>,
}

impl ChannelSink {
    pub fn new(tx: std::sync::mpsc::Sender<(String, String)>) -> Self {
        Self { tx }
    }
}

impl EventSink for ChannelSink {
    fn emit(&self, topic: &str, payload: String) {
        let _ = self.tx.send((topic.to_string(), payload));
    }
}

/// core 需要的文件系统路径（原 app_config_dir / app_data_dir 的替代）。
#[derive(Clone, Debug)]
pub struct CorePaths {
    /// AI key 加密文件等所在目录（原 tauri app_config_dir）。
    pub config_dir: PathBuf,
}

impl CorePaths {
    pub fn new(config_dir: PathBuf) -> Self {
        Self { config_dir }
    }

    /// 缺省位置：~/.config/solomd-gpui（macOS 同样用此路径，自用足够）。
    pub fn default_paths() -> Self {
        let config_dir = dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("solomd-gpui");
        Self { config_dir }
    }
}
