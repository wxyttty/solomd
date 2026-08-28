//! whiteboard.rs — tldraw 白板独立窗口（gpui-wry 承载）。
//!
//! 自 spike 检查点④移植并产品化：
//! - 打开入口是 NotePanel 的「白板」按钮（无围栏时自动在文末补一个）；
//! - 初始快照取自活跃笔记的编辑器缓冲；
//! - 画板防抖回报快照 → 重写围栏体（info 行逐字节保留）→ 直接落盘；
//!   回写走 NotePanel（编辑器文本 + vault.write_note），标签不进脏态。
//!
//! IPC 协议见 gpui/board-webview/src/main.ts 头注释。

use std::ops::Range;
use std::sync::OnceLock;

use gpui::{
    div, AppContext as _, Context, Entity, Focusable, IntoElement,
    ParentElement as _, Render, SharedString, Styled as _, TitlebarOptions, Window, WindowOptions,
};
use gpui_component::{h_flex, v_flex, ActiveTheme as _, Root};

use crate::note_panel::NotePanel;

/// 打开（或聚焦语义上的）白板窗口：NotePanel「白板」按钮调用。
pub fn open_board(
    note: gpui::WeakEntity<NotePanel>,
    note_title: String,
    initial_snapshot: String,
    cx: &mut gpui::App,
) {
    let title: SharedString = format!("白板 — {note_title}").into();
    let _ = cx.open_window(
        WindowOptions {
            titlebar: Some(TitlebarOptions {
                title: Some(title),
                ..Default::default()
            }),
            ..Default::default()
        },
        move |window, cx| {
            let view = cx.new(|cx| BoardPanel::new(note.clone(), initial_snapshot.clone(), window, cx));
            // 不烘焙 bg：Root::render 每帧按 cx.theme() 刷背景。
            cx.new(|cx| Root::new(view, window, cx))
        },
    );
}

/// tldraw 围栏定位：返回 (info 行范围, body 范围)。
/// 反引号长度自适应（围栏头 ````` 与闭合 ````` 长度 >= 头部才闭合），
/// 容忍 tldraw-markdown 产出的长快照体里出现普通 ``` 行。
pub(crate) fn find_tldraw_fence(md: &str) -> Option<(Range<usize>, Range<usize>)> {
    let info_start = md.find("```tldraw")?;
    let ticks = md[info_start..]
        .chars()
        .take_while(|c| *c == '`')
        .count();
    let line_end = md[info_start..].find('\n')? + info_start;
    let body_start = line_end + 1;

    let mut pos = body_start;
    loop {
        if pos >= md.len() {
            return None;
        }
        let line_start = pos;
        let rest = &md[pos..];
        let nl = rest.find('\n').unwrap_or(rest.len());
        let line_body = &rest[..nl];
        let line_ticks = line_body.chars().take_while(|c| *c == '`').count();
        if line_ticks >= ticks && line_body.trim_matches('`').trim().is_empty() {
            return Some((info_start..line_end, body_start..line_start));
        }
        pos = line_start + nl + 1;
    }
}

// ---------------------------------------------------------------------------
// 以下为 macOS / Windows 实际承载；其他平台给空实现（与 spike 同策略）。
// ---------------------------------------------------------------------------

#[cfg(any(target_os = "macos", target_os = "windows"))]
const HTML_SHELL: &str = r##"<!doctype html>
<html><head><meta charset="utf-8"><title>board</title>
<style>html,body{margin:0;height:100%;font-family:-apple-system,sans-serif}
#wrap{display:flex;flex-direction:column;height:100%}
#board{flex:1;min-height:0}
#status{padding:4px 10px;font-size:12px;color:#666;border-top:1px solid #eee}</style>
<style>__BOARD_CSS__</style>
</head><body>
<div id="wrap"><div id="board"></div><div id="status">加载中…</div></div>
<script>__BOARD_JS__</script>
</body></html>"##;

#[cfg(any(target_os = "macos", target_os = "windows"))]
static BOARD_PAGE: OnceLock<String> = OnceLock::new();

#[cfg(any(target_os = "macos", target_os = "windows"))]
fn board_page() -> &'static str {
    BOARD_PAGE.get_or_init(|| {
        let css = include_str!("../../../board-webview/board-bundle.css");
        let js = include_str!("../../../board-webview/board-bundle.js");
        HTML_SHELL
            .replace("__BOARD_CSS__", css)
            .replace("__BOARD_JS__", js)
    })
}

/// board→host 消息（从 wry ipc 线程投递到 GPUI 异步泵）。
#[cfg(any(target_os = "macos", target_os = "windows"))]
#[derive(Debug)]
enum BoardMsg {
    Boot,
    Loaded { bytes: usize },
    Snapshot { body: String },
    BootError { error: String },
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
fn parse_board_msg(raw: &str) -> Option<BoardMsg> {
    let v: serde_json::Value = serde_json::from_str(raw).ok()?;
    match v.get("type")?.as_str()? {
        "solomd-boot" => Some(BoardMsg::Boot),
        "solomd-loaded" => Some(BoardMsg::Loaded {
            bytes: v.get("bytes")?.as_u64()? as usize,
        }),
        "solomd-snapshot" => Some(BoardMsg::Snapshot {
            body: v.get("body")?.as_str()?.to_string(),
        }),
        "solomd-boot-error" => Some(BoardMsg::BootError {
            error: v.get("error")?.as_str()?.to_string(),
        }),
        _ => None,
    }
}

pub struct BoardPanel {
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    webview: Entity<gpui_wry::WebView>,
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    rx: std::sync::mpsc::Receiver<BoardMsg>,
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    note: gpui::WeakEntity<NotePanel>,
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    initial_snapshot: String,
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    last_written: Option<String>,
    status: SharedString,
}

impl BoardPanel {
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    pub fn new(
        note: gpui::WeakEntity<NotePanel>,
        initial_snapshot: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        use raw_window_handle::HasWindowHandle;
        use std::sync::mpsc;

        let (tx, rx) = mpsc::channel::<BoardMsg>();

        let handle = window.window_handle().expect("No window handle");
        let webview = wry::WebViewBuilder::new()
            .with_html(board_page())
            .with_ipc_handler(move |req: wry::http::Request<String>| {
                if let Some(msg) = parse_board_msg(&req.body()) {
                    let _ = tx.send(msg);
                }
            })
            .build_as_child(&handle)
            .expect("Failed to build wry webview");

        let webview = cx.new(|cx| gpui_wry::WebView::new(webview, window, cx));

        let this = Self {
            webview,
            rx,
            note,
            last_written: None,
            status: format!("初始快照 {} bytes", initial_snapshot.len()).into(),
            initial_snapshot,
        };
        this.start_pump(cx);
        this
    }

    /// 异步泵：boot → 下发快照 → 持续消费防抖快照回写笔记（不退出）。
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    fn start_pump(&self, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| loop {
            cx.background_executor()
                .timer(std::time::Duration::from_millis(120))
                .await;
            let Some(entity) = this.upgrade() else { break };
            entity.update(cx, |panel, cx| {
                while let Ok(msg) = panel.rx.try_recv() {
                    match msg {
                        BoardMsg::Boot => {
                            let snapshot = panel.initial_snapshot.clone();
                            let script =
                                format!("window.solomdBoard.loadSnapshot({snapshot})");
                            panel.webview.update(cx, |v, _| {
                                let _ = v.raw().evaluate_script(&script);
                            });
                            panel.status =
                                format!("已下发围栏快照（{} bytes）", snapshot.len()).into();
                        }
                        BoardMsg::Loaded { bytes } => {
                            panel.status =
                                format!("画板已载入宿主快照（{bytes} bytes）").into();
                        }
                        BoardMsg::Snapshot { body } => {
                            // 防抖快照持续回报：内容没变不回写。
                            if panel.last_written.as_deref() == Some(body.as_str()) {
                                continue;
                            }
                            let wrote = panel.note.upgrade().map(|np| {
                                np.update(cx, |n, cx| {
                                    n.write_tldraw_snapshot(&body, cx);
                                })
                            });
                            match wrote {
                                Some(()) => {
                                    let n = body.len();
                                    panel.last_written = Some(body);
                                    panel.status = format!("已回写围栏（{n} bytes）").into();
                                }
                                None => {
                                    panel.status = "笔记已关闭，回写停止".into();
                                }
                            }
                        }
                        BoardMsg::BootError { error } => {
                            panel.status = format!("❌ 挂载失败: {error}").into();
                        }
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }
}

impl Render for BoardPanel {
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .size_full()
            .child(
                h_flex()
                    .px_2()
                    .py_1()
                    .gap_2()
                    .border_b_1()
                    .border_color(cx.theme().border)
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .truncate()
                            .child(self.status.clone()),
                    ),
            )
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .child(self.webview.clone()),
            )
    }

    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().child("白板仅支持 macOS / Windows")
    }
}

impl Focusable for BoardPanel {
    fn focus_handle(&self, cx: &gpui::App) -> gpui::FocusHandle {
        cx.focus_handle()
    }
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
impl BoardPanel {
    pub fn new(
        _note: gpui::WeakEntity<NotePanel>,
        _initial_snapshot: String,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Self {
        Self {
            status: "白板仅支持 macOS / Windows".into(),
        }
    }
}
