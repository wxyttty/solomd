//! clipboard_html.rs — 剪贴板富文本（HTML flavor）直连系统 API。
//!
//! 为什么需要这个模块：GPUI 的 `ClipboardEntry` 只有三种形态——
//! `String` / `Image` / `ExternalPaths`，**没有 HTML flavor**：
//!
//! ```ignore
//! pub enum ClipboardEntry {
//!     String(ClipboardString),
//!     Image(Image),
//!     ExternalPaths(ExternalPaths),
//! }
//! ```
//!
//! 所以 `cx.read_from_clipboard()` 拿不到浏览器 / Word / 备忘录放在
//! `public.html`（Windows 是 `CF_HTML`）里的富文本。这里**不改平台层、
//! 不 fork 框架**：objc2 三件套本来就在依赖树里（gpui_macos 用同版本），
//! 只是把它们变成 app 的直接依赖，用 `NSPasteboard` 读 HTML。
//!
//! 其它平台：`read_html` 返回 None，调用方回退到纯文本路径（粘贴仍可用，
//! 只是不带格式）。

/// 读剪贴板里的 HTML（`public.html`）。无 HTML flavor 时 None。
#[cfg(target_os = "macos")]
pub fn read_html() -> Option<String> {
    use objc2_app_kit::{NSPasteboard, NSPasteboardTypeHTML};
    let pb = NSPasteboard::generalPasteboard();
    let s = unsafe { pb.stringForType(NSPasteboardTypeHTML) }?;
    Some(s.to_string())
}

#[cfg(not(target_os = "macos"))]
pub fn read_html() -> Option<String> {
    // Windows 需要 CF_HTML 头（版本/偏移/长度），Linux 需要 x11rb 或
    // wl-clipboard 子进程；两者都未接入，回退纯文本路径。
    None
}
