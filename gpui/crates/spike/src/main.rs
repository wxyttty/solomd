//! SoloMD GPUI 迁移 Phase 0 spike。
//!
//! 用法：`cargo run -p spike -- [dock|markdown|editor|webview]`
//!
//! 每次启动打开一个验证面板，便于逐项测量（默认 dock）：
//!   - dock:     检查点① —— Dock 布局 + 文件树 + 中央笔记视图
//!   - markdown: TextView 渲染样例笔记（表格/图片/代码块/mermaid 源）
//!   - editor:   Editor 源码模式（P1 caret-reveal 装饰层的底座）
//!   - webview:  gpui-wry 独立承载能力验证（白板路线的前置检查）
//!
//! 仅使用 gpui-component 当前 HEAD 示例中逐字验证过的 API。

use std::{collections::HashSet, path::PathBuf};

use std::cell::RefCell;
use std::rc::Rc;

mod spike_images;
mod spike_mermaid;
mod warm_theme;

use gpui::prelude::FluentBuilder as _;
use gpui::*;
use gpui_component::{
    ActiveTheme as _, Root, button::Button,
    dock::{self, ClosePanel, DockArea, DockItem, PanelEvent, ToggleZoom},
    h_flex, v_flex,
    input::{Input, InputState},
    text::markdown,
};
use gpui_component_assets::Assets;

/// P0 检查点 ② 的渲染样例。刻意覆盖 TextView 当前已知能力边界内的结构：
/// 标题/强调/链接/行内代码/GFM 表格/围栏代码块/外链图片/mermaid 围栏源码
/// （mermaid 在 TextView 里按代码块显示，真正的图形化渲染属 P1 ariel-rs 工作）。
const SAMPLE_MD: &str = r#"# SoloMD GPUI Spike

一段**加粗**、*斜体*、`行内代码` 与[链接](https://github.com/zed-industries/zed)的普通段落。

## 表格

| 功能 | Tauri | GPUI |
|---|---|---|
| 空载内存 | WebView 常驻 | 无 WebView |
| 渲染 | DOM/CSS | GPU 直绘 |

## 代码块

```rust
actions!(spike, [PasteImage]);

fn main() {
    println!("hello native");
}
```

## 图片

![demo](https://solomd.app/icon.png)

## Mermaid（P1 前显示为源码块）

```mermaid
flowchart TD
    A[SoloMD] -->|migrate| B[GPUI]
```
"#;

actions!(spike, [PasteImage]);

fn main() {
    let panel = std::env::args().nth(1).unwrap_or_else(|| "dock".into());

    let app = gpui_platform::application().with_assets(Assets);

    app.run(move |cx| {
        // 必须先于任何 gpui-component 特性调用。
        gpui_component::init(cx);
        // zorite 编辑器的编辑键位（独立 key context，不遮蔽宿主快捷键）。
        gpui_editor::bind_keys(cx);
        // 图片粘贴：绑真实粘贴键（cmd-/ctrl- 按平台），无图时 propagate
        // 给编辑器原生文本粘贴——与 zorite 同款双键位模式。
        #[cfg(target_os = "macos")]
        cx.bind_keys([KeyBinding::new("cmd-v", PasteImage, Some("Editor"))]);
        #[cfg(not(target_os = "macos"))]
        cx.bind_keys([KeyBinding::new("ctrl-v", PasteImage, Some("Editor"))]);
        // Dock 的标签关闭 / 缩放动作需要键位绑定才能被触发。
        cx.bind_keys([
            KeyBinding::new("cmd-w", ClosePanel, None),
            KeyBinding::new("ctrl-w", ClosePanel, None),
            KeyBinding::new("cmd-y", ToggleZoom, None),
        ]);
        cx.activate(true);

        let kind = match panel.as_str() {
            "editor" => PanelKind::Editor,
            "webview" => PanelKind::Webview,
            "markdown" => PanelKind::Markdown,
            "wys" | "live" => PanelKind::Wys,
            _ => PanelKind::Dock,
        };

        cx.spawn(async move |cx| {
            cx.open_window(WindowOptions::default(), move |window, cx| {
                let view = cx.new(|cx| Workspace::new(kind, window, cx));
                // 窗口第一层必须是 gpui-component 的 Root。
                cx.new(|cx| Root::new(view, window, cx).bg(cx.theme().background))
            })
            .expect("Failed to open window");
        })
        .detach();
    });
}

enum PanelKind {
    Dock,
    Markdown,
    Editor,
    Webview,
    Wys,
}

impl PanelKind {
    fn title(&self) -> &'static str {
        match self {
            PanelKind::Dock => "Spike — Dock + FileTree",
            PanelKind::Markdown => "Spike — Markdown TextView",
            PanelKind::Editor => "Spike — Editor (source mode)",
            PanelKind::Webview => "Spike — Wry Webview",
            PanelKind::Wys => "P1 — gpui-editor (zorite) WYSIWYG",
        }
    }
}

struct Workspace {
    kind: PanelKind,
    editor: Option<Entity<EditorPanel>>,
    webview: Option<Entity<WebviewPanel>>,
    dock_area: Option<Entity<DockArea>>,
    wys: Option<Entity<WysPanel>>,
    ping_count: u32,
}

impl Workspace {
    fn new(kind: PanelKind, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let editor = match kind {
            PanelKind::Editor => Some(cx.new(|cx| EditorPanel::new(window, cx))),
            _ => None,
        };
        let webview = match kind {
            PanelKind::Webview => Some(cx.new(|cx| WebviewPanel::new(window, cx))),
            _ => None,
        };

        let dock_area = match kind {
            PanelKind::Dock => Some(build_dock(window, cx)),
            _ => None,
        };

        let wys = match kind {
            PanelKind::Wys => Some(cx.new(|cx| WysPanel::new(window, cx))),
            _ => None,
        };

        Self {
            kind,
            editor,
            webview,
            dock_area,
            wys,
            ping_count: 0,
        }
    }
}

impl Render for Workspace {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let title = self.kind.title();

        let body = match self.kind {
            PanelKind::Markdown => div()
                .id("md-scroll")
                .size_full()
                .overflow_y_scroll()
                .child(markdown(SAMPLE_MD))
                .into_any_element(),
            PanelKind::Editor => match &self.editor {
                Some(editor) => div().size_full().child(editor.clone()).into_any_element(),
                None => div().into_any_element(),
            },
            PanelKind::Webview => match &self.webview {
                // webview 以原生视图浮在窗口之上，占满剩余区域即可。
                Some(webview) => div().size_full().child(webview.clone()).into_any_element(),
                None => div()
                    .size_full()
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_color(cx.theme().muted_foreground)
                    .child("gpui-wry 仅支持 macOS / Windows")
                    .into_any_element(),
            },
            PanelKind::Wys => match &self.wys {
                Some(w) => div().size_full().child(w.clone()).into_any_element(),
                None => div().into_any_element(),
            },
            PanelKind::Dock => match &self.dock_area {
                Some(area) => div()
                    .flex_1()
                    .min_h_0()
                    .overflow_hidden()
                    .child(area.clone())
                    .into_any_element(),
                None => div().into_any_element(),
            },
        };

        v_flex()
            .size_full()
            .child(
                h_flex()
                    .px_3()
                    .py_2()
                    .gap_2()
                    .border_b_1()
                    .border_color(cx.theme().border)
                    .child(
                        div()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(title.to_string()),
                    )
                    .child(Button::new("quit").label("Quit").on_click(|_, _, cx| {
                        cx.quit();
                    }))
                    .when_some(
                        self.webview.as_ref(),
                        |this, _webview| {
                            let n = self.ping_count + 1;
                            this.child(
                                Button::new("ping-board")
                                    .label(format!("Ping→Board #{}", n))
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.ping_count += 1;
                                        if let Some(wv) = this.webview.as_ref() {
                                            wv.update(cx, |p, cx| p.ping(this.ping_count, cx));
                                        }
                                    })),
                            )
                            .child(div().text_xs().text_color(cx.theme().muted_foreground).child("点页面内“发送快照”按钮，看终端 stdout 的 [ipc] 日志"))
                        },
                    ),
            )
            .child(body)
    }
}

/// P0 检查点 ③ 的底座：空 Editor，后续在此之上实现 caret-reveal 装饰层，
/// 并接入大文件生成器做性能实测。
/// P0 检查点③：大文件性能 harness。
///
/// 生成确定性的 markdown 混合内容（标题/段落/代码块/表格），默认目标 ~5MB。
/// 加载耗时与文档规模会打印到 stdout 并显示在面板头部；滚动/输入延迟由人工
/// 目检感受，后续可在此面板上加帧率探针。
fn generate_large_markdown(target_bytes: usize) -> String {
    let mut out = String::with_capacity(target_bytes + 4096);
    let mut i = 0usize;
    while out.len() < target_bytes {
        i += 1;
        out.push_str(&format!("\n## 第 {} 节\n\n", i));
        out.push_str(&format!(
            "这一段是用于性能测试的正文内容，包含**加粗**、*斜体*和`行内代码`，以及一个[链接](https://example.com/{})。\n\n",
            i
        ));
        if i % 3 == 0 {
            out.push_str("```rust\nfn example() -> u32 {\n    // deterministic filler\n    42 * 7 + 13\n}\n```\n\n");
        }
        if i % 4 == 0 {
            out.push_str(&format!(
                "| 列A | 列B | 列C |\n|---|---|---|\n| {} | 值{} | 备注{} |\n\n",
                i,
                i * 2,
                i * 3
            ));
        }
        if i % 50 == 0 {
            out.push_str(&format!("\n# 第 {} 章\n\n", i / 50));
        }
    }
    out
}

struct EditorPanel {
    state: Entity<InputState>,
    stats: DocStats,
}

#[derive(Clone, Copy)]
struct DocStats {
    bytes: usize,
    lines: usize,
    gen_ms: u128,
    load_ms: u128,
}

impl EditorPanel {
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        const TARGET_BYTES: usize = 5 * 1024 * 1024;

        let t0 = std::time::Instant::now();
        let text = generate_large_markdown(TARGET_BYTES);
        let gen_ms = t0.elapsed().as_millis();
        let bytes = text.len();
        let lines = text.lines().count();

        let t1 = std::time::Instant::now();
        let state = cx.new(|cx| {
            InputState::new(window, cx)
                .default_value(text)
                .multi_line(true)
        });
        let load_ms = t1.elapsed().as_millis();

        let stats = DocStats {
            bytes,
            lines,
            gen_ms,
            load_ms,
        };
        println!(
            "[perf] doc = {:.1} MB / {} lines · generate {} ms · load-into-editor {} ms",
            bytes as f64 / 1024.0 / 1024.0,
            lines,
            gen_ms,
            load_ms
        );

        Self { state, stats }
    }
}

impl Render for EditorPanel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let s = self.stats;
        v_flex()
            .size_full()
            .child(
                h_flex()
                    .px_2()
                    .py_1()
                    .gap_2()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(format!(
                        "{:.1} MB · {} 行 · 生成 {}ms · 载入编辑器 {}ms（滚动/输入手感请人工目检）",
                        s.bytes as f64 / 1024.0 / 1024.0,
                        s.lines,
                        s.gen_ms,
                        s.load_ms
                    )),
            )
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .p_2()
                    .child(Input::new(&self.state).h(relative(1.))),
            )
    }
}

// 与 gpui-component examples/webview 同构：wry 以子视图方式挂到窗口句柄上，
// 再由 gpui-wry 包一层成为可渲染实体。
//
// P0 检查点④：真实 tldraw runtime（board-webview/ 打包产物）+ IPC 快照回写闭环。
// 协议见 board-webview/src/main.ts 头注释；宿主侧用 channel + 异步泵消费消息，
// 因为 wry 的 ipc 回调拿不到 GPUI 上下文。
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

/// 样例笔记：含一个 tldraw 围栏。围栏解析/重写在 Rust 侧做最小实现，
/// 字节级不变量（id/height 属性保留、围栏体替换）是检查点④的验收内容之一。
const NOTE_MD: &str = r#"# 白板演示笔记

正文段落。下面是一个白板：

```tldraw id="demo-board" height="520"
{}
```

围栏之后的正文。"#;

static BOARD_PAGE: std::sync::OnceLock<String> = std::sync::OnceLock::new();

fn board_page() -> &'static str {
    BOARD_PAGE.get_or_init(|| {
        let css = include_str!("../../../board-webview/board-bundle.css");
        let js = include_str!("../../../board-webview/board-bundle.js");
        HTML_SHELL.replace("__BOARD_CSS__", css).replace("__BOARD_JS__", js)
    })
}

/// board→host 消息（从 wry ipc 线程投递到 GPUI 异步泵）。
#[derive(Debug)]
enum BoardMsg {
    Boot,
    Loaded { bytes: usize },
    Snapshot { body: String },
    BootError { error: String },
}

fn parse_board_msg(raw: &str) -> Option<BoardMsg> {
    let v: serde_json::Value = serde_json::from_str(raw).ok()?;
    match v.get("type")?.as_str()? {
        "solomd-boot" => Some(BoardMsg::Boot),
        "solomd-loaded" => Some(BoardMsg::Loaded { bytes: v.get("bytes")?.as_u64()? as usize }),
        "solomd-snapshot" => Some(BoardMsg::Snapshot {
            body: v.get("body")?.as_str()?.to_string(),
        }),
        "solomd-boot-error" => Some(BoardMsg::BootError {
            error: v.get("error")?.as_str()?.to_string(),
        }),
        _ => None,
    }
}

/// 最小围栏解析：返回 (info 行范围, body 范围)。spike 级实现，
/// 真实版本需移植 tldraw-markdown.ts 的容错属性读取与反引号长度自适应。
fn find_tldraw_fence(md: &str) -> Option<(std::ops::Range<usize>, std::ops::Range<usize>)> {
    let info_start = md.find("```tldraw")?;
    let line_end = md[info_start..].find('\n')? + info_start;
    let body_start = line_end + 1;
    let close = md[body_start..].find("```")? + body_start;
    Some((info_start..line_end, body_start..close))
}

struct WebviewPanel {
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    webview: Entity<gpui_wry::WebView>,
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    rx: std::sync::mpsc::Receiver<BoardMsg>,
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    note_md: String,
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    last_event: String,
}

impl WebviewPanel {
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        use raw_window_handle::HasWindowHandle;
        use std::sync::mpsc;

        let (tx, rx) = mpsc::channel::<BoardMsg>();

        // 从样例笔记提取初始围栏体作为画板初始快照。
        let note_md = NOTE_MD.to_string();
        let initial_snapshot = find_tldraw_fence(&note_md)
            .map(|(_, body)| note_md[body].trim().to_string())
            .unwrap_or_else(|| "{}".to_string());
        println!(
            "[fence] 初始快照 {} bytes: {}",
            initial_snapshot.len(),
            initial_snapshot
        );

        let handle = window.window_handle().expect("No window handle");
        let webview = wry::WebViewBuilder::new()
            .with_html(board_page())
            .with_ipc_handler(move |req: wry::http::Request<String>| {
                if let Some(msg) = parse_board_msg(&req.body()) {
                    println!("[ipc] {:?} ({}) bytes", &msg, req.body().len());
                    let _ = tx.send(msg);
                } else {
                    println!("[ipc] 未识别消息: {}", req.body());
                }
            })
            .build_as_child(&handle)
            .expect("Failed to build wry webview");

        let webview = cx.new(|cx| gpui_wry::WebView::new(webview, window, cx));

        let this = Self {
            webview,
            rx,
            note_md,
            last_event: "等待 board 启动…".into(),
        };
        this.start_pump(cx);
        this
    }

    /// 异步泵：消费 ipc channel，驱动「boot → 下发快照 → 收回报 → 重写围栏」闭环。
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    fn start_pump(&self, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| loop {
            cx.background_executor()
                .timer(std::time::Duration::from_millis(120))
                .await;
            let Some(entity) = this.upgrade() else { break };
            let mut quit = false;
            entity.update(cx, |panel, cx| {
                while let Ok(msg) = panel.rx.try_recv() {
                    match msg {
                        BoardMsg::Boot => {
                            let snapshot = find_tldraw_fence(&panel.note_md)
                                .map(|(_, b)| panel.note_md[b].trim().to_string())
                                .unwrap_or_default();
                            let script = format!(
                                "window.solomdBoard.loadSnapshot({})",
                                snapshot
                            );
                            panel.webview.update(cx, |v, _| {
                                let _ = v.raw().evaluate_script(&script);
                            });
                            panel.last_event = format!(
                                "已下发围栏快照（{} bytes），等待画板回报…",
                                snapshot.len()
                            );
                        }
                        BoardMsg::Loaded { bytes } => {
                            panel.last_event = format!("画板已载入宿主快照（{} bytes）", bytes);
                        }
                        BoardMsg::Snapshot { body } => {
                            // 围栏回写：替换 body，保留 info 行（id/height 不变量）。
                            let before = panel.note_md.len();
                            if let Some((info, old_body)) = find_tldraw_fence(&panel.note_md) {
                                let info_line = panel.note_md[info].to_string();
                                let mut next_md = String::with_capacity(before + body.len());
                                next_md.push_str(&panel.note_md[..old_body.start]);
                                next_md.push_str(&body);
                                next_md.push('\n');
                                next_md.push_str(&panel.note_md[old_body.end..]);
                                panel.note_md = next_md;

                                // 验收不变量：重写后重新定位围栏，info 行必须逐字节一致。
                                let preserved = find_tldraw_fence(&panel.note_md)
                                    .map(|(info2, _)| panel.note_md[info2.clone()] == info_line)
                                    .unwrap_or(false);
                                println!(
                                    "[fence] 围栏已重写：{} → {} bytes · info 行保留: {}",
                                    before,
                                    panel.note_md.len(),
                                    preserved
                                );
                                println!("[fence] 新围栏体前 120 bytes: {}", {
                                    let s = body.replace('\n', " ");
                                    let mut t = s.as_str();
                                    if t.len() > 120 {
                                        t = &t[..120];
                                    }
                                    t.to_string()
                                });
                                panel.last_event = format!(
                                    "✅ 快照回写闭环：围栏 {} → {} bytes（info 行保留: {}）",
                                    before,
                                    panel.note_md.len(),
                                    preserved
                                );
                                quit = true;
                            }
                        }
                        BoardMsg::BootError { error } => {
                            panel.last_event = format!("❌ 挂载失败: {}", error);
                            quit = true;
                        }
                    }
                }
                cx.notify();
            });
            if quit {
                break;
            }
        })
        .detach();
    }

    /// host→board 手动探针：注入脚本调用页面内的 hostPing。
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    pub fn ping(&self, n: u32, cx: &mut App) {
        self.webview.update(cx, |v, _| {
            let script = format!("window.hostPing && window.hostPing({})", n);
            let _ = v.raw().evaluate_script(&script);
        });
    }

    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    fn new(_: &mut Window, _: &mut App) -> Self {
        Self
    }
}

impl Render for WebviewPanel {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        #[cfg(any(target_os = "macos", target_os = "windows"))]
        return v_flex()
            .size_full()
            .child(div().flex_1().min_h_0().child(self.webview.clone()))
            .child(
                div()
                    .px_2()
                    .py_1()
                    .text_xs()
                    .text_color(gpui::black())
                    .child(self.last_event.clone()),
            );
        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        return div();
    }
}

// ---------------------------------------------------------------------------
// 检查点① Dock 布局：左 dock 文件树 + 中央笔记视图。
// API 镜像 gpui-component examples/dock.rs（DockSkin::dock_area →
// set_center/set_dock/set_dock_size）。
// ---------------------------------------------------------------------------

fn build_dock(window: &mut Window, cx: &mut Context<Workspace>) -> Entity<DockArea> {
    let dock_area = cx.new(|cx| DockArea::new("spike-dock", Some(0), window, cx));
    let weak = dock_area.downgrade();

    let left =
        DockItem::tab(cx.new(|cx| FileTreePanel::new(window, cx)), &weak, window, cx);
    let center =
        DockItem::tab(cx.new(|cx| NoteViewPanel::new(window, cx)), &weak, window, cx);

    dock_area.update(cx, |area, cx| {
        area.set_center(center, window, cx);
        area.set_left_dock(left, Some(px(260.)), true, window, cx);
    });

    dock_area
}

/// 中央面板：渲染样例笔记（复用 markdown 检查点的内容）。
struct NoteViewPanel {
    focus_handle: FocusHandle,
}

impl NoteViewPanel {
    fn new(_: &mut Window, cx: &mut Context<Self>) -> Self {
        Self {
            focus_handle: cx.focus_handle(),
        }
    }
}

impl Render for NoteViewPanel {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .id("note-scroll")
            .size_full()
            .overflow_y_scroll()
            .px_4()
            .py_3()
            .child(markdown(SAMPLE_MD))
    }
}

impl Focusable for NoteViewPanel {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl EventEmitter<PanelEvent> for NoteViewPanel {}

impl dock::Panel for NoteViewPanel {
    fn panel_name(&self) -> &'static str {
        "note-view"
    }
}

/// 左侧文件树：真实读取工作区目录，两级展开/收起。
/// 交互与视觉都是骨架级——验证的是 Dock 内嵌自绘交互面板的可行性。
struct FileTreePanel {
    focus_handle: FocusHandle,
    root: PathBuf,
    expanded: HashSet<PathBuf>,
}

impl FileTreePanel {
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let _ = window;
        Self {
            focus_handle: cx.focus_handle(),
            root: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
            expanded: HashSet::new(),
        }
    }

    fn toggle(&mut self, path: PathBuf) {
        if !self.expanded.remove(&path) {
            self.expanded.insert(path);
        }
    }
}

impl FileTreePanel {
    /// 在 render 的 Context<Self> 里递归展开目录行（listener 需要 Context 才能构造）。
    fn walk(
        &self,
        dir: &std::path::Path,
        depth: usize,
        rows: &mut Vec<AnyElement>,
        counter: &mut usize,
        cx: &mut Context<Self>,
    ) {
        if depth > 2 || rows.len() > 400 {
            return;
        }
        let mut entries: Vec<_> = match std::fs::read_dir(dir) {
            Ok(rd) => rd.filter_map(Result::ok).collect(),
            Err(_) => return,
        };
        // 目录在前、忽略大小写排序；跳过构建产物与隐藏文件。
        entries.sort_by_key(|e| {
            (!e.path().is_dir(), e.file_name().to_string_lossy().to_lowercase())
        });
        entries.retain(|e| {
            let name = e.file_name().to_string_lossy().to_string();
            !name.starts_with('.') && name != "target" && name != "node_modules"
        });
        // 根层只展示 gpui 工作区自身，避免把整个仓库拉进来。
        if depth == 0 {
            entries.retain(|e| {
                matches!(
                    e.file_name().to_string_lossy().as_ref(),
                    "crates" | "Cargo.toml" | "README.md" | "rust-toolchain.toml"
                )
            });
        }

        for entry in entries {
            let path = entry.path();
            let is_dir = path.is_dir();
            let name = entry.file_name().to_string_lossy().to_string();
            let expanded = is_dir && self.expanded.contains(&path);
            let glyph = if is_dir {
                if expanded { "▾ " } else { "▸ " }
            } else {
                "· "
            };

            *counter += 1;
            let id = SharedString::from(format!("tree-row-{}", *counter));
            let click_path = path.clone();

            rows.push(
                div()
                    .id(id)
                    .flex()
                    .items_center()
                    .gap_0p5()
                    .pl(px(8. + depth as f32 * 16.))
                    .pr_2()
                    .py_0p5()
                    .rounded_sm()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .hover(|s| s.bg(cx.theme().foreground.opacity(0.06)))
                    .on_click(cx.listener(move |this: &mut Self, _: &ClickEvent, _, _| {
                        this.toggle(click_path.clone());
                    }))
                    .child(glyph.to_string())
                    .child(
                        div()
                            .text_color(if is_dir {
                                cx.theme().foreground
                            } else {
                                cx.theme().muted_foreground
                            })
                            .child(name),
                    )
                    .into_any_element(),
            );

            if expanded {
                self.walk(&path, depth + 1, rows, counter, cx);
            }
        }
    }
}

impl Render for FileTreePanel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let mut rows = Vec::new();
        let mut counter = 0usize;
        self.walk(&self.root, 0, &mut rows, &mut counter, cx);
        v_flex()
            .size_full()
            .id("file-tree")
            .overflow_y_scroll()
            .py_1()
            .children(rows)
    }
}

impl Focusable for FileTreePanel {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl EventEmitter<PanelEvent> for FileTreePanel {}

impl dock::Panel for FileTreePanel {
    fn panel_name(&self) -> &'static str {
        "file-tree"
    }
}


// ---------------------------------------------------------------------------
// P1：gpui-editor（zorite，GPL-3.0）集成验证。
//
// 采用前硬闸门：5MB / 20.9 万行文档的载入耗时与可交互性。该编辑器 auto-grow
// 无内滚动（宿主负责滚动容器），此处先用固定高度窗口直接渲染。
// 后续增量：SyntaxStyle 主题化、block providers（图片/mermaid）、IME 手测。
// ---------------------------------------------------------------------------

/// 目检用小样例：覆盖 caret-reveal 各行内构造 + 块级构造。
const WYS_DEMO_MD: &str = r#"# WYSIWYG 目检

普通段落带**加粗**、*italic English* 与 *拉丁斜体混排*、~~删除线~~、`行内代码` 和[链接](https://example.com)。

> 引用块：暖灰文字与左侧竖条。

- [ ] 任务项一
- [x] 任务项二

![demo](_assets/demo.png)

行内图片：![demo](_assets/demo.png) 与文字混排——缩略图可左键预览、光标可从两侧穿越进入。

## 二级标题

```rust
fn demo() { println!("fenced code"); }
```

```mermaid
flowchart LR
    A[SoloMD] -->|GPUI| B[Native]
    B --> C{Done}
```

--- 
"#;

struct WysPanel {
    state: Entity<gpui_editor::EditorState>,
    wys_off: bool,
}

impl WysPanel {
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        // 默认小样例供目检；SPIKE_BIG=1 切 5MB 性能闸门复测。
        let (text, big) = if std::env::var("SPIKE_BIG").is_ok() {
            let t0 = std::time::Instant::now();
            let text = generate_large_markdown(5 * 1024 * 1024);
            println!("[perf-zorite] generate {} ms", t0.elapsed().as_millis());
            (text, true)
        } else {
            (WYS_DEMO_MD.to_string(), false)
        };

        let images = Rc::new(RefCell::new(spike_images::ImageStore::new(
            std::env::current_dir().unwrap_or_default(),
        )));
        let mermaid = Rc::new(RefCell::new(spike_mermaid::MermaidStore::default()));

        let t1 = std::time::Instant::now();
        let state = cx.new(|cx| {
            let mut ed = gpui_editor::EditorState::new(window, cx).with_text(text.clone());
            // WYSIWYG 开关：装了 SyntaxStyle 即活渲染（clear_markdown_style 可随时关闭）。
            let mono = cx.theme().mono_font_family.clone();
            ed.set_markdown_style(warm_theme::warm_light(mono), cx);

            // 图片：provider 只读缓存；解码在下方 ensure 中驱动。
            {
                let store = images.clone();
                ed.set_block_image_provider(move |src| store.borrow_mut().get(src));
            }
            // mermaid：provider 只读缓存；渲染在下方 ensure 中 off-thread 驱动。
            {
                let store = mermaid.clone();
                ed.set_block_mermaid_provider(move |source| {
                    store.borrow().get(&SharedString::from(source))
                });
            }
            ed
        });

        // EditorEvent 消费：SoloMD 的导航语义接线点（P1 剩余项）。
        // 设计注意：左键点图片发 PreviewImage 而非放置光标——编辑源码要
        // "点旁边 + 方向键移入"（reveal-on-caret 是光标驱动）。
        let cwd = std::env::current_dir().unwrap_or_default();
        cx.subscribe(&state, move |_, _, ev: &gpui_editor::EditorEvent, _cx| match ev {
            gpui_editor::EditorEvent::PreviewImage(src) => {
                println!("[wys] PreviewImage: {src} → 系统看图器");
                let p = cwd.join(src.trim_start_matches("./"));
                let _ = std::process::Command::new("open").arg(p).spawn();
            }
            gpui_editor::EditorEvent::OpenLink(url) => {
                println!("[wys] OpenLink: {url} → 系统浏览器");
                let _ = std::process::Command::new("open").arg(url.as_ref()).spawn();
            }
            gpui_editor::EditorEvent::OpenWikiLink(target) => {
                println!("[wys] OpenWikiLink: {target} →（真实应用里跳转笔记）");
            }
            _ => {}
        })
        .detach();
        let load_ms = t1.elapsed().as_millis();

        // 驱动首轮渲染/解码（zorite 模式：ensure 幂等占坑 → 后台完成 → 重绘）。
        Self::ensure_assets(&text, &mermaid, cx);

        println!(
            "[perf-zorite] doc = {:.1} MB / {} lines · load-into-editor {} ms{}",
            text.len() as f64 / 1024.0 / 1024.0,
            text.lines().count(),
            load_ms,
            if big { " （SPIKE_BIG 性能模式）" } else { " （目检模式）" }
        );

        Self { state, wys_off: false }
    }

    /// WYSIWYG 开关：off = 全量源码模式（编辑图片/公式源码的官方方式）。
    fn toggle_wys(&mut self, cx: &mut Context<Self>) {
        self.wys_off = !self.wys_off;
        self.state.update(cx, |ed, cx| {
            if self.wys_off {
                ed.clear_markdown_style(cx);
            } else {
                let mono = cx.theme().mono_font_family.clone();
                ed.set_markdown_style(warm_theme::warm_light(mono), cx);
            }
        });
        cx.notify();
    }

    /// 扫描文本中的 mermaid 围栏，占坑后离线程渲染，完成后重绘
    /// （zorite stores.rs 的 ensure_content_mermaid 精简版；图片由 provider 同步解码）。
    fn ensure_assets(
        text: &str,
        mermaid: &spike_mermaid::SharedMermaidStore,
        cx: &mut Context<Self>,
    ) {
        for source in gpui_editor::mermaid_sources(text) {
            if !mermaid.borrow_mut().begin(&source) {
                continue;
            }
            let theme_done = (); // 主题为常量，无需主线程构建
            let svg = cx.svg_renderer();
            let store = mermaid.clone();
            let src = source.to_string();
            cx.spawn(async move |this, cx| {
                let result = cx
                    .background_executor()
                    .spawn(async move {
                        spike_mermaid::render_to_image(&src, &svg, spike_mermaid::RASTER_SCALE)
                    })
                    .await;
                store.borrow_mut().finish(source, result);
                let _ = this.update(cx, |_, cx| {
                    cx.notify();
                    cx.refresh_windows();
                });
            })
            .detach();
            let _ = theme_done;
        }
    }
}

impl WysPanel {
    /// ⌘V：剪贴板有图片/图片文件 → 存 `_assets/` → 插入 markdown 引用。
    /// 否则 propagate 给编辑器原生文本粘贴（crate 的 paste 对图片剪贴板也放行）。
    fn on_paste_image(&mut self, _: &PasteImage, window: &mut Window, cx: &mut Context<Self>) {
        use gpui::{ClipboardEntry, ImageFormat};

        let clip_image = |cx: &mut Context<Self>| {
            cx.read_from_clipboard()?.entries().iter().find_map(|e| match e {
                ClipboardEntry::Image(img) => {
                    let ext = match img.format() {
                        ImageFormat::Png => "png",
                        ImageFormat::Jpeg => "jpg",
                        ImageFormat::Webp => "webp",
                        ImageFormat::Gif => "gif",
                        ImageFormat::Bmp => "bmp",
                        ImageFormat::Tiff => "tiff",
                        ImageFormat::Svg => "svg",
                        _ => "png",
                    };
                    Some((img.bytes().to_vec(), ext))
                }
                _ => None,
            })
        };
        let clip_files = |cx: &mut Context<Self>| {
            cx.read_from_clipboard()?.entries().iter().find_map(|e| match e {
                ClipboardEntry::ExternalPaths(p) if !p.paths().is_empty() => {
                    Some(p.paths().to_vec())
                }
                _ => None,
            })
        };

        // Finder 拷贝的文件
        if let Some(paths) = clip_files(cx) {
            let mut inserted = 0;
            for path in paths.iter().filter(|p| {
                p.extension()
                    .and_then(|e| e.to_str())
                    .map(|e| {
                        matches!(
                            e.to_lowercase().as_str(),
                            "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" | "svg"
                        )
                    })
                    .unwrap_or(false)
            }) {
                if let Some(rel) = import_to_assets(path) {
                    let md = format!("\n![{}]({})\n", file_stem(&rel), rel);
                    self.state.update(cx, |ed, cx| {
                        ed.replace_text_in_range(None, &md, window, cx);
                    });
                    inserted += 1;
                }
            }
            if inserted > 0 {
                cx.notify();
                return;
            }
            cx.propagate();
            return;
        }

        // 剪贴板位图（截图等）
        let Some((bytes, ext)) = clip_image(cx) else {
            cx.propagate();
            return;
        };
        let name = format!("paste-{}.{}", chrono_now_compact(), ext);
        let rel = format!("_assets/{name}");
        if std::fs::write(&rel, &bytes).is_ok() {
            let md = format!("\n![{name}]({rel})\n");
            self.state.update(cx, |ed, cx| {
                        ed.replace_text_in_range(None, &md, window, cx);
                    });
            println!("[wys] 粘贴图片已保存 {rel} ({} bytes)", bytes.len());
            cx.notify();
        } else {
            eprintln!("[wys] 粘贴图片写盘失败: {rel}");
            cx.propagate();
        }
    }
}

fn file_stem(p: &str) -> &str {
    std::path::Path::new(p)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("image")
}

fn chrono_now_compact() -> String {
    let d = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    format!("{}", d.as_secs())
}

/// 拷贝外部图片文件进 `_assets/`（内容寻址去重的 spike 简化版：按文件名）。
fn import_to_assets(src: &std::path::Path) -> Option<String> {
    std::fs::create_dir_all("_assets").ok()?;
    let name = src.file_name()?.to_str()?.to_string();
    let dest = std::path::Path::new("_assets").join(&name);
    if !dest.exists() {
        std::fs::copy(src, &dest).ok()?;
    }
    Some(format!("_assets/{name}"))
}

impl Render for WysPanel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        use gpui_component::button::{Button, ButtonVariants as _};
        use gpui_component::Sizable as _;
        v_flex()
            .size_full()
            .on_action(cx.listener(Self::on_paste_image))
            .child(
                gpui_component::h_flex()
                    .px_2()
                    .py_1()
                    .gap_2()
                    .child(
                        Button::new("toggle-wys")
                            .label(if self.wys_off { "WYSIWYG 关（源码模式）" } else { "WYSIWYG 开" })
                            .xsmall()
                            .ghost()
                            .on_click(cx.listener(|this, _, _, cx| this.toggle_wys(cx))),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child("独立图片=原子对象：⌫ 删除/右键菜单/角柄拖拽缩放；源码编辑用开关"),
                    ),
            )
            .child(
                div().flex_1().min_h_0().text_size(px(16.)).child(self.state.clone()),
            )
    }
}
