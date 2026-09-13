//! file_tree.rs — vault 浏览面板：两级懒展开，点 .md 发 OpenNote 事件。
//!
//! P2 文件树操作包（上游 4.12.0 移植）：隐藏文件开关、扩展名过滤、
//! 选中行「重命名/移动」（core file_ops::fs_move_inner——移动 + `<stem>.assets/`
//! 随迁 + 全库链接重写一次完成）；Moved 事件由 Workspace 重指向已开标签。

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use gpui::{
    div, px, prelude::FluentBuilder as _, AnyElement, AppContext as _, ClickEvent, Context,
    EventEmitter, Focusable, InteractiveElement as _, IntoElement, ParentElement as _, Render,
    SharedString, StatefulInteractiveElement as _, Styled as _, Window,
};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::input::{Input, InputEvent, InputState};
use gpui_component::ActiveTheme as _;
use gpui_component::dock::{BasePanel, Panel, PanelEvent};
use gpui_component::{h_flex, Sizable as _};

use crate::vault::{Entry, Vault};

pub enum TreeEvent {
    OpenNote(PathBuf),
    /// 文件/目录已在磁盘上移动（链接重写由 core 完成）→ Workspace 重指向标签。
    Moved { old: PathBuf, new: PathBuf },
}

pub struct FileTreePanel {
    focus_handle: gpui::FocusHandle,
    vault: Vault,
    expanded: HashSet<PathBuf>,
    /// 显示 dotfile（target/node_modules 始终隐藏）。
    show_hidden: bool,
    /// 扩展名过滤（逗号分隔，如 "md,png"；空 = 不过滤；只作用于文件）。
    ext_filter: gpui::Entity<InputState>,
    /// 当前选中行（点击即选中；目录同时展开/收起）。
    selected: Option<PathBuf>,
    /// 重命名/移动进行中：输入框预填 vault 相对路径。
    renaming: bool,
    rename_input: gpui::Entity<InputState>,
}

impl FileTreePanel {
    pub fn new(vault: Vault, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let ext_filter = cx.new(|cx| InputState::new(window, cx).placeholder("类型过滤 md,png"));
        let rename_input = cx.new(|cx| InputState::new(window, cx));
        cx.subscribe_in(&ext_filter, window, |this, _, ev: &InputEvent, _, cx| {
            if matches!(ev, InputEvent::Change) {
                cx.notify();
            }
        })
        .detach();
        cx.subscribe_in(
            &rename_input,
            window,
            |this, _, ev: &InputEvent, window, cx| match ev {
                InputEvent::PressEnter { .. } => this.commit_rename(window, cx),
                _ => {}
            },
        )
        .detach();
        Self {
            focus_handle: cx.focus_handle(),
            vault,
            expanded: HashSet::new(),
            show_hidden: false,
            ext_filter,
            selected: None,
            renaming: false,
            rename_input,
        }
    }

    fn toggle(&mut self, path: PathBuf) {
        if !self.expanded.remove(&path) {
            self.expanded.insert(path);
        }
    }

    fn ext_pass(&self, cx: &Context<Self>, name: &str) -> bool {
        let f = self.ext_filter.read(cx).value().trim().to_lowercase();
        if f.is_empty() {
            return true;
        }
        let ext = name.rsplit('.').next().unwrap_or("").to_lowercase();
        f.split(',')
            .any(|e| !e.trim().is_empty() && e.trim() == ext)
    }

    fn rel_of(&self, p: &Path) -> PathBuf {
        p.strip_prefix(&self.vault.root).unwrap_or(p).to_path_buf()
    }

    /// 提交重命名/移动：core `fs_move_inner`（移动 + assets 随迁 + 链接重写）。
    /// 成功后发 Moved 让 Workspace 重指向已开标签。
    fn commit_rename(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.renaming = false;
        let Some(old) = self.selected.clone() else { return };
        let new_rel = self.rename_input.read(cx).value().trim().to_string();
        if new_rel.is_empty() {
            return;
        }
        let from = old.clone();
        let to = self.vault.root.join(&new_rel);
        let from_s = from.to_string_lossy().to_string();
        let to_s = to.to_string_lossy().to_string();
        cx.spawn_in(window, async move |this, cx| {
            let r = cx
                .background_executor()
                .spawn(async move {
                    solomd_core::fs_move_inner(from_s, to_s)
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.selected = None;
                match r {
                    Ok(()) => {
                        cx.emit(TreeEvent::Moved { old: from, new: to });
                    }
                    Err(e) => eprintln!("[tree] 移动失败: {e}"),
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn walk(
        &self,
        rel: &Path,
        depth: usize,
        rows: &mut Vec<AnyElement>,
        counter: &mut usize,
        cx: &mut Context<Self>,
    ) {
        if depth > 4 || rows.len() > 500 {
            return;
        }
        let Ok(entries) = self.vault.list_dir_ex(rel, self.show_hidden) else {
            return;
        };
        for Entry {
            path,
            name,
            is_dir,
        } in entries
        {
            if !is_dir && !self.ext_pass(cx, &name) {
                continue;
            }
            let expanded = is_dir && self.expanded.contains(&path);
            let glyph = if is_dir {
                if expanded { "▾ " } else { "▸ " }
            } else {
                "· "
            };
            *counter += 1;
            let selected = self.selected.as_ref() == Some(&path);
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
                    .when(selected, |s| s.bg(cx.theme().accent.opacity(0.12)))
                    .hover(|s| s.bg(cx.theme().accent.opacity(0.12)))
                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                        this.selected = Some(open_path.clone());
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
                let rel_child = self.rel_of(&path);
                self.walk(&rel_child, depth + 1, rows, counter, cx);
            }
        }
    }
}

impl BasePanel for FileTreePanel {
    fn panel_name(&self) -> &'static str {
        "file-tree"
    }
}

impl Panel for FileTreePanel {
    fn title(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        "笔记库".into_any_element()
    }
}

impl Render for FileTreePanel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let mut rows = Vec::new();
        let mut counter = 0usize;
        self.walk(Path::new(""), 0, &mut rows, &mut counter, cx);

        // 工具行：隐藏开关 + 类型过滤 + 重命名入口。
        let toolbar = h_flex()
            .px_2()
            .py_1()
            .gap_1()
            .border_b_1()
            .border_color(theme.border)
            .child(
                Button::new("tree-hidden")
                    .label(if self.show_hidden { "隐 ✓" } else { "隐" })
                    .xsmall()
                    .ghost()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.show_hidden = !this.show_hidden;
                        cx.notify();
                    })),
            )
            .child(div().flex_1().min_w_0().child(Input::new(&self.ext_filter).small()))
            .child(
                Button::new("tree-rename")
                    .label("✏")
                    .xsmall()
                    .ghost()
                                        .on_click(cx.listener(|this, _, window, cx| {
                        if let Some(sel) = this.selected.clone() {
                            let rel = this.rel_of(&sel);
                            this.renaming = true;
                            this.rename_input.update(cx, |s, cx| {
                                s.set_value(rel.to_string_lossy().as_ref(), window, cx)
                            });
                            cx.notify();
                        }
                    })),
            );

        // 重命名/移动行。
        let rename_row = self.renaming.then(|| {
            h_flex()
                .px_2()
                .py_1()
                .gap_1()
                .border_b_1()
                .border_color(theme.border)
                .child(div().text_xs().text_color(theme.muted_foreground).child("→"))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .child(Input::new(&self.rename_input).small()),
                )
                .child(
                    Button::new("tree-rename-ok")
                        .label("✓")
                        .xsmall()
                        .ghost()
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.commit_rename(window, cx);
                        })),
                )
        });

        div()
            .id("file-tree")
            .size_full()
            .overflow_y_scroll()
            .py_1()
            .child(toolbar)
            .children(rename_row)
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
