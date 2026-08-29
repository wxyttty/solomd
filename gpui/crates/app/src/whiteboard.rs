//! whiteboard.rs — 原生白板独立窗口（gpui-whiteboard 承载，替代原 tldraw webview）。
//!
//! - 打开入口是 NotePanel 的「白板」按钮（无围栏时自动在文末补一个）；
//! - 围栏 info 行 `​```whiteboard id="…" height="…"`，体 = Scene JSON（空 `{}` = 空板）；
//! - 初始快照取自活跃笔记的编辑器缓冲；
//! - 画板 ChangeFn 每手势回传全场景 JSON（crate 内已按手势批量）→ 重写围栏体
//!   （info 行逐字节保留）→ 直接落盘；回写走 NotePanel（编辑器文本 +
//!   vault.write_note），标签不进脏态。
//!
//! 主题：WhiteboardStyleFn 闭包签名拿不到 cx（paint 频率调用），与 zorite 同款
//! 模式改为读静态快照——`sync_style` 在开窗与 Workspace 主题切换时把 cx.theme()
//! 映射进 `STYLE`。

use std::ops::Range;
use std::rc::Rc;
use std::sync::{OnceLock, RwLock};

use gpui::{
    AppContext as _, Context, Entity, Focusable, IntoElement, Render, SharedString,
    TitlebarOptions, Window, WindowOptions,
};
use gpui_component::{ActiveTheme as _, Root};
use gpui_whiteboard::{Scene, WhiteboardStyle, WhiteboardStyleFn, WhiteboardView};

use crate::note_panel::NotePanel;

/// 打开（或聚焦语义上的）白板窗口：NotePanel「白板」按钮调用。
pub fn open_board(
    note: gpui::WeakEntity<NotePanel>,
    note_title: String,
    initial_snapshot: String,
    cx: &mut gpui::App,
) {
    sync_style(cx);
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
            let view =
                cx.new(|cx| BoardPanel::new(note.clone(), initial_snapshot.clone(), window, cx));
            // 不烘焙 bg：Root::render 每帧按 cx.theme() 刷背景。
            cx.new(|cx| Root::new(view, window, cx))
        },
    );
}

/// 白板围栏定位：返回 (info 行范围, body 范围)。
/// 反引号长度自适应（围栏头与闭合长度 >= 头部才闭合），
/// 容忍 Scene JSON 产出里出现普通 ``` 行。
pub(crate) fn find_board_fence(md: &str) -> Option<(Range<usize>, Range<usize>)> {
    let info_start = md.find("```whiteboard")?;
    let ticks = md[info_start..].chars().take_while(|c| *c == '`').count();
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
// 主题快照：闭包拿不到 cx，读这里；写点 = open_board + Workspace 主题切换。
// ---------------------------------------------------------------------------

static STYLE: OnceLock<RwLock<Option<WhiteboardStyle>>> = OnceLock::new();

/// 把当前 cx.theme() 映射成画板样式快照。
pub fn sync_style(cx: &gpui::App) {
    let t = cx.theme();
    let style = WhiteboardStyle {
        bg: t.background,
        grid: t.border,
        text: t.muted_foreground,
        ink: t.foreground,
        panel: t.popover,
        // 弹出面（取色器/菜单）要压得住花画布：略提透明度上限保持玻璃感。
        panel_strong: gpui::Hsla {
            a: 0.92,
            ..t.popover
        },
        accent: t.accent,
        selection: t.primary,
        // 取色器快捷色：中性（墨色→浅灰）+ 主题色 + 一组固定色相铺开。
        swatches: vec![
            t.foreground,
            gpui::hsla(0.0, 0.0, 0.45, 1.0),
            gpui::hsla(0.0, 0.0, 0.92, 1.0),
            t.primary,
            t.danger,
            t.success,
            gpui::hsla(0.075, 1.0, 0.625, 1.0), // 品牌橙 #ff9f40
            gpui::hsla(0.14, 0.80, 0.52, 1.0),  // 琥珀
            gpui::hsla(0.33, 0.55, 0.45, 1.0),  // 绿
            gpui::hsla(0.52, 0.65, 0.50, 1.0),  // 青
            gpui::hsla(0.60, 0.70, 0.55, 1.0),  // 蓝
            gpui::hsla(0.75, 0.55, 0.58, 1.0),  // 紫
        ],
    };
    let lock = STYLE.get_or_init(|| RwLock::new(None));
    if let Ok(mut slot) = lock.write() {
        *slot = Some(style);
    }
}

/// 主题闭包：读快照；未同步过时给中性兜底（理论不可达——开窗前必 sync）。
fn board_style() -> WhiteboardStyleFn {
    Rc::new(|| {
        STYLE
            .get()
            .and_then(|l| l.read().ok().and_then(|s| s.clone()))
            .unwrap_or_else(|| WhiteboardStyle {
                bg: gpui::hsla(0.0, 0.0, 0.98, 1.0),
                grid: gpui::hsla(0.0, 0.0, 0.85, 1.0),
                text: gpui::hsla(0.0, 0.0, 0.45, 1.0),
                ink: gpui::hsla(0.0, 0.0, 0.1, 1.0),
                panel: gpui::hsla(0.0, 0.0, 1.0, 0.9),
                panel_strong: gpui::hsla(0.0, 0.0, 1.0, 0.97),
                accent: gpui::hsla(0.075, 1.0, 0.625, 1.0),
                selection: gpui::hsla(0.58, 0.9, 0.55, 1.0),
                swatches: vec![],
            })
    })
}

// ---------------------------------------------------------------------------
// BoardPanel：独立窗口根实体，承载 WhiteboardView 并做围栏回写。
// ---------------------------------------------------------------------------

pub struct BoardPanel {
    board: Entity<WhiteboardView>,
    note: gpui::WeakEntity<NotePanel>,
    last_written: Option<String>,
}

impl BoardPanel {
    pub fn new(
        note: gpui::WeakEntity<NotePanel>,
        initial_snapshot: String,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let board = cx.new(|cx| {
            WhiteboardView::new(Scene::from_json(&initial_snapshot), board_style(), cx)
        });
        // 回写链：ChangeFn（手势提交）→ BoardPanel 去重 → NotePanel 围栏手术落盘。
        // 捕 WeakEntity 防环；调用点在 WhiteboardView 事件处理内，只碰别的实体。
        let weak = cx.entity().downgrade();
        board.update(cx, |view, _| {
            view.set_on_change(Rc::new(move |json: String, _window, cx: &mut gpui::App| {
                if let Some(panel) = weak.upgrade() {
                    panel.update(cx, |p, cx| p.write_back(json, cx));
                }
            }));
        });
        Self {
            board,
            note,
            last_written: None,
        }
    }

    /// 每手势一次：内容没变不回写。
    fn write_back(&mut self, body: String, cx: &mut Context<Self>) {
        if self.last_written.as_deref() == Some(body.as_str()) {
            return;
        }
        let wrote = self.note.upgrade().map(|np| {
            np.update(cx, |n, cx| n.write_board_snapshot(&body, cx));
        });
        if wrote.is_some() {
            self.last_written = Some(body);
        }
    }
}

impl Render for BoardPanel {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        self.board.clone()
    }
}

impl Focusable for BoardPanel {
    fn focus_handle(&self, cx: &gpui::App) -> gpui::FocusHandle {
        cx.focus_handle()
    }
}
