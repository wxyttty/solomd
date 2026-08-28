//! workspace.rs — DockArea 组装：左侧文件树 + 中央多标签笔记区。

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;

use gpui::{
    actions, div, px, AppContext as _, Context, Entity, InteractiveElement as _, IntoElement,
    ParentElement as _, Render, SharedString, StatefulInteractiveElement as _, Styled as _,
    Window,
};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::dock::{DockArea, DockItem, TabPanel};
use gpui_component::input::{Input, InputEvent, InputState};
use gpui_component::scroll::ScrollableElement as _;
use gpui_component::{h_flex, v_flex, ActiveTheme as _, Sizable as _};

use crate::file_tree::{FileTreePanel, TreeEvent};
use crate::images::ImageStore;
use crate::mermaid::MermaidStore;
use crate::note_panel::NotePanel;
use crate::settings::Settings;
use crate::vault::Vault;

actions!(solomd, [TogglePalette, ClosePalette, ToggleTheme]);

pub struct Workspace {
    vault: Vault,
    dock_area: Entity<DockArea>,
    center_tabs: Entity<TabPanel>,
    /// 有序打开列表（TabPanel 的镜像；顺序即标签顺序）。
    tabs: Vec<(PathBuf, Entity<NotePanel>)>,
    /// 最后一个标签被关掉后置位；render（有 window）里自动开 welcome 兜底。
    pending_welcome: bool,
    /// 外部变更检测基线（全库 mtime）。
    mt_baseline: std::collections::HashMap<PathBuf, u64>,
    file_tree: Entity<FileTreePanel>,
    /// ⌘P 笔记切换面板。
    palette_open: bool,
    palette_query: Entity<InputState>,
    dark: bool,
    settings: Settings,
    /// AI 面板（懒创建）。
    ai_panel: Option<Entity<crate::ai_panel::AiPanel>>,
    keys_panel: Option<Entity<crate::keys_panel::KeysPanel>>,
    search_panel: Option<Entity<crate::search_panel::SearchPanel>>,
    sync_panel: Option<Entity<crate::sync_panel::SyncPanel>>,
    recipes_panel: Option<Entity<crate::recipes_panel::RecipesPanel>>,
    history_panel: Option<Entity<crate::history_panel::HistoryPanel>>,
    info_panel: Option<Entity<crate::info_panel::InfoPanel>>,
    right_tab: RightTab,
    /// 当前活跃笔记（历史/AI/导出共用；由 NotePanel 激活回调驱动）。
    active_note: Option<PathBuf>,
    active_panel: Option<gpui::WeakEntity<NotePanel>>,
    /// 导出浮层状态。
    export_open: bool,
    export_running: bool,
    export_status: Option<SharedString>,
    pandoc_status: Option<SharedString>,
    images: Rc<RefCell<ImageStore>>,
    mermaid: Rc<RefCell<MermaidStore>>,
}

#[derive(PartialEq, Clone, Copy)]
enum RightTab {
    None,
    Ai,
    Keys,
    Search,
    Sync,
    Recipes,
    History,
    Info,
}

/// 导出菜单项（label, pandoc 输出扩展名）。
const EXPORT_FORMATS: &[(&str, &str)] = &[
    ("Word (.docx)", "docx"),
    ("HTML (.html)", "html"),
    ("EPUB (.epub)", "epub"),
    ("ODT (.odt)", "odt"),
    ("RTF (.rtf)", "rtf"),
    ("LaTeX (.tex)", "tex"),
];

impl Workspace {
    pub fn new(vault: Vault, window: &mut Window, cx: &mut Context<Self>) -> Self {
        // 空库给一篇示例，避免首次启动白屏。
        if vault
            .list_dir(Path::new(""))
            .map(|e| e.is_empty())
            .unwrap_or(true)
        {
            let _ = vault.write_note(
                Path::new("welcome.md"),
                "# 欢迎使用 SoloMD (GPUI)\n\n这是迁移后的原生版本。\n\n- 编辑后 ⌘S 保存，标题出现 • 表示未保存\n- 左侧点击打开笔记，⌘N 新建\n- 图片：截图后 ⌘V 直接粘贴\n\n```mermaid\nflowchart LR\n    A[Tauri] -->|migrate| B[GPUI]\n```\n",
            );
        }

        let dock_area = cx.new(|cx| DockArea::new("solomd-dock", Some(0), window, cx));
        let weak_area = dock_area.downgrade();

        // 中央 TabPanel 自建自管（动态开/关标签）。
        let center_tabs = cx.new(|cx| TabPanel::new(None, weak_area.clone(), window, cx));
        let center = DockItem::Tabs {
            size: None,
            items: Vec::new(),
            active_ix: 0,
            view: center_tabs.clone(),
        };

        let file_tree = cx.new(|cx| FileTreePanel::new(vault.clone(), window, cx));

        dock_area.update(cx, |area, cx| {
            area.set_center(center, window, cx);
            area.set_left_dock(
                DockItem::tab(file_tree.clone(), &weak_area, window, cx),
                Some(px(240.)),
                true,
                window,
                cx,
            );
        });

        // 文件树点开笔记 → open_note。
        cx.subscribe_in(&file_tree, window, |this, _, ev: &TreeEvent, window, cx| match ev {
            TreeEvent::OpenNote(path) => this.open_note(path.clone(), window, cx),
        })
        .detach();

        // 信息面板（大纲/反链/标签）点条目 → 打开并跳行。
        // （info_panel 懒创建，创建后在这里挂订阅。）

        // 外部变更轮询（多设备同步场景）：干净标签自动重载，脏标签不动（编辑器版本优先）。
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(std::time::Duration::from_secs(2))
                    .await;
                let Some(entity) = this.upgrade() else { break };
                let changed = entity.update(cx, |ws, cx| ws.poll_external(cx));
                if changed {
                    let _ = entity.update(cx, |ws, cx| {
                        ws.file_tree.update(cx, |_, cx| cx.notify());
                    });
                }
            }
        })
        .detach();

        // debug 调试桥：/state /open_note /run_recipe 的 UI 侧执行泵。
        #[cfg(debug_assertions)]
        cx.spawn_in(window, async move |this, cx| loop {
            cx.background_executor()
                .timer(std::time::Duration::from_millis(120))
                .await;
            let Some(entity) = this.upgrade() else { break };
            while let Some(req) = crate::dev_bridge::try_recv() {
                let crate::dev_bridge::DevRequest { action, reply } = req;
                let v = entity
                    .update_in(cx, |ws, window, cx| ws.handle_dev(action, window, cx))
                    .unwrap_or_else(|_| {
                        serde_json::json!({"ok": false, "error": "workspace update 失败"})
                    });
                let _ = reply.send(v);
            }
        })
        .detach();

        let mt_baseline = vault.mtimes();
        let this_vault = vault.clone();
        let mut this = Self {
            vault,
            dock_area,
            center_tabs,
            tabs: Vec::new(),
            pending_welcome: false,
            palette_open: false,
            palette_query: cx.new(|cx| {
                InputState::new(window, cx)
                    .placeholder("输入文件名过滤，Enter 打开首个匹配")
            }),
            dark: false,
            settings: Settings::default(),
            ai_panel: None,
            keys_panel: None,
            search_panel: None,
            sync_panel: None,
            recipes_panel: None,
            history_panel: None,
            info_panel: None,
            right_tab: RightTab::None,
            active_note: None,
            active_panel: None,
            export_open: false,
            export_running: false,
            export_status: None,
            pandoc_status: None,
            mt_baseline,
            file_tree: file_tree.clone(),
            images: Rc::new(RefCell::new(ImageStore::new(
                std::env::current_dir().unwrap_or_default(),
            ))),
            mermaid: Rc::new(RefCell::new(MermaidStore::default())),
        };
        // ⌘P 面板：Enter 打开首个匹配。
        cx.subscribe_in(&this.palette_query, window, |this, _, ev: &InputEvent, window, cx| match ev {
            InputEvent::PressEnter { .. } => this.palette_confirm_first(window, cx),
            _ => {}
        })
        .detach();

        // 恢复持久化主题：全局 Theme + 已打开编辑器的 SyntaxStyle。
        let saved = Settings::load(&this_vault.root);
        this.dark = saved.dark;
        this.settings = saved;
        if this.dark {
            gpui_component::theme::Theme::change(
                gpui_component::theme::ThemeMode::Dark,
                Some(window),
                cx,
            );
            this.apply_editor_theme(cx);
        }

        // 启动自动打开 welcome（无则第一篇笔记）。
        let welcome = this_vault.resolve(Path::new("welcome.md"));
        let first = if welcome.exists() {
            Some(welcome)
        } else {
            this_vault
                .list_dir(Path::new(""))
                .ok()
                .and_then(|es| es.into_iter().find(|e| !e.is_dir))
                .map(|e| e.path)
        };
        if let Some(path) = first {
            this.open_note(path, window, cx);
        }
        this
    }

    pub fn open_note(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        self.pending_welcome = false;
        // 已打开 → 激活该标签。
        if let Some(ix) = self.tabs.iter().position(|(p, _)| *p == path) {
            self.activate(ix, cx);
            self.note_activated(path, cx);
            return;
        }

        let vault = self.vault.clone();
        let images = self.images.clone();
        let mermaid = self.mermaid.clone();
        let panel = cx.new(|cx| NotePanel::new(path.clone(), vault, images, mermaid, window, cx));

        // Tab 关闭（UI ✕ / ⌘W）→ 同步本地列表。
        let weak_ws = cx.entity().downgrade();
        let close_path = path.clone();
        panel.update(cx, |np, _| {
            np.on_removed_notify(Box::new(move |app| {
                if let Some(ws) = weak_ws.upgrade() {
                    let _ = ws.update(app, |ws, cx| {
                        ws.tabs.retain(|(p, _)| *p != close_path);
                        // dock 设计：最后一个面板不可关。空了就置标记，
                        // 下一帧 render 里自动开 welcome（render 有 window）。
                        if ws.tabs.is_empty() {
                            ws.pending_welcome = true;
                        }
                        cx.notify();
                    });
                }
            }));
        });

        // 标签激活（打开/点击）→ 跟踪当前笔记。
        let weak_ws = cx.entity().downgrade();
        let active_path = path.clone();
        panel.update(cx, |np, _| {
            np.on_activated_notify(Box::new(move |app| {
                if let Some(ws) = weak_ws.upgrade() {
                    let _ = ws.update(app, |ws, cx| ws.note_activated(active_path.clone(), cx));
                }
            }));
        });

        self.tabs.push((path, panel.clone()));
        self.center_tabs.update(cx, |tabs, cx| {
            tabs.add_panel(Arc::new(panel) as Arc<dyn gpui_component::dock::PanelView>, window, cx);
        });
    }

    fn open_palette(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.palette_open = true;
        self.palette_query.update(cx, |q, cx| q.set_value("", window, cx));
        self.palette_query.update(cx, |q, cx| q.focus(window, cx));
        cx.notify();
    }

    fn close_palette(&mut self, cx: &mut Context<Self>) {
        self.palette_open = false;
        cx.notify();
    }

    /// 当前过滤词匹配的笔记（最多 12 条）。
    fn palette_matches(&self, cx: &Context<Self>) -> Vec<PathBuf> {
        let query = self.palette_query.read(cx).value().to_lowercase();
        self.vault
            .walk_notes()
            .into_iter()
            .filter(|p| {
                query.is_empty() || p.to_string_lossy().to_lowercase().contains(&query)
            })
            .take(12)
            .collect()
    }

    fn palette_confirm_first(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let first = self.palette_matches(cx).first().cloned();
        if let Some(path) = first {
            self.close_palette(cx);
            self.open_note(path, window, cx);
        }
    }

    /// 激活第 ix 个标签：走 DockItem::active_index 的公开侧通道
    /// （1505b14 的 TabPanel::set_active_ix 是私有）。
    fn activate(&self, ix: usize, cx: &mut Context<Self>) {
        self.dock_area.update(cx, |area, cx| {
            let item = area.center().clone();
            let _ = item.active_index(ix, cx);
        });
    }

    /// 对比 mtime 基线：外部修改的已打开干净标签自动重载；返回是否有变化（刷新文件树）。
    fn poll_external(&mut self, cx: &mut Context<Self>) -> bool {
        let changes = self.vault.diff_changes(&self.mt_baseline);
        self.mt_baseline = self.vault.mtimes();
        if !changes.modified.is_empty() || !changes.deleted.is_empty() {
            eprintln!(
                "[poll] modified={:?} deleted={:?}",
                changes.modified, changes.deleted
            );
        }
        if changes.modified.is_empty() && changes.deleted.is_empty() {
            return false;
        }

        for path in &changes.modified {
            if let Some((_, panel)) = self.tabs.iter().find(|(p, _)| p == path) {
                // 打开中的：仅当无未保存修改且磁盘内容确实不同才重载。
                let dirty = panel.read(cx).dirty;
                if dirty {
                    continue; // 编辑器版本优先；保存后自然并轨
                }
                match self.vault.read_note(path) {
                    Ok(text) => {
                        let panel = panel.clone();
                        panel.update(cx, |np, cx| {
                            if np.state.read(cx).text() != text {
                                np.state.update(cx, |ed, cx| ed.set_text(text, cx));
                                eprintln!("[sync] 外部修改已重载: {}", path.display());
                                cx.notify();
                            }
                        });
                    }
                    Err(e) => eprintln!("[poll] 读盘失败 {path:?}: {e}"),
                }
            }
        }
        // deleted：打开中的标签保持内存版（保存即重建文件），不特殊处理。
        true
    }

    /// 明暗切换：全局 Theme + 所有打开编辑器的 SyntaxStyle + 持久化。
    pub fn toggle_theme(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.dark = !self.dark;
        gpui_component::theme::Theme::change(
            if self.dark {
                gpui_component::theme::ThemeMode::Dark
            } else {
                gpui_component::theme::ThemeMode::Light
            },
            Some(window),
            cx,
        );
        self.apply_editor_theme(cx);
        self.settings.dark = self.dark;
        self.settings.save(&self.vault.root);
        cx.notify();
    }

    /// 把当前明暗对应的 SyntaxStyle 重涂到所有打开的编辑器。
    fn apply_editor_theme(&mut self, cx: &mut Context<Self>) {
        for (_, panel) in &self.tabs {
            let dark = self.dark;
            panel.update(cx, |np, cx| np.apply_theme(dark, cx));
        }
    }

    pub fn new_note(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match self.vault.create_note(Path::new("")) {
            Ok(path) => self.open_note(path, window, cx),
            Err(e) => eprintln!("[new] {e}"),
        }
    }

    /// 笔记激活（打开/点击标签）：更新跟踪状态并推送各右栏页。
    fn note_activated(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        self.active_note = Some(path.clone());
        self.active_panel = self
            .tabs
            .iter()
            .find(|(p, _)| *p == path)
            .map(|(_, panel)| panel.downgrade());
        if let Some(hp) = &self.history_panel {
            hp.update(cx, |h, cx| h.set_active(Some(path.clone()), cx));
        }
        if let Some(ip) = &self.info_panel {
            let weak = self.active_panel.clone();
            ip.update(cx, |i, cx| i.set_active(Some(path.clone()), weak, cx));
        }
        if let Some(ap) = &self.ai_panel {
            ap.update(cx, |a, _| a.set_active_note(Some(path.clone())));
            ap.update(cx, |a, _| a.set_insert_target(self.active_panel.clone()));
        }
    }

    /// 打开笔记并跳到第 line 行（1-based，0 = 只打开）。
    pub fn open_note_at(
        &mut self,
        path: PathBuf,
        line: u32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_note(path.clone(), window, cx);
        if line == 0 {
            return;
        }
        if let Some((_, panel)) = self.tabs.iter().find(|(p, _)| *p == path) {
            let text = self.vault.read_note(&path).unwrap_or_default();
            let offset: usize = text.lines().take(line as usize).map(|l| l.len() + 1).sum();
            let panel = panel.clone();
            panel.update(cx, |np, cx| np.jump_to_offset(offset, window, cx));
        }
    }

    /// debug 调试桥的动作执行（debug-only；见 dev_bridge.rs）。
    #[cfg(debug_assertions)]
    fn handle_dev(
        &mut self,
        action: crate::dev_bridge::DevAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> serde_json::Value {
        use crate::dev_bridge::DevAction as DA;
        match action {
            DA::State => serde_json::json!({
                "ok": true,
                "vault": self.vault.root.display().to_string(),
                "active_note":
                    self.active_note.as_ref().map(|p| p.display().to_string()),
                "tabs": self
                    .tabs
                    .iter()
                    .map(|(p, _)| p.display().to_string())
                    .collect::<Vec<_>>(),
            }),
            DA::OpenNote(path) => {
                let p = PathBuf::from(&path);
                let resolved = if p.is_absolute() { p } else { self.vault.resolve(&p) };
                if !resolved.exists() {
                    return serde_json::json!({
                        "ok": false,
                        "error": format!("文件不存在: {}", resolved.display())
                    });
                }
                self.open_note(resolved, window, cx);
                serde_json::json!({"ok": true})
            }
            DA::RunRecipe(slug) => {
                let ws = self.vault.root.to_string_lossy().to_string();
                let sink: std::sync::Arc<dyn solomd_core::deps::EventSink> =
                    std::sync::Arc::new(solomd_core::deps::NullSink);
                let slug2 = slug.clone();
                cx.spawn(async move |_, cx| {
                    let _ = cx
                        .background_executor()
                        .spawn(async move {
                            let _ =
                                solomd_core::recipe_runner::recipes_run_now(sink, ws, slug2)
                                    .await;
                        })
                        .await;
                })
                .detach();
                serde_json::json!({"ok": true, "started": slug})
            }
        }
    }

    /// 工具栏「导出」：开浮层 + 首次探测 pandoc。
    fn toggle_export(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.export_open = !self.export_open;
        if self.export_open && self.pandoc_status.is_none() {
            cx.spawn_in(window, async move |this, cx| {
                let info = cx
                    .background_executor()
                    .spawn(async move { solomd_core::pandoc::pandoc_detect() })
                    .await;
                let _ = this.update_in(cx, |ws, window, _cx| {
                    ws.pandoc_status = Some(match info {
                        Ok(Some(i)) => {
                            SharedString::from(format!("pandoc {} · {}", i.version, i.path))
                        }
                        Ok(None) => "未检测到 pandoc（brew install pandoc）".into(),
                        Err(e) => SharedString::from(format!("pandoc 检测失败：{e}")),
                    });
                    window.refresh();
                });
            })
            .detach();
        }
        cx.notify();
    }

    /// 导出当前笔记（含未保存编辑）为指定格式，输出到 <vault>/exports/。
    fn export_as(&mut self, ext: String, window: &mut Window, cx: &mut Context<Self>) {
        if self.export_running {
            return;
        }
        let Some(path) = self.active_note.clone() else {
            self.export_status = Some("没有打开的笔记".into());
            cx.notify();
            return;
        };
        let Some(panel) = self.active_panel.clone() else {
            self.export_status = Some("没有打开的笔记".into());
            cx.notify();
            return;
        };
        // 未保存内容一并导出（读编辑器缓冲，不是磁盘）。
        let text = match panel.update(cx, |np, cx| np.state.read(cx).text().to_string()) {
            Ok(t) => t,
            Err(_) => {
                self.export_status = Some("笔记面板已关闭".into());
                cx.notify();
                return;
            }
        };
        let stem = path
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| "note".into());
        let out_dir = self.vault.root.join("exports");
        let out_path = out_dir.join(format!("{stem}.{ext}"));
        let out_display = out_path.to_string_lossy().to_string();
        let out_path_str = out_display.clone();

        self.export_running = true;
        self.export_status = Some(SharedString::from(format!("导出 {ext} 中…")));
        self.export_open = false;
        cx.notify();

        cx.spawn_in(window, async move |this, cx| {
            let r = cx
                .background_executor()
                .spawn(async move {
                    let _ = std::fs::create_dir_all(&out_dir);
                    solomd_core::pandoc::pandoc_export_inner(solomd_core::pandoc::ExportArgs {
                        input_markdown: text,
                        format: ext.clone(),
                        output_path: out_path_str,
                        bibliography: None,
                        csl: None,
                        template: None,
                        extra_args: Vec::new(),
                    })
                })
                .await;
            let _ = this.update_in(cx, |ws, window, _cx| {
                ws.export_running = false;
                ws.export_status = Some(match r {
                    Ok(()) => SharedString::from(format!("已导出 {out_display}")),
                    Err(e) => SharedString::from(format!("导出失败：{e}")),
                });
                window.refresh();
            });
        })
        .detach();
    }
}

impl Render for Workspace {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // 空态兜底：所有标签关完 → 回到 welcome（dock 要求至少一个面板）。
        if self.pending_welcome {
            self.pending_welcome = false;
            let welcome = self.vault.resolve(Path::new("welcome.md"));
            if !welcome.exists() {
                let _ = self.vault.write_note(&welcome, "# Home\n");
            }
            self.open_note(welcome, window, cx);
        }
        v_flex()
            .size_full()
            .on_action(cx.listener(|this, _: &crate::note_panel::NewNote, window, cx| {
                this.new_note(window, cx);
            }))
            .on_action(cx.listener(|this, _: &TogglePalette, window, cx| {
                this.open_palette(window, cx);
            }))
            .on_action(cx.listener(|this, _: &ClosePalette, _, cx| {
                this.close_palette(cx);
            }))
            .on_action(cx.listener(|this, _: &ToggleTheme, window, cx| {
                this.toggle_theme(window, cx);
            }))
            .child(
                h_flex()
                    .px_3()
                    .py_1p5()
                    .gap_2()
                    .bg(cx.theme().background)
                    .border_b_1()
                    .border_color(cx.theme().border)
                    .text_color(cx.theme().foreground)
                    .child(
                        div()
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(cx.theme().foreground)
                            .child("SoloMD"),
                    )
                    .child(
                        Button::new("new-note")
                            .label("新建笔记 (⌘N)")
                            .xsmall()
                            .ghost()
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.new_note(window, cx);
                            })),
                    )
                    .child(
                        Button::new("ai-panel")
                            .label(if self.right_tab == RightTab::Ai { "AI ▾" } else { "AI" })
                            .xsmall()
                            .ghost()
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.right_tab = if this.right_tab == RightTab::Ai {
                                    RightTab::None
                                } else {
                                    RightTab::Ai
                                };
                                if this.ai_panel.is_none() && this.right_tab == RightTab::Ai {
                                    this.ai_panel = Some(cx.new(|cx| {
                                        crate::ai_panel::AiPanel::new(
                                            this.vault.clone(),
                                            window,
                                            cx,
                                        )
                                    }));
                                    if let Some(ap) = &this.ai_panel {
                                        let note = this.active_note.clone();
                                        let target = this.active_panel.clone();
                                        ap.update(cx, |a, _| {
                                            a.set_active_note(note);
                                            a.set_insert_target(target);
                                        });
                                    }
                                }
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new("search-panel")
                            .label(if self.right_tab == RightTab::Search { "搜索 ▾" } else { "搜索" })
                            .xsmall()
                            .ghost()
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.right_tab = if this.right_tab == RightTab::Search {
                                    RightTab::None
                                } else {
                                    RightTab::Search
                                };
                                if this.search_panel.is_none() && this.right_tab == RightTab::Search {
                                    this.search_panel = Some(cx.new(|cx| {
                                        crate::search_panel::SearchPanel::new(
                                            this.vault.root.clone(),
                                            window,
                                            cx,
                                        )
                                    }));
                                }
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new("recipes-panel")
                            .label("Recipes")
                            .xsmall()
                            .ghost()
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.right_tab = if this.right_tab == RightTab::Recipes {
                                    RightTab::None
                                } else {
                                    RightTab::Recipes
                                };
                                if this.recipes_panel.is_none()
                                    && this.right_tab == RightTab::Recipes
                                {
                                    this.recipes_panel = Some(cx.new(|cx| {
                                        crate::recipes_panel::RecipesPanel::new(
                                            this.vault.clone(),
                                            window,
                                            cx,
                                        )
                                    }));
                                }
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new("sync-panel")
                            .label("同步")
                            .xsmall()
                            .ghost()
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.right_tab = if this.right_tab == RightTab::Sync {
                                    RightTab::None
                                } else {
                                    RightTab::Sync
                                };
                                if this.sync_panel.is_none() && this.right_tab == RightTab::Sync {
                                    this.sync_panel = Some(cx.new(|cx| {
                                        crate::sync_panel::SyncPanel::new(
                                            this.vault.clone(),
                                            window,
                                            cx,
                                        )
                                    }));
                                }
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new("keys-panel")
                            .label(if self.right_tab == RightTab::Keys { "Keys ▾" } else { "Keys" })
                            .xsmall()
                            .ghost()
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.right_tab = if this.right_tab == RightTab::Keys {
                                    RightTab::None
                                } else {
                                    RightTab::Keys
                                };
                                if this.keys_panel.is_none() && this.right_tab == RightTab::Keys {
                                    this.keys_panel = Some(cx.new(|cx| {
                                        crate::keys_panel::KeysPanel::new(window, cx)
                                    }));
                                }
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new("info-panel")
                            .label(if self.right_tab == RightTab::Info { "信息 ▾" } else { "信息" })
                            .xsmall()
                            .ghost()
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.right_tab = if this.right_tab == RightTab::Info {
                                    RightTab::None
                                } else {
                                    RightTab::Info
                                };
                                if this.info_panel.is_none() && this.right_tab == RightTab::Info {
                                    let panel = cx.new(|cx| {
                                        crate::info_panel::InfoPanel::new(
                                            this.vault.clone(),
                                            window,
                                            cx,
                                        )
                                    });
                                    let note = this.active_note.clone();
                                    let weak = this.active_panel.clone();
                                    panel.update(cx, |i, cx| i.set_active(note, weak, cx));
                                    this.info_panel = Some(panel.clone());
                                    // 条目点击 → 打开笔记并跳行。
                                    cx.subscribe_in(
                                        &panel,
                                        window,
                                        |this, _, ev: &crate::info_panel::InfoEvent, window, cx| {
                                            match ev {
                                                crate::info_panel::InfoEvent::OpenNote { path, line } => {
                                                    this.open_note_at(
                                                        path.clone(),
                                                        *line,
                                                        window,
                                                        cx,
                                                    );
                                                }
                                            }
                                        },
                                    )
                                    .detach();
                                }
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new("history-panel")
                            .label(if self.right_tab == RightTab::History { "历史 ▾" } else { "历史" })
                            .xsmall()
                            .ghost()
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.right_tab = if this.right_tab == RightTab::History {
                                    RightTab::None
                                } else {
                                    RightTab::History
                                };
                                if this.history_panel.is_none()
                                    && this.right_tab == RightTab::History
                                {
                                    this.history_panel = Some(cx.new(|cx| {
                                        crate::history_panel::HistoryPanel::new(
                                            this.vault.clone(),
                                            window,
                                            cx,
                                        )
                                    }));
                                    let note = this.active_note.clone();
                                    if let Some(hp) = &this.history_panel {
                                        hp.update(cx, |h, cx| h.set_active(note, cx));
                                    }
                                }
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new("toggle-theme")
                            .label(if self.dark { "☀ 浅色" } else { "🌙 深色" })
                            .xsmall()
                            .ghost()
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.toggle_theme(window, cx);
                            })),
                    )
                    .child(
                        Button::new("export")
                            .label(if self.export_open { "导出 ▾" } else { "导出" })
                            .xsmall()
                            .ghost()
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.toggle_export(window, cx);
                            })),
                    )
                    .child(
                        div()
                            .flex_1()
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(cx.theme().muted_foreground)
                                    .truncate()
                                    .child(self.export_status.clone().unwrap_or_default()),
                            ),
                    ),
            )
            .child(
                match self.right_tab {
                    RightTab::None => div()
                        .flex_1()
                        .min_h_0()
                        .overflow_hidden()
                        .child(self.dock_area.clone())
                        .into_any_element(),
                    tab => {
                        let body = match tab {
                            RightTab::Ai => self
                                .ai_panel
                                .as_ref()
                                .map(|p| p.clone().into_any_element())
                                .unwrap_or_else(|| div().into_any_element()),
                            RightTab::Keys => self
                                .keys_panel
                                .as_ref()
                                .map(|p| p.clone().into_any_element())
                                .unwrap_or_else(|| div().into_any_element()),
                            RightTab::Search => self
                                .search_panel
                                .as_ref()
                                .map(|p| p.clone().into_any_element())
                                .unwrap_or_else(|| div().into_any_element()),
                            RightTab::Sync => self
                                .sync_panel
                                .as_ref()
                                .map(|p| p.clone().into_any_element())
                                .unwrap_or_else(|| div().into_any_element()),
                            RightTab::Recipes => self
                                .recipes_panel
                                .as_ref()
                                .map(|p| p.clone().into_any_element())
                                .unwrap_or_else(|| div().into_any_element()),
                            RightTab::History => self
                                .history_panel
                                .as_ref()
                                .map(|p| p.clone().into_any_element())
                                .unwrap_or_else(|| div().into_any_element()),
                            RightTab::Info => self
                                .info_panel
                                .as_ref()
                                .map(|p| p.clone().into_any_element())
                                .unwrap_or_else(|| div().into_any_element()),
                            RightTab::None => div().into_any_element(),
                        };
                        div()
                            .flex_1()
                            .min_h_0()
                            .overflow_hidden()
                            .flex()
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .overflow_hidden()
                                    .child(self.dock_area.clone()),
                            )
                            .child(
                                div()
                                    .w(px(400.))
                                    .min_w_0()
                                    .h_full()
                                    .overflow_y_scrollbar()
                                    .border_l_1()
                                    .border_color(cx.theme().border)
                                    .child(body),
                            )
                            .into_any_element()
                    }
                },
            )
            // 浮层必须是最后一个 child：GPUI 后画的盖先画的，
            // 否则卡片被不透明的 dock 区域盖住（只有无背景的顶栏透出遮罩）。
            .child(self.render_palette_overlay(window, cx))
            .child(self.render_export_overlay(window, cx))
    }

}

impl Workspace {
    /// 导出浮层：半透明遮罩 + 居中卡片（pandoc 状态 + 格式列表）。
    fn render_export_overlay(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if !self.export_open {
            return div().into_any_element();
        }
        let theme = cx.theme().clone();

        let mut list = v_flex().gap_0p5();
        for (label, ext) in EXPORT_FORMATS {
            let ext = ext.to_string();
            list = list.child(
                div()
                    .id(SharedString::from(format!("export-{ext}")))
                    .px_3()
                    .py_1p5()
                    .rounded_md()
                    .text_sm()
                    .cursor_pointer()
                    .hover(|s| s.bg(theme.accent.opacity(0.12)))
                    .on_click({
                        let ext = ext.clone();
                        cx.listener(move |this, _, window, cx| {
                            this.export_as(ext.clone(), window, cx);
                        })
                    })
                    .child(*label),
            );
        }

        div()
            .absolute()
            .inset_0()
            .bg(gpui::black().opacity(0.25))
            .on_mouse_down(gpui::MouseButton::Left, {
                let this = cx.weak_entity();
                move |_, _, cx| {
                    let _ = this.update(cx, |ws, cx| {
                        ws.export_open = false;
                        cx.notify();
                    });
                }
            })
            .child(
                div()
                    .absolute()
                    .left(px(160.))
                    .w(px(320.))
                    .top(px(72.))
                    .flex()
                    .flex_col()
                    .rounded_lg()
                    .border_1()
                    .border_color(theme.border)
                    .bg(theme.background)
                    .shadow_lg()
                    .overflow_hidden()
                    .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| {
                        cx.stop_propagation();
                    })
                    .child(
                        div()
                            .p_2()
                            .border_b_1()
                            .border_color(theme.border)
                            .child(
                                v_flex()
                                    .gap_0p5()
                                    .child(
                                        div()
                                            .font_weight(gpui::FontWeight::SEMIBOLD)
                                            .text_sm()
                                            .child("导出当前笔记（Pandoc）"),
                                    )
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(theme.muted_foreground)
                                            .child(
                                                self.export_status.clone().unwrap_or_else(|| {
                                                    self.pandoc_status
                                                        .clone()
                                                        .unwrap_or_else(|| "检测 pandoc…".into())
                                                }),
                                            ),
                                    ),
                            ),
                    )
                    .child(div().p_1().child(list)),
            )
            .into_any_element()
    }
}

impl Workspace {
    /// ⌘P 浮层：半透明遮罩 + 居中卡片（输入框 + 匹配列表）。
    fn render_palette_overlay(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if !self.palette_open {
            return div().into_any_element();
        }
        let matches = self.palette_matches(cx);
        let theme = cx.theme().clone();

        let mut list = v_flex().gap_0p5();
        if matches.is_empty() {
            list = list.child(
                div()
                    .px_3()
                    .py_2()
                    .text_sm()
                    .text_color(theme.muted_foreground)
                    .child("无匹配笔记"),
            );
        }
        for path in matches {
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();
            let dir = path
                .parent()
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_default();
            let open_path = path.clone();
            list = list.child(
                div()
                    .id(SharedString::from(format!("pal-{}", open_path.display())))
                    .flex()
                    .items_center()
                    .justify_between()
                    .px_3()
                    .py_1p5()
                    .rounded_md()
                    .text_sm()
                    .cursor_pointer()
                    .hover(|s| s.bg(theme.accent.opacity(0.12)))
                    .on_mouse_down(gpui::MouseButton::Left, {
                        let this = cx.weak_entity();
                        move |_, window, cx| {
                            let _ = this.update(cx, |ws, cx| {
                                ws.close_palette(cx);
                                ws.open_note(open_path.clone(), window, cx);
                            });
                        }
                    })
                    .child(name)
                    .child(
                        div()
                            .text_xs()
                            .text_color(theme.muted_foreground)
                            .child(dir),
                    ),
            );
        }

        let query = self.palette_query.clone();
        div()
            .absolute()
            .inset_0()
            .bg(gpui::black().opacity(0.25))
            .on_mouse_down(gpui::MouseButton::Left, {
                let this = cx.weak_entity();
                move |_, _, cx| {
                    let _ = this.update(cx, |ws, cx| ws.close_palette(cx));
                }
            })
            .child(
                div()
                    .absolute()
                    .left(px(160.))
                    .right(px(160.))
                    .top(px(72.))
                    .flex()
                    .flex_col()
                    .rounded_lg()
                    .border_1()
                    .border_color(theme.border)
                    .bg(theme.background)
                    .shadow_lg()
                    .overflow_hidden()
                    .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| {
                        cx.stop_propagation();
                    })
                    .child(
                        div()
                            .p_2()
                            .border_b_1()
                            .border_color(theme.border)
                            .child(Input::new(&query).small()),
                    )
                    .child(div().max_h(px(420.)).overflow_y_scrollbar().p_1().child(list)),
            )
            .into_any_element()
    }
}
