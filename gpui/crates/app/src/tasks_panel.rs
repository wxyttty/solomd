//! tasks_panel.rs — 全库任务右栏页。
//!
//! 数据源：solomd-core workspace_index 的 `TaskRef` 扫描（`- [ ]` / `- [x]`，
//! 跳过围栏与 front matter，行号为文件行号）。首次打开 lazy init 索引，之后
//! 由 index-updated 事件驱动刷新（watcher 增量重扫）。
//!
//! 交互：点击文本 → OpenNote 事件由 Workspace 执行（open_note_at 跳行）；
//! 点击复选框 → 重写文件该行的 `[ ]`/`[x]` 并落盘（watcher 自动带回新索引）。
//! 移植自上游 be333cf。

use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::Arc;

use gpui::{
    div, prelude::FluentBuilder as _, Context, EventEmitter, FocusHandle, Focusable,
    InteractiveElement as _, IntoElement, ParentElement as _, Render, SharedString,
    StatefulInteractiveElement as _, Styled as _, Window,
};
use gpui_component::{h_flex, v_flex, ActiveTheme as _, Sizable as _};
use solomd_core::deps::{ChannelSink, EventSink};
use solomd_core::workspace_index::{
    workspace_index_files, workspace_index_init, workspace_index_tasks, IndexCtx, TaskItem,
};

use crate::vault::Vault;
use crate::INDEX_INITED;

pub enum TaskEvent {
    /// 打开某笔记并跳到第 line 行（0 = 只打开）。
    OpenNote { path: PathBuf, line: u32 },
}

pub struct TasksPanel {
    focus_handle: FocusHandle,
    vault: Vault,
    tasks: Vec<TaskItem>,
    index_ready: bool,
    index_status: SharedString,
}

impl TasksPanel {
    pub fn new(vault: Vault, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let mut this = Self {
            focus_handle: cx.focus_handle(),
            vault,
            tasks: Vec::new(),
            index_ready: false,
            index_status: "索引未就绪".into(),
        };

        // index-updated 事件泵（info_panel 同款）。
        let (tx, rx) = std::sync::mpsc::channel::<(String, String)>();
        let weak = cx.entity().downgrade();
        let data_dir = this.vault.root.join(".solomd");
        cx.spawn_in(window, async move |_this, cx| {
            loop {
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(300))
                    .await;
                let Some(entity) = weak.upgrade() else { break };
                let mut events: Vec<String> = Vec::new();
                while let Ok((topic, _)) = rx.try_recv() {
                    events.push(topic);
                }
                if events.is_empty() {
                    continue;
                }
                let _ = entity.update(cx, |panel, cx| {
                    if events.iter().any(|t| t == "index-updated") {
                        panel.index_ready = true;
                        panel.index_status = "索引就绪".into();
                        panel.refresh(cx);
                    }
                    cx.notify();
                });
            }
        })
        .detach();

        if !INDEX_INITED.swap(true, Ordering::Relaxed) {
            this.index_status = "建索引中…".into();
            let folder = this.vault.root.to_string_lossy().to_string();
            let sink: Arc<dyn EventSink> = Arc::new(ChannelSink::new(tx));
            cx.spawn_in(window, async move |this, cx| {
                let r = cx
                    .background_executor()
                    .spawn(async move {
                        match solomd_core::rt::spawn(async move {
                            workspace_index_init(IndexCtx { sink, data_dir }, folder).await
                        })
                        .await
                        {
                            Ok(inner) => inner,
                            Err(e) => Err(format!("join: {e}")),
                        }
                    })
                    .await;
                let _ = this.update(cx, |panel, cx| {
                    panel.index_ready = true;
                    panel.index_status = match r {
                        Ok(n) => format!("索引就绪 · {n} 篇").into(),
                        Err(e) => format!("索引失败: {e}").into(),
                    };
                    panel.refresh(cx);
                    cx.notify();
                });
            })
            .detach();
        } else {
            this.index_ready = true;
        }
        this.refresh(cx);
        this
    }

    fn refresh(&mut self, cx: &mut Context<Self>) {
        if let Ok(list) = workspace_index_tasks() {
            self.tasks = list;
        }
        cx.notify();
    }

    /// 勾选切换：按文件行号重写 `[ ]` ↔ `[x]`（含 front matter 的文件行号）。
    /// 落盘后 watcher 增量重扫 → index-updated → 泵刷新列表。
    fn toggle(&mut self, ix: usize, cx: &mut Context<Self>) {
        let Some(item) = self.tasks.get(ix).cloned() else {
            return;
        };
        let path = PathBuf::from(&item.path);
        let Ok(raw) = std::fs::read_to_string(&path) else {
            self.index_status = "读取失败".into();
            cx.notify();
            return;
        };
        let mut lines: Vec<String> = raw.lines().map(|l| l.to_string()).collect();
        let idx = item.line as usize - 1;
        let Some(line) = lines.get_mut(idx) else {
            return;
        };
        if item.done {
            // [x] → [ ]
            if let Some(p) = line.find("[x]").or_else(|| line.find("[X]")) {
                line.replace_range(p..p + 3, "[ ]");
            }
        } else if let Some(p) = line.find("[ ]") {
            line.replace_range(p..p + 3, "[x]");
        }
        if let Err(e) = std::fs::write(&path, lines.join("\n") + "\n") {
            eprintln!("[tasks] 回写失败 {}: {e}", path.display());
            return;
        }
        eprintln!("[tasks] 勾选切换 {}#{}", path.display(), item.line);
    }

    fn render_task(&mut self, ix: usize, cx: &mut Context<Self>) -> gpui::AnyElement {
        let Some(item) = self.tasks.get(ix).cloned() else {
            return div().into_any_element();
        };
        let theme = cx.theme().clone();
        let name = item.name.clone();
        let checked = item.done;
        h_flex()
            .id(SharedString::from(format!("task-{ix}")))
            .items_center()
            .gap_2()
            .px_2()
            .py_1()
            .rounded_md()
            .hover(|s| s.bg(theme.accent.opacity(0.12)))
            .child(
                div()
                    .id(SharedString::from(format!("task-check-{ix}")))
                    .cursor_pointer()
                    .text_sm()
                    .text_color(if checked {
                        theme.success
                    } else {
                        theme.muted_foreground
                    })
                    .on_mouse_down(gpui::MouseButton::Left, cx.listener(move |this, _, _, cx| {
                        this.toggle(ix, cx);
                    }))
                    .child(if checked { "☑" } else { "☐" }),
            )
            .child(
                div()
                    .id(SharedString::from(format!("task-open-{ix}")))
                    .flex_1()
                    .min_w_0()
                    .cursor_pointer()
                    .text_sm()
                    .when(checked, |s| {
                        s.text_color(theme.muted_foreground).line_through()
                    })
                    .on_mouse_down(gpui::MouseButton::Left, cx.listener(move |this, _, _, cx| {
                        if let Some(item) = this.tasks.get(ix).cloned() {
                            cx.emit(TaskEvent::OpenNote {
                                path: PathBuf::from(&item.path),
                                line: item.line,
                            });
                        }
                    }))
                    .child(div().truncate().child(item.text.clone())),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child(name),
            )
            .into_any_element()
    }
}

impl Render for TasksPanel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let done = self.tasks.iter().filter(|t| t.done).count();
        v_flex()
            .size_full()
            .child(
                h_flex()
                    .px_3()
                    .py_2()
                    .justify_between()
                    .border_b_1()
                    .border_color(theme.border)
                    .child(
                        div()
                            .text_xs()
                            .text_color(theme.muted_foreground)
                            .child(format!(
                                "{} / {} 已完成 · {}",
                                done,
                                self.tasks.len(),
                                self.index_status
                            )),
                    ),
            )
            .child(
                div()
                    .id("tasks-scroll")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .p_1()
                    .children(if self.tasks.is_empty() {
                        vec![div()
                            .px_2()
                            .py_2()
                            .text_sm()
                            .text_color(theme.muted_foreground)
                            .child(if self.index_ready {
                                "没有任务——用 `- [ ] ` 在任意笔记里建一个"
                            } else {
                                "等索引就绪…"
                            })
                            .into_any_element()]
                    } else {
                        (0..self.tasks.len())
                            .map(|ix| self.render_task(ix, cx))
                            .collect()
                    }),
            )
    }
}

impl EventEmitter<TaskEvent> for TasksPanel {}

impl Focusable for TasksPanel {
    fn focus_handle(&self, _: &gpui::App) -> gpui::FocusHandle {
        self.focus_handle.clone()
    }
}
