//! info_panel.rs — 大纲 / 反链 / 标签 三合一右栏页（内部小 tab 切换）。
//!
//! 数据源：
//! - 大纲：纯前端解析活跃笔记缓冲（跳过代码围栏内的 `#`），点击跳转光标
//! - 反链/标签：solomd-core 的 workspace_index（全局 STATE + notify watcher），
//!   首次打开本面板时 lazy init，之后由 index-updated 事件驱动刷新
//!
//! 跳转通过 `InfoEvent::OpenNote` 事件由 Workspace 执行（open_note + 光标定位）。

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use gpui::{
    div, px, prelude::FluentBuilder as _, Context, EventEmitter, FocusHandle,
    Focusable, FontWeight, InteractiveElement as _, IntoElement, ParentElement as _, Render,
    SharedString, StatefulInteractiveElement as _, Styled as _, Window,
};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::{h_flex, v_flex, ActiveTheme as _, Sizable as _};
use solomd_core::deps::{ChannelSink, EventSink};
use solomd_core::workspace_index::{
    workspace_index_backlinks, workspace_index_files, workspace_index_init,
    workspace_index_tags, BacklinkRef, IndexCtx, TagCount,
};

use crate::note_panel::NotePanel;
use crate::vault::Vault;

/// workspace_index 只初始化一次（单 vault 单进程）。
static INDEX_INITED: AtomicBool = AtomicBool::new(false);

#[derive(PartialEq, Clone, Copy)]
enum InfoTab {
    Outline,
    Backlinks,
    Tags,
}

pub enum InfoEvent {
    /// 打开某笔记并跳到第 line 行（0 = 只打开）。
    OpenNote { path: PathBuf, line: u32 },
}

/// 大纲节点：层级（# 个数）、标题文本、字节偏移。
struct OutlineNode {
    level: u8,
    text: String,
    offset: usize,
}

/// 编辑器缓冲中的工具调用卡（Transcript 里 one turn 的形态之一）。
pub struct InfoPanel {
    focus_handle: FocusHandle,
    vault: Vault,
    tab: InfoTab,
    active: Option<PathBuf>,
    active_panel: Option<gpui::WeakEntity<NotePanel>>,
    outline: Vec<OutlineNode>,
    backlinks: Vec<BacklinkRef>,
    tags: Vec<TagCount>,
    expanded_tag: Option<String>,
    index_ready: bool,
    index_status: SharedString,
}

impl InfoPanel {
    pub fn new(vault: Vault, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let mut this = Self {
            focus_handle: cx.focus_handle(),
            vault,
            tab: InfoTab::Outline,
            active: None,
            active_panel: None,
            outline: Vec::new(),
            backlinks: Vec::new(),
            tags: Vec::new(),
            expanded_tag: None,
            index_ready: false,
            index_status: "索引未就绪".into(),
        };

        // index-updated 事件泵：init 完成与 watcher 增量重扫都走这里。
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
                        // workspace_index_init 是 tokio 桥接 async（内部
                        // spawn_blocking + watcher），经 rt 桥接执行。
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
                let _ = this.update_in(cx, |panel, _, cx| {
                    if let Err(e) = r {
                        panel.index_status = SharedString::from(format!("索引失败：{e}"));
                    }
                    cx.notify();
                });
            })
            .detach();
        } else {
            // 已经建过（面板重开）：直接可用。
            this.index_ready = !workspace_index_files().unwrap_or_default().is_empty();
            this.index_status = if this.index_ready { "索引就绪" } else { "索引为空" }.into();
        }

        this.refresh(cx);
        this
    }

    /// Workspace 在笔记打开/激活时推送。
    pub fn set_active(
        &mut self,
        path: Option<PathBuf>,
        panel: Option<gpui::WeakEntity<NotePanel>>,
        cx: &mut Context<Self>,
    ) {
        if self.active == path {
            return;
        }
        self.active = path;
        self.active_panel = panel;
        self.refresh(cx);
    }

    fn refresh(&mut self, cx: &mut Context<Self>) {
        self.refresh_outline(cx);
        self.refresh_index(cx);
    }

    /// 大纲：读活跃笔记的编辑器缓冲（含未保存编辑），跳过围栏。
    fn refresh_outline(&mut self, cx: &mut Context<Self>) {
        self.outline.clear();
        let Some(panel) = self.active_panel.as_ref().and_then(|w| w.upgrade()) else {
            return;
        };
        let text = panel.read(cx).state.read(cx).text().to_string();
        let mut in_fence = false;
        let mut offset = 0usize;
        for line in text.lines() {
            let line_len = line.len() + 1; // +1 for '\n'（末行差 1，无碍）
            let trimmed = line.trim_start();
            if trimmed.starts_with("```") {
                in_fence = !in_fence;
            } else if !in_fence {
                let hashes = trimmed.bytes().take_while(|b| *b == b'#').count();
                if (1..=6).contains(&hashes)
                    && trimmed[hashes..].starts_with(' ')
                    && line.starts_with('#')
                {
                    self.outline.push(OutlineNode {
                        level: hashes as u8,
                        text: trimmed[hashes..].trim().to_string(),
                        offset,
                    });
                }
            }
            offset += line_len;
        }
    }

    /// 反链/标签：读 workspace_index 全局 STATE（同步、内存内）。
    fn refresh_index(&mut self, _cx: &mut Context<Self>) {
        if !INDEX_INITED.load(Ordering::Relaxed) {
            return;
        }
        self.backlinks = match self.active.as_ref().map(|p| {
            p.file_stem()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_default()
        }) {
            Some(stem) if !stem.is_empty() => {
                workspace_index_backlinks(stem).unwrap_or_default()
            }
            _ => Vec::new(),
        };
        self.tags = workspace_index_tags().unwrap_or_default();
    }
}

impl EventEmitter<InfoEvent> for InfoPanel {}

impl InfoPanel {
    fn render_outline(&mut self, cx: &mut Context<Self>) -> gpui::AnyElement {
        if self.active.is_none() {
            return div()
                .text_sm()
                .text_color(cx.theme().muted_foreground)
                .child("没有打开的笔记")
                .into_any_element();
        }
        if self.outline.is_empty() {
            return div()
                .text_sm()
                .text_color(cx.theme().muted_foreground)
                .child("没有标题")
                .into_any_element();
        }
        let mut list = v_flex().gap_0p5();
        for node in &self.outline {
            let indent = (node.level as f32 - 1.) * 14.;
            let offset = node.offset;
            list = list.child(
                div()
                    .id(SharedString::from(format!("ol-{}", node.offset)))
                    .pl(px(indent))
                    .pr_2()
                    .py_0p5()
                    .rounded_sm()
                    .text_sm()
                    .cursor_pointer()
                    .truncate()
                    .hover(|s| s.bg(cx.theme().accent.opacity(0.12)))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        if let Some(panel) = this.active_panel.clone() {
                            let _ = panel.update_in(cx, |np, window, cx| {
                                np.jump_to_offset(offset, window, cx);
                            });
                        }
                        let _ = window;
                    }))
                    .child(node.text.clone()),
            );
        }
        list.into_any_element()
    }

    fn render_backlinks(&mut self, cx: &mut Context<Self>) -> gpui::AnyElement {
        if !self.index_ready {
            return div()
                .text_sm()
                .text_color(cx.theme().muted_foreground)
                .child(self.index_status.clone())
                .into_any_element();
        }
        if self.backlinks.is_empty() {
            return div()
                .text_sm()
                .text_color(cx.theme().muted_foreground)
                .child("没有反链（其他笔记里的 [[链接]] 会出现在这里）")
                .into_any_element();
        }
        let mut list = v_flex().gap_1p5();
        for b in &self.backlinks {
            let path = PathBuf::from(&b.from_path);
            let line = b.line;
            let ctx_lines: Vec<String> = b
                .context
                .iter()
                .map(|l| l.chars().take(120).collect())
                .collect();
            list = list.child(
                v_flex()
                    .px_2()
                    .py_1p5()
                    .rounded_md()
                    .border_1()
                    .border_color(cx.theme().border)
                    .gap_1()
                    .child(
                        div()
                            .id(SharedString::from(format!("bl-{}", b.from_path)))
                            .text_sm()
                            .font_weight(FontWeight::MEDIUM)
                            .cursor_pointer()
                            .hover(|s| s.text_color(cx.theme().accent))
                            .on_click(cx.listener(move |_this, _, _, cx| {
                                cx.emit(InfoEvent::OpenNote {
                                    path: path.clone(),
                                    line,
                                });
                            }))
                            .child(b.from_name.clone()),
                    )
                    .child(
                        v_flex()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .font_family("Menlo")
                            .children(ctx_lines.into_iter().map(|l| {
                                div().child(l).into_any_element()
                            })),
                    ),
            );
        }
        list.into_any_element()
    }

    fn render_tags(&mut self, cx: &mut Context<Self>) -> gpui::AnyElement {
        if !self.index_ready {
            return div()
                .text_sm()
                .text_color(cx.theme().muted_foreground)
                .child(self.index_status.clone())
                .into_any_element();
        }
        if self.tags.is_empty() {
            return div()
                .text_sm()
                .text_color(cx.theme().muted_foreground)
                .child("没有标签（front matter tags 或正文 #tag）")
                .into_any_element();
        }
        let expanded = self.expanded_tag.clone();
        let mut list = v_flex().gap_1();
        for t in &self.tags {
            let is_open = expanded.as_deref() == Some(t.tag.as_str());
            let tag_row = t.clone();
            list = list.child(
                v_flex()
                    .rounded_md()
                    .border_1()
                    .border_color(cx.theme().border)
                    .child(
                        div()
                            .id(SharedString::from(format!("tag-{}", t.tag)))
                            .px_2()
                            .py_1p5()
                            .flex()
                            .items_center()
                            .justify_between()
                            .cursor_pointer()
                            .hover(|s| s.bg(cx.theme().accent.opacity(0.12)))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.expanded_tag = if this.expanded_tag.as_deref()
                                    == Some(tag_row.tag.as_str())
                                {
                                    None
                                } else {
                                    Some(tag_row.tag.clone())
                                };
                                cx.notify();
                            }))
                            .child(
                                div().text_sm().child(t.tag.clone()),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(format!("{}", t.count)),
                            ),
                    )
                    .when(is_open, |this| {
                        this.child(
                            v_flex()
                                .px_2()
                                .pb_1p5()
                                .gap_0p5()
                                .children(t.files.iter().map(|f| {
                                    let path = PathBuf::from(f);
                                    let name = path
                                        .file_name()
                                        .map(|n| n.to_string_lossy().to_string())
                                        .unwrap_or_else(|| f.clone());
                                    div()
                                        .id(SharedString::from(format!("tagf-{}", f)))
                                        .text_xs()
                                        .cursor_pointer()
                                        .hover(|s| s.text_color(cx.theme().accent))
                                        .on_click(cx.listener(move |_, _, _, cx| {
                                            cx.emit(InfoEvent::OpenNote {
                                                path: path.clone(),
                                                line: 0,
                                            });
                                        }))
                                        .child(name.clone())
                                        .into_any_element()
                                })),
                        )
                    }),
            );
        }
        list.into_any_element()
    }
}

impl Render for InfoPanel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let (outline_sel, bl_sel, tags_sel) = (
            self.tab == InfoTab::Outline,
            self.tab == InfoTab::Backlinks,
            self.tab == InfoTab::Tags,
        );
        let title = match self.tab {
            InfoTab::Outline => format!(
                "大纲 · {}",
                self.active
                    .as_ref()
                    .and_then(|p| p.file_name().map(|n| n.to_string_lossy().to_string()))
                    .unwrap_or_default()
            ),
            InfoTab::Backlinks => format!("反链 · {}", self.backlinks.len()),
            InfoTab::Tags => format!("标签 · {}", self.tags.len()),
        };

        let body = match self.tab {
            InfoTab::Outline => self.render_outline(cx),
            InfoTab::Backlinks => self.render_backlinks(cx),
            InfoTab::Tags => self.render_tags(cx),
        };

        v_flex()
            .size_full()
            .child(
                h_flex()
                    .px_3()
                    .py_1p5()
                    .gap_2()
                    .border_b_1()
                    .border_color(cx.theme().border)
                    .child(div().font_weight(FontWeight::SEMIBOLD).child("信息"))
                    .child(
                        div()
                            .text_xs()
                            .flex_1()
                            .truncate()
                            .text_color(cx.theme().muted_foreground)
                            .child(title),
                    )
                    .child(
                        Button::new("info-refresh")
                            .label("刷新")
                            .xsmall()
                            .ghost()
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.refresh(cx);
                            })),
                    ),
            )
            .child(
                h_flex()
                    .px_3()
                    .py_1()
                    .gap_1()
                    .border_b_1()
                    .border_color(cx.theme().border)
                    .child(
                        Button::new("tab-outline")
                            .label("大纲")
                            .xsmall()
                            .when(outline_sel, |b| b.primary())
                            .when(!outline_sel, |b| b.ghost())
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.tab = InfoTab::Outline;
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new("tab-backlinks")
                            .label("反链")
                            .xsmall()
                            .when(bl_sel, |b| b.primary())
                            .when(!bl_sel, |b| b.ghost())
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.tab = InfoTab::Backlinks;
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new("tab-tags")
                            .label("标签")
                            .xsmall()
                            .when(tags_sel, |b| b.primary())
                            .when(!tags_sel, |b| b.ghost())
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.tab = InfoTab::Tags;
                                cx.notify();
                            })),
                    ),
            )
            .child(
                div()
                    .id("info-scroll")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .p_2()
                    .child(body),
            )
    }
}

impl Focusable for InfoPanel {
    fn focus_handle(&self, _: &gpui::App) -> FocusHandle {
        self.focus_handle.clone()
    }
}
