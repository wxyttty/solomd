//! file_tree.rs — vault 浏览面板：两级懒展开，点 .md 发 OpenNote 事件。

use std::collections::HashSet;
use std::path::PathBuf;

use gpui::{
    div, px, AnyElement, ClickEvent, Context, EventEmitter, Focusable, InteractiveElement as _,
    IntoElement, ParentElement as _, Render, SharedString, StatefulInteractiveElement as _,
    Styled as _, Window,
};
use gpui_component::ActiveTheme as _;
use gpui_component::dock::{Panel, PanelEvent};

use crate::vault::{Entry, Vault};

pub enum TreeEvent {
    OpenNote(PathBuf),
}

pub struct FileTreePanel {
    focus_handle: gpui::FocusHandle,
    vault: Vault,
    expanded: HashSet<PathBuf>,
}

impl FileTreePanel {
    pub fn new(vault: Vault, _: &mut Window, cx: &mut Context<Self>) -> Self {
        Self {
            focus_handle: cx.focus_handle(),
            vault,
            expanded: HashSet::new(),
        }
    }

    fn toggle(&mut self, path: PathBuf) {
        if !self.expanded.remove(&path) {
            self.expanded.insert(path);
        }
    }

    fn walk(
        &self,
        rel: &std::path::Path,
        depth: usize,
        rows: &mut Vec<AnyElement>,
        counter: &mut usize,
        cx: &mut Context<Self>,
    ) {
        if depth > 4 || rows.len() > 500 {
            return;
        }
        let Ok(entries) = self.vault.list_dir(rel) else {
            return;
        };
        for Entry {
            path,
            name,
            is_dir,
        } in entries
        {
            let expanded = is_dir && self.expanded.contains(&path);
            let glyph = if is_dir {
                if expanded { "▾ " } else { "▸ " }
            } else {
                "· "
            };
            *counter += 1;
            let id = SharedString::from(format!("tree-{}", *counter));
            let open_path = path.clone();

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
                    .hover(|s| s.bg(cx.theme().accent.opacity(0.12)))
                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                        if open_path.is_dir() {
                            this.toggle(open_path.clone());
                        } else {
                            cx.emit(TreeEvent::OpenNote(open_path.clone()));
                        }
                    }))
                    .child(glyph.to_string())
                    .child(name)
                    .into_any_element(),
            );

            if expanded {
                let rel_child = path
                    .strip_prefix(self.vault.root.as_path())
                    .unwrap_or(&path)
                    .to_path_buf();
                self.walk(&rel_child, depth + 1, rows, counter, cx);
            }
        }
    }
}

impl Panel for FileTreePanel {
    fn panel_name(&self) -> &'static str {
        "file-tree"
    }

    fn title(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        "笔记库".into_any_element()
    }
}

impl Render for FileTreePanel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let mut rows = Vec::new();
        let mut counter = 0usize;
        self.walk(std::path::Path::new(""), 0, &mut rows, &mut counter, cx);
        div()
            .id("file-tree")
            .size_full()
            .overflow_y_scroll()
            .py_1()
            .children(rows)
    }
}

impl EventEmitter<TreeEvent> for FileTreePanel {}
impl EventEmitter<PanelEvent> for FileTreePanel {}

impl Focusable for FileTreePanel {
    fn focus_handle(&self, _: &gpui::App) -> gpui::FocusHandle {
        self.focus_handle.clone()
    }
}
