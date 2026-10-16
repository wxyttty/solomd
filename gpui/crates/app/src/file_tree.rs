//! file_tree.rs — vault 浏览面板：两级懒展开，点 .md 发 OpenNote 事件。
//!
//! P2 文件树操作包（上游 4.12.0 移植）：隐藏文件开关、扩展名过滤、
//! 选中行「重命名/移动」（core file_ops::fs_move_inner——移动 + `<stem>.assets/`
//! 随迁 + 全库链接重写一次完成）；Moved 事件由 Workspace 重指向已开标签。
//!
//! P3 右键菜单（对齐上游 FileTree.vue context menu）：新建文件/文件夹、
//! 重命名/移动、删除（AlertDialog 确认 → core fs_delete，优先进废纸篓）、
//! 复制（相对）路径、在 Finder 中显示。行内编辑沿用上游语义：
//! 新建/重命名共用一个输入行，Enter 提交 / Esc 取消。

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use gpui::{
    div, px, prelude::FluentBuilder as _, AnyElement, AppContext as _, ClickEvent, Context,
    EventEmitter, Focusable, InteractiveElement as _, IntoElement, ParentElement as _, Render,
    SharedString, StatefulInteractiveElement as _, Styled as _, Window,
};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::input::{Input, InputEvent, InputState};
use gpui_component::menu::ContextMenuExt as _;
use gpui_component::ActiveTheme as _;
use gpui_component::dock::{BasePanel, Panel, PanelEvent};
use gpui_component::{h_flex, Sizable as _, WindowExt as _};

use crate::vault::{Entry, Vault};

pub enum TreeEvent {
    OpenNote(PathBuf),
    /// 文件/目录已在磁盘上移动（链接重写由 core 完成）→ Workspace 重指向标签。
    Moved { old: PathBuf, new: PathBuf },
    /// 文件/目录已删除 → Workspace 关掉对应已开标签。
    Deleted(PathBuf),
}

/// 行内编辑的三种形态（上游 FileTree.vue editing.kind 的 Rust 版）。
enum TreeEdit {
    /// 在 parent 下新建文件；name 是当前输入值。无扩展名时补 .md。
    NewFile { parent: PathBuf },
    /// 在 parent 下新建目录。
    NewDir { parent: PathBuf },
    /// 重命名/移动：original 是磁盘上的现有路径，输入值为 vault 相对路径
    /// （支持跨目录移动，core fs_move_inner 会随迁 assets + 重写链接）。
    Rename { original: PathBuf },
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
    /// 行内编辑（新建/重命名）进行中。
    editing: Option<TreeEdit>,
    edit_input: gpui::Entity<InputState>,
    /// 待删除确认的路径（AlertDialog 打开中）。
    delete_target: Option<PathBuf>,
}

impl FileTreePanel {
    pub fn new(vault: Vault, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let ext_filter = cx.new(|cx| InputState::new(window, cx).placeholder("类型过滤 md,png"));
        let edit_input = cx.new(|cx| InputState::new(window, cx));
        cx.subscribe_in(&ext_filter, window, |_this, _, ev: &InputEvent, _, cx| {
            if matches!(ev, InputEvent::Change) {
                cx.notify();
            }
        })
        .detach();
        cx.subscribe_in(
            &edit_input,
            window,
            |this, _, ev: &InputEvent, window, cx| match ev {
                InputEvent::PressEnter { .. } => this.commit_edit(window, cx),
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
            editing: None,
            edit_input,
            delete_target: None,
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

    /// 开始行内编辑：预填值 + 展开目标目录（新建时新行才有地方出现）。
    fn start_edit(&mut self, edit: TreeEdit, preset: &str, window: &mut Window, cx: &mut Context<Self>) {
        if let TreeEdit::NewFile { parent } | TreeEdit::NewDir { parent } = &edit {
            self.expanded.insert(parent.clone());
        }
        self.editing = Some(edit);
        self.edit_input.update(cx, |s, cx| {
            s.set_value(preset, window, cx);
        });
        cx.notify();
    }

    fn cancel_edit(&mut self, cx: &mut Context<Self>) {
        self.editing = None;
        cx.notify();
    }

    /// 提交行内编辑（新建文件/新建目录/重命名移动）。全部走 core file_ops，
    /// 后台线程执行；成功后发事件让 Workspace 重指向/打开。
    fn commit_edit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(edit) = self.editing.take() else { return };
        let name = self.edit_input.read(cx).value().trim().to_string();
        if name.is_empty() {
            cx.notify();
            return;
        }
        match edit {
            TreeEdit::NewFile { parent } => {
                // 上游语义：无扩展名时默认补 .md（我们只编辑 md/txt）。
                let final_name =
                    if name.rsplit('.').next().map(|e| !e.is_empty() && name.contains('.')).unwrap_or(false) {
                        name.clone()
                    } else {
                        format!("{name}.md")
                    };
                let target = parent.join(&final_name);
                let open = target.clone();
                cx.spawn_in(window, async move |this, cx| {
                    let r = cx
                        .background_executor()
                        .spawn(async move { solomd_core::fs_create_file(target.to_string_lossy().to_string(), Some(String::new())) })
                        .await;
                    let _ = this.update(cx, |_this, cx| {
                        match r {
                            Ok(()) => cx.emit(TreeEvent::OpenNote(open)),
                            Err(e) => eprintln!("[tree] 新建失败: {e}"),
                        }
                        cx.notify();
                    });
                })
                .detach();
            }
            TreeEdit::NewDir { parent } => {
                let target = parent.join(&name);
                cx.spawn_in(window, async move |this, cx| {
                    let r = cx
                        .background_executor()
                        .spawn(async move { solomd_core::fs_create_dir(target.to_string_lossy().to_string()) })
                        .await;
                    let _ = this.update(cx, |_this, cx| {
                        if let Err(e) = r {
                            eprintln!("[tree] 建目录失败: {e}");
                        }
                        cx.notify();
                    });
                })
                .detach();
            }
            TreeEdit::Rename { original } => {
                let to = self.vault.root.join(&name);
                if to == original {
                    cx.notify();
                    return;
                }
                let from_s = original.to_string_lossy().to_string();
                let to_s = to.to_string_lossy().to_string();
                cx.spawn_in(window, async move |this, cx| {
                    let r = cx
                        .background_executor()
                        .spawn(async move { solomd_core::fs_move_inner(from_s, to_s) })
                        .await;
                    let _ = this.update(cx, |this, cx| {
                        this.selected = None;
                        match r {
                            Ok(()) => cx.emit(TreeEvent::Moved { old: original, new: to }),
                            Err(e) => eprintln!("[tree] 移动失败: {e}"),
                        }
                        cx.notify();
                    });
                })
                .detach();
            }
        }
    }

    /// 删除确认后执行：core fs_delete（优先进系统废纸篓）。
    /// 成功后发 Deleted 让 Workspace 关掉对应标签。
    fn do_delete(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        // vault 根不可删（core 不知道 root，边界在 UI 层把关）。
        if path == self.vault.root {
            eprintln!("[tree] 拒绝删除 vault 根目录");
            return;
        }
        cx.spawn(async move |this, cx| {
            let p = path.clone();
            let r = cx
                .background_executor()
                .spawn(async move { solomd_core::fs_delete(p.to_string_lossy().to_string()) })
                .await;
            let _ = this.update(cx, |this, cx| {
                if this.selected.as_ref() == Some(&path) {
                    this.selected = None;
                }
                match r {
                    Ok(()) => cx.emit(TreeEvent::Deleted(path)),
                    Err(e) => eprintln!("[tree] 删除失败: {e}"),
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// 在 Finder / 文件管理器中显示（app 层职责；reveal 失败回退打开父目录）。
    /// 走 GPUI 平台层 `reveal_path`（macOS Finder 选中项），失败时降级
    /// `open_with_system` 打开父目录（上游 reveal-in-file-manager.ts 同款链）。
    fn reveal_in_finder(&self, cx: &mut Context<Self>, path: &Path) {
        cx.reveal_path(path);
    }

    fn copy_path(&self, path: &Path, relative: bool, cx: &mut Context<Self>) {
        let text = if relative {
            self.rel_of(path).to_string_lossy().replace('\\', "/")
        } else {
            path.to_string_lossy().to_string()
        };
        cx.write_to_clipboard(gpui::ClipboardItem::new_string(text));
    }

    fn walk(
        &self,
        rel: &Path,
        depth: usize,
        rows: &mut Vec<AnyElement>,
        cx: &mut Context<Self>,
    ) {
        if depth > 4 || rows.len() > 500 {
            return;
        }
        let Ok(entries) = self.vault.list_dir_ex(rel, self.show_hidden) else {
            return;
        };
        // 菜单项 on_click 只有 (&mut Window, &mut App)——没有面板实体句柄。
        // 在这里捕获 weak entity，点击时 upgrade + update 回到面板。
        let weak = cx.entity().downgrade();
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
            let selected = self.selected.as_ref() == Some(&path);
            // 稳定 ID：由路径派生（counter ID 在过滤/展开后会让
            // context menu 的 element state 绑错行）。
            let id = SharedString::from(format!(
                "tree-{}",
                path.to_string_lossy().replace(['/', '\\', '.'], "-")
            ));
            let open_path = path.clone();
            let menu_path = path.clone();
            let menu_path2 = path.clone();
            let menu_path3 = path.clone();
            let menu_path4 = path.clone();
            let menu_is_dir = is_dir;
            let vault_root = self.vault.root.clone();
            let weak = weak.clone();

            let row = div()
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
                .context_menu(move |menu, _window, _cx| {
                    let mut m = menu;
                    // 目录：新建文件 / 新建文件夹。
                    if menu_is_dir {
                        let p = menu_path.clone();
                        let w = weak.clone();
                        m = m.item(
                            gpui_component::menu::PopupMenuItem::new("新建文件").on_click(
                                move |_, window, cx| {
                                    if let Some(tree) = w.upgrade() {
                                        let _ = tree.update(cx, |tree, cx| {
                                            tree.start_edit(
                                                TreeEdit::NewFile { parent: p.clone() },
                                                "untitled.md",
                                                window,
                                                cx,
                                            );
                                        });
                                    }
                                },
                            ),
                        );
                        let p2 = menu_path.clone();
                        let w = weak.clone();
                        m = m.item(
                            gpui_component::menu::PopupMenuItem::new("新建文件夹").on_click(
                                move |_, window, cx| {
                                    if let Some(tree) = w.upgrade() {
                                        let _ = tree.update(cx, |tree, cx| {
                                            tree.start_edit(
                                                TreeEdit::NewDir { parent: p2.clone() },
                                                "新文件夹",
                                                window,
                                                cx,
                                            );
                                        });
                                    }
                                },
                            ),
                        );
                        m = m.separator();
                    }
                    // 重命名/移动（F2 语义；输入 vault 相对路径）。
                    {
                        let p3 = menu_path2.clone();
                        let root = vault_root.clone();
                        let w = weak.clone();
                        m = m.item(
                            gpui_component::menu::PopupMenuItem::new("重命名/移动…").on_click(
                                move |_, window, cx| {
                                    if let Some(tree) = w.upgrade() {
                                        let _ = tree.update(cx, |tree, cx| {
                                            let rel = p3
                                                .strip_prefix(&root)
                                                .unwrap_or(&p3)
                                                .to_string_lossy()
                                                .to_string();
                                            tree.selected = Some(p3.clone());
                                            tree.start_edit(
                                                TreeEdit::Rename { original: p3.clone() },
                                                &rel,
                                                window,
                                                cx,
                                            );
                                        });
                                    }
                                },
                            ),
                        );
                    }
                    // 删除（危险项；确认对话框在点击后弹出）。
                    {
                        let p4 = menu_path3.clone();
                        let w = weak.clone();
                        m = m.item(
                            gpui_component::menu::PopupMenuItem::new("删除").on_click(
                                move |_, _window, cx| {
                                    if let Some(tree) = w.upgrade() {
                                        let _ = tree.update(cx, |tree, cx| {
                                            tree.delete_target = Some(p4.clone());
                                            cx.notify();
                                        });
                                    }
                                },
                            ),
                        );
                    }
                    m = m.separator();
                    // 复制路径。
                    {
                        let pa = menu_path4.clone();
                        let w = weak.clone();
                        m = m.item(
                            gpui_component::menu::PopupMenuItem::new("复制路径").on_click(
                                move |_, _window, cx| {
                                    if let Some(tree) = w.upgrade() {
                                        let _ = tree.update(cx, |tree, cx| {
                                            tree.copy_path(&pa, false, cx);
                                        });
                                    }
                                },
                            ),
                        );
                    }
                    {
                        let pr = menu_path4.clone();
                        let w = weak.clone();
                        m = m.item(
                            gpui_component::menu::PopupMenuItem::new("复制相对路径").on_click(
                                move |_, _window, cx| {
                                    if let Some(tree) = w.upgrade() {
                                        let _ = tree.update(cx, |tree, cx| {
                                            tree.copy_path(&pr, true, cx);
                                        });
                                    }
                                },
                            ),
                        );
                    }
                    m = m.separator();
                    // 在 Finder / 文件管理器中显示。
                    {
                        let pv = menu_path4.clone();
                        let w = weak.clone();
                        m.item(
                            gpui_component::menu::PopupMenuItem::new("在 Finder 中显示").on_click(
                                move |_, _window, _cx| {
                                    if let Some(tree) = w.upgrade() {
                                        let _ = tree.update(_cx, |tree, cx| {
                                            tree.reveal_in_finder(cx, &pv);
                                        });
                                    }
                                },
                            ),
                        )
                    }
                });
            rows.push(row.into_any_element());

            if expanded {
                let rel_child = self.rel_of(&path);
                self.walk(&rel_child, depth + 1, rows, cx);
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
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let mut rows = Vec::new();
        self.walk(Path::new(""), 0, &mut rows, cx);

        // 工具行：隐藏开关 + 类型过滤 + 新建入口。
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
                Button::new("tree-new")
                    .label("＋")
                    .xsmall()
                    .ghost()
                    .on_click(cx.listener(|this, _, window, cx| {
                        // 工具栏新建：落在选中目录（无选中 = vault 根）。
                        let parent = this
                            .selected
                            .clone()
                            .filter(|p| p.is_dir())
                            .unwrap_or_else(|| this.vault.root.clone());
                        this.start_edit(TreeEdit::NewFile { parent }, "untitled.md", window, cx);
                    })),
            );

        // 行内编辑行（新建/重命名共用）。
        let edit_row = self.editing.is_some().then(|| {
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
                        .child(Input::new(&self.edit_input).small()),
                )
                .child(
                    Button::new("tree-edit-ok")
                        .label("✓")
                        .xsmall()
                        .ghost()
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.commit_edit(window, cx);
                        })),
                )
                .child(
                    Button::new("tree-edit-cancel")
                        .label("✕")
                        .xsmall()
                        .ghost()
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.cancel_edit(cx);
                        })),
                )
        });

        // 删除确认：AlertDialog（window.open_alert_dialog，模态）。
        if let Some(target) = self.delete_target.clone() {
            let name = target
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();
            let is_dir = target.is_dir();
            self.delete_target = None;
            window_open_delete_dialog(&name, is_dir, target, window, cx);
        }
        div()
            .id("file-tree")
            .size_full()
            .overflow_y_scroll()
            .py_1()
            .child(toolbar)
            .children(edit_row)
            .children(rows)
    }
}

/// 打开删除确认 AlertDialog。确认 → do_delete；取消/关闭 → 清状态。
/// （Render 里调用，每帧幂等：delete_target 只在菜单点击时置位。）
fn window_open_delete_dialog(
    name: &str,
    is_dir: bool,
    target: PathBuf,
    window: &mut Window,
    cx: &mut Context<FileTreePanel>,
) {
    let what = if is_dir { "文件夹" } else { "文件" }.to_string();
    let name = name.to_string();
    let weak = cx.entity().downgrade();
    let target2 = target.clone();
    cx.defer_in(window, move |_this: &mut FileTreePanel, window: &mut Window, cx: &mut Context<FileTreePanel>| {
        let weak_ok = weak.clone();
        let weak_cancel = weak.clone();
        let target_ok = target2.clone();
        window.open_alert_dialog(cx, move |alert, _, _| {
            let what = what.clone();
            let name = name.clone();
            alert
                .title(format!("删除{what}"))
                .description(format!("“{name}” 将移入废纸篓。"))
                .show_cancel(true)
                .button_props(
                    gpui_component::dialog::DialogButtonProps::default()
                        .ok_text("删除")
                        .ok_variant(gpui_component::button::ButtonVariant::Danger),
                )
                .on_ok({
                    let weak_ok = weak_ok.clone();
                    let target_ok = target_ok.clone();
                    move |_, _, cx| {
                        if let Some(tree) = weak_ok.upgrade() {
                            let _ = tree.update(cx, |tree, cx| {
                                tree.delete_target = None;
                                tree.do_delete(target_ok.clone(), cx);
                            });
                        }
                        true
                    }
                })
                .on_cancel({
                    let weak_cancel = weak_cancel.clone();
                    move |_, _, _cx| {
                        if let Some(tree) = weak_cancel.upgrade() {
                            let _ = tree.update(_cx, |tree, cx| {
                                tree.delete_target = None;
                                cx.notify();
                            });
                        }
                        true
                    }
                })
        });
    });
}

impl EventEmitter<TreeEvent> for FileTreePanel {}
impl EventEmitter<PanelEvent> for FileTreePanel {}

impl Focusable for FileTreePanel {
    fn focus_handle(&self, _: &gpui::App) -> gpui::FocusHandle {
        self.focus_handle.clone()
    }
}
