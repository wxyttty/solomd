//! search_panel.rs — 搜索右栏页：语义搜索（rag）+ 全文兜底 + 索引重建。

use std::path::PathBuf;

use gpui::{
    div, AppContext as _, Context, Entity, FocusHandle, Focusable, FontWeight,
    InteractiveElement as _, IntoElement, ParentElement as _, Render, SharedString,
    StatefulInteractiveElement as _, Styled as _, Window,
};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::input::{Input, InputState};
use gpui_component::{h_flex, v_flex, ActiveTheme as _, Sizable as _};
use solomd_core::rag;

pub struct SearchPanel {
    focus_handle: FocusHandle,
    vault_root: PathBuf,
    query: Entity<InputState>,
    hits: Vec<rag::RagHit>,
    status: SharedString,
}

impl SearchPanel {
    pub fn new(vault_root: PathBuf, window: &mut Window, cx: &mut Context<Self>) -> Self {
        Self {
            focus_handle: cx.focus_handle(),
            vault_root,
            query: cx.new(|cx| InputState::new(window, cx).placeholder("输入关键词…")),
            hits: Vec::new(),
            status: "就绪".into(),
        }
    }

    fn do_search(&mut self, _: &gpui::ClickEvent, window: &mut Window, cx: &mut Context<Self>) {
        let query = self.query.read(cx).value().to_string();
        if query.trim().is_empty() {
            return;
        }
        self.status = "搜索中…".into();
        let folder = self.vault_root.to_string_lossy().to_string();

        cx.spawn_in(window, async move |this, cx| {
            // 语义优先；索引未建/失败时回退全文（search_in_dir_inner）。
            let hits = match solomd_core::rag::rag_search_inner(
                folder.clone(),
                query.clone(),
                20,
            ) {
                Ok(h) if !h.is_empty() => h,
                _ => match solomd_core::search::search_in_dir_inner(folder, query, 20) {
                    Ok(full) => full
                        .into_iter()
                        .map(|h| rag::RagHit {
                            path: h.file.clone(),
                            name: h.file.rsplit('/').next().unwrap_or(&h.file).to_string(),
                            chunk_idx: h.line as u32,
                            char_start: 0,
                            char_end: 0,
                            score: 0.0,
                            snippet: format!("{}…", h.snippet),
                        })
                        .collect(),
                    Err(e) => {
                        let _ = this.update_in(cx, |panel, window, cx| {
                            panel.status = SharedString::from(format!("错误：{e}"));
                            window.refresh();
                        });
                        return;
                    }
                },
            };
            let n = hits.len();
            let _ = this.update_in(cx, |panel, window, cx| {
                panel.status = SharedString::from(format!("{n} 条结果"));
                panel.hits = hits;
                window.refresh();
            });
        })
        .detach();
    }

    fn reindex(&mut self, _: &gpui::ClickEvent, window: &mut Window, cx: &mut Context<Self>) {
        self.status = "重建索引中…".into();
        let folder = self.vault_root.to_string_lossy().to_string();
        cx.spawn_in(window, async move |this, cx| {
            let r = cx
                .background_executor()
                .spawn(async move { rag::rag_reindex_inner(folder) })
                .await;
            let _ = this.update_in(cx, |panel, window, cx| match r {
                Ok(st) => {
                    panel.status =
                        SharedString::from(format!("索引：{}/{} 文件", st.indexed_files, st.total_files));
                    window.refresh();
                    cx.notify();
                }
                Err(e) => {
                    panel.status = SharedString::from(format!("索引失败：{e}"));
                    window.refresh();
                    cx.notify();
                }
            });
        })
        .detach();
    }
}

impl Render for SearchPanel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let mut results: Vec<gpui::AnyElement> = Vec::new();
        for (ix, hit) in self.hits.iter().enumerate() {
            results.push(
                div()
                    .id(SharedString::from(format!("hit-{ix}")))
                    .px_2()
                    .py_1p5()
                    .rounded_md()
                    .hover(|s| s.bg(cx.theme().accent.opacity(0.12)))
                    .child(
                        div()
                            .text_sm()
                            .font_weight(FontWeight::MEDIUM)
                            .child(format!(
                                "{} · {}",
                                hit.name,
                                if hit.score > 0.0 {
                                    format!("相关度 {:.2}", hit.score)
                                } else {
                                    format!("行 {}", hit.chunk_idx + 1)
                                }
                            )),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(hit.snippet.chars().take(140).collect::<String>()),
                    ),
            );
        }

        v_flex()
            .size_full()
            .child(
                h_flex()
                    .px_3()
                    .py_1p5()
                    .gap_2()
                    .border_b_1()
                    .border_color(cx.theme().border)
                    .child(div().font_weight(FontWeight::SEMIBOLD).child("搜索"))
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(self.status.clone()),
                    ),
            )
            .child(div().p_2().flex().flex_col().gap_2().child(
                h_flex()
                    .gap_1p5()
                    .child(Input::new(&self.query).flex_1().small())
                    .child(
                        Button::new("search-go")
                            .label("搜")
                            .xsmall()
                            .primary()
                            .on_click(cx.listener(Self::do_search)),
                    )
                    .child(
                        Button::new("search-reindex")
                            .label("重建索引")
                            .xsmall()
                            .ghost()
                            .on_click(cx.listener(Self::reindex)),
                    ),
            ))
            .child(
                div()
                    .id("search-results")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .px_2()
                    .pb_2()
                    .children(results),
            )
    }
}

impl Focusable for SearchPanel {
    fn focus_handle(&self, _: &gpui::App) -> gpui::FocusHandle {
        self.focus_handle.clone()
    }
}
