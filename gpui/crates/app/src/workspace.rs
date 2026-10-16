//! workspace.rs — DockArea 组装：左侧文件树 + 中央多标签笔记区。

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use gpui::{
    actions, div, px, AppContext as _, Context, Entity, InteractiveElement as _, IntoElement,
    ParentElement as _, Render, SharedString, StatefulInteractiveElement as _, Styled as _,
    Window,
};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::dock::{panel_handle, DockArea, DockLayout, DockPlacement, DockSkin};
use gpui_component::command::{Command, CommandItem, CommandState};
use gpui_component::scroll::ScrollableElement as _;
use gpui_component::{h_flex, v_flex, ActiveTheme as _, Sizable as _};

use crate::file_tree::{FileTreePanel, TreeEvent};
use crate::images::ImageStore;
use crate::mermaid::MermaidStore;
use crate::note_panel::NotePanel;
use crate::settings::Settings;
use crate::vault::Vault;

actions!(solomd, [TogglePalette, ClosePalette, ToggleTheme, ImportFile]);

pub struct Workspace {
    vault: Vault,
    dock_area: Entity<DockArea>,
    /// 有序打开列表（dock 中央组内容的镜像；顺序即标签顺序）。
    tabs: Vec<(PathBuf, Entity<NotePanel>)>,
    /// 最后一个标签被关掉后置位；render（有 window）里自动开 welcome 兜底。
    pending_welcome: bool,
    /// 外部变更检测基线（全库 mtime）。
    mt_baseline: std::collections::HashMap<PathBuf, u64>,
    /// AutoGit 提交并发防护：任务进行中时新保存跳过本轮提交
    /// （文件已落盘，下一轮保存把增量一并提交——git 快照语义无丢失）。
    auto_git_running: bool,
    /// busy 期间到达的保存（folder, file）：任务结束后补一轮提交，
    /// 保证最后一次保存最终进入提交。
    auto_git_pending: Option<(PathBuf, PathBuf)>,
    file_tree: Entity<FileTreePanel>,
    /// ⌘P 笔记切换面板。
    palette_open: bool,
    command_state: Entity<CommandState>,
    /// ⌘P 条目顺序（渲染时与 items 一一对应，on_confirm 按 row 取回）。
    palette_paths: Vec<PathBuf>,
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
    tasks_panel: Option<Entity<crate::tasks_panel::TasksPanel>>,
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
    math: Rc<RefCell<crate::math::MathStore>>,
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
    Tasks,
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

        // DockSkin::dock_area = DockArea + 默认皮肤（无 skin 只绘制空 chrome）。
        let (dock_area, _skin) = DockSkin::dock_area("solomd-dock", Some(0), window, cx);

        let file_tree = cx.new(|cx| FileTreePanel::new(vault.clone(), window, cx));

        dock_area.update(cx, |area, cx| {
            area.set_center(DockLayout::tabs(), window, cx);
            area.set_dock(
                DockPlacement::Left,
                DockLayout::tabs().panel(file_tree.clone()),
                window,
                cx,
            );
            area.set_dock_size(DockPlacement::Left, px(240.), window, cx);
        });

        // 文件树点开笔记 → open_note。
        cx.subscribe_in(&file_tree, window, |this, _, ev: &TreeEvent, window, cx| match ev {
            TreeEvent::OpenNote(path) => this.open_note(path.clone(), window, cx),
            TreeEvent::Moved { old, new } => {
                // 移动/重命名后重指向已开标签（面板的 path 字段决定保存落点）。
                for (p, panel) in &mut this.tabs {
                    if *p == *old {
                        *p = new.clone();
                        panel.update(cx, |np, _| np.path = new.clone());
                    }
                }
                if this.active_note.as_ref() == Some(old) {
                    this.active_note = Some(new.clone());
                }
                cx.notify();
            }
            TreeEvent::Deleted(path) => {
                // 删除后关掉对应已开标签（含目录下子树）：dock remove_panel
                // （on_removed 回调会 defer 清 tabs 列表，防重入 panic）。
                let targets: Vec<_> = this
                    .tabs
                    .iter()
                    .filter(|(p, _)| p == path || p.starts_with(&path))
                    .map(|(_, panel)| panel.clone())
                    .collect();
                for panel in targets {
                    this.dock_area.update(cx, |area, cx| {
                        area.remove_panel(panel.clone(), window, cx);
                    });
                }
                if this
                    .active_note
                    .as_ref()
                    .is_some_and(|p| *p == *path || p.starts_with(path))
                {
                    this.active_note = None;
                    this.active_panel = None;
                }
                // dock 要求至少一个面板；空了下一帧 render 自动开 welcome。
                if this.tabs.is_empty() {
                    this.pending_welcome = true;
                }
                cx.notify();
            }
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
            vault: vault.clone(),
            dock_area,
            tabs: Vec::new(),
            pending_welcome: false,
            palette_open: false,
            command_state: cx.new(|cx| CommandState::new(window, cx)),
            palette_paths: Vec::new(),
            dark: false,
            settings: Settings::default(),
            ai_panel: None,
            keys_panel: None,
            search_panel: None,
            sync_panel: None,
            recipes_panel: None,
            history_panel: None,
            info_panel: None,
            tasks_panel: None,
            right_tab: RightTab::None,
            active_note: None,
            active_panel: None,
            export_open: false,
            export_running: false,
            export_status: None,
            pandoc_status: None,
            mt_baseline,
            auto_git_running: false,
            auto_git_pending: None,
            file_tree: file_tree.clone(),
            images: Rc::new(RefCell::new(ImageStore::new(vault.root.clone()))),
            mermaid: Rc::new(RefCell::new(MermaidStore::default())),
            math: Rc::new(RefCell::new(crate::math::MathStore::default())),
        };
        // ⌘P 的 Enter/Esc/↑↓ 键控与过滤由 gpui-component 的 Command 组件承担
        // （gpui_component::init 已绑定其 context 键位），宿主只管开合与确认。

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
            self.activate(ix, window, cx);
            self.note_activated(path, cx);
            return;
        }

        let vault = self.vault.clone();
        let images = self.images.clone();
        let mermaid = self.mermaid.clone();
        let math = self.math.clone();
        let typewriter = self.settings.typewriter;
        let save_folder = self.vault.root.clone();
        let panel = cx.new(|cx| {
            NotePanel::new(
                path.clone(), vault, images, mermaid, math, typewriter, window, cx,
            )
        });

        // Tab 关闭（UI ✕ / ⌘W）→ 同步本地列表。
        let weak_ws = cx.entity().downgrade();
        let close_path = path.clone();
        panel.update(cx, |np, _| {
            // [[wikilink]] 点击 → 打开/新建目标笔记（解析在 core workspace_index）。
            let weak_open = weak_ws.clone();
            np.on_open_note_notify(Box::new(move |window, app, path| {
                // defer：wikilink 点击发生在编辑器事件里，Workspace 可能仍在
                // update 栈上，直接 update 会重入 panic。window.defer 把窗口
                // 句柄带进延迟闭包，绕开借用生命周期。
                let weak = weak_open.clone();
                window.defer(app, move |window, app| {
                    if let Some(ws) = weak.upgrade() {
                        let _ = ws.update(app, |ws, cx| ws.open_note(path, window, cx));
                    }
                });
            }));
        });
        panel.update(cx, |np, _| {
            // ⌘S 保存成功 → AutoGit（组合层职责：core git_auto_commit 发
            // git-committed 事件，这里监听后触发 on-commit recipes——
            // 事件解耦铁律。上游 useAutoCommit 同款“每次保存立即提交”语义。
            let weak_save = weak_ws.clone();
            let folder_cb = save_folder.clone();
            np.on_saved_notify(Box::new(move |app, file| {
                let weak = weak_save.clone();
                let folder = folder_cb.clone();
                app.defer(move |app| {
                    if let Some(ws) = weak.upgrade() {
                        let _ = ws.update(app, |ws, cx| {
                            ws.auto_git_commit(folder, file, cx);
                        });
                    }
                });
            }));
            // Esc 关查找条后转发关 ⌘P 浮层（propagate 不跨 action 类型）。
            let weak_esc = weak_ws.clone();
            np.on_close_palette_notify(Box::new(move |app| {
                let weak = weak_esc.clone();
                app.defer(move |app| {
                    if let Some(ws) = weak.upgrade() {
                        let _ = ws.update(app, |ws, cx| {
                            if ws.palette_open {
                                ws.close_palette(cx);
                            }
                        });
                    }
                });
            }));
        });
        panel.update(cx, |np, _| {
            np.on_removed_notify(Box::new(move |app| {
                // defer：on_removed 在 dock_area.update 栈内同步触发（activate 的
                // remove+re-add、UI 关闭），此时 ws.update 重入会 panic。
                let weak = weak_ws.clone();
                let close = close_path.clone();
                app.defer(move |app| {
                    if let Some(ws) = weak.upgrade() {
                        let _ = ws.update(app, |ws, cx| {
                            ws.tabs.retain(|(p, _)| *p != close);
                            // dock 设计：最后一个面板不可关。空了就置标记，
                            // 下一帧 render 里自动开 welcome（render 有 window）。
                            if ws.tabs.is_empty() {
                                ws.pending_welcome = true;
                            }
                            cx.notify();
                        });
                    }
                });
            }));
        });

        // 标签激活（打开/点击）→ 跟踪当前笔记。
        let weak_ws = cx.entity().downgrade();
        let active_path = path.clone();
        panel.update(cx, |np, _| {
            np.on_activated_notify(Box::new(move |app| {
                // defer：add_panel 自动激活在 dock_area.update 栈内同步触发。
                let weak = weak_ws.clone();
                let active = active_path.clone();
                app.defer(move |app| {
                    if let Some(ws) = weak.upgrade() {
                        let _ = ws.update(app, |ws, cx| ws.note_activated(active, cx));
                    }
                });
            }));
        });

        self.tabs.push((path, panel.clone()));
        self.dock_area.update(cx, |area, cx| {
            // add_panel_view + panel_handle：皮肤从 handle 恢复 title()；
            // 裸 Entity 只能退到 panel_name（tab 全显 "note" 的根因）。
            area.add_panel_view(
                panel_handle(panel.clone()),
                DockPlacement::Center,
                None,
                window,
                cx,
            );
        });
    }

    fn open_palette(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.palette_open = true;
        let state = self.command_state.clone();
        state.update(cx, |s, cx| {
            s.set_query("", window, cx);
            s.focus(window, cx);
        });
        cx.notify();
    }

    pub(crate) fn close_palette(&mut self, cx: &mut Context<Self>) {
        self.palette_open = false;
        cx.notify();
    }

    /// 激活第 ix 个标签：remove + re-add（add_panel 落位即激活）。
    /// on_removed 回调会在中途同步 tabs 列表，随后在这里重新压回。
    fn activate(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(panel) = self.tabs.get(ix).map(|(_, p)| p.clone()) else {
            return;
        };
        self.dock_area.update(cx, |area, cx| {
            area.remove_panel(panel.clone(), window, cx);
            area.add_panel_view(
                panel_handle(panel),
                DockPlacement::Center,
                None,
                window,
                cx,
            );
        });
        self.pending_welcome = false;
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
        // 白板窗口不在 GPUI 全局里逐帧读主题（闭包无 cx），切主题时刷新其快照。
        crate::whiteboard::sync_style(cx);
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

    /// AutoGit：保存后自动提交（NotePanel on_saved → 此处）。走 core
    /// 事件版 git_auto_commit：真实提交后发 git-committed 事件，本方法
    /// 同时监听该事件触发 on-commit recipes（组合层职责，铁律：core
    /// 模块零横向依赖，跨模块触发用事件解耦）。设置关闭时直接返回。
    /// 仓库未初始化时 core 返回 Err，静默跳过（上游 ensureInitialized
    /// 的降级语义）。busy 期间新保存跳过本轮（文件已落盘，下一轮
    /// 保存会把增量一并提交——git 快照语义无丢失）。
    fn auto_git_commit(&mut self, folder: PathBuf, file: PathBuf, cx: &mut Context<Self>) {
        // 每次重读盘上配置：NotePanel 的 AutoGit 按钮直接写 settings.json，
        // 不经过 Workspace 缓存（self.settings 只在启动时加载）——用缓存
        // 会让新开关不生效。
        let auto_git = crate::settings::Settings::load(&self.vault.root).auto_git;
        if !auto_git || self.auto_git_running {
            // busy 期间丢弃的保存：记 pending 标记，任务结束时补一轮提交
            // （保证最后一次保存最终进入提交——git 快照会把期间增量一并
            // 收进，不丢数据）。
            if auto_git && self.auto_git_running {
                self.auto_git_pending = Some((folder, file));
            }
            return;
        }
        self.auto_git_running = true;
        let folder_s = folder.to_string_lossy().to_string();
        let file_s = file.to_string_lossy().to_string();
        let ws_root = self.vault.root.clone();
        cx.spawn(async move |this, cx| {
            // 事件接收：ChannelSink → mpsc，提交完成后在本任务里直接消费，
            // 不需要常驻泵（git-committed 是低频单发事件）。
            let (tx, rx) = std::sync::mpsc::channel::<(String, String)>();
            let sink: std::sync::Arc<dyn solomd_core::deps::EventSink> =
                std::sync::Arc::new(solomd_core::deps::ChannelSink::new(tx));
            let r = solomd_core::rt::spawn(async move {
                solomd_core::git_history::git_auto_commit(
                    sink.as_ref(),
                    folder_s,
                    Some(file_s),
                    None,
                )
                .await
            })
            .await;
            match r {
                Ok(Ok(Some(sha))) => {
                    eprintln!("[autogit] 已提交 {}", &sha[..7.min(sha.len())]);
                    // 消费 git-committed 事件 → on-commit recipes。
                    for (topic, payload) in rx.try_iter() {
                        if topic == "git-committed" {
                            if let Ok(v) = serde_json::from_str::<serde_json::Value>(&payload) {
                                let sink2: std::sync::Arc<
                                    dyn solomd_core::deps::EventSink,
                                > = std::sync::Arc::new(
                                    solomd_core::deps::NullSink,
                                );
                                let file = v["file"].as_str().map(|s| s.to_string());
                                let sha = v["sha"].as_str().unwrap_or_default().to_string();
                                let root = ws_root.clone();
                                let _ = solomd_core::rt::spawn(async move {
                                    solomd_core::recipe_runner::dispatch_on_commit(
                                        sink2,
                                        root,
                                        file,
                                        sha,
                                    )
                                    .await;
                                })
                                .await;
                            }
                        }
                    }
                }
                Ok(Ok(None)) => {} // 树无变化，正常无操作
                Ok(Err(e)) => eprintln!("[autogit] {e}"),
                Err(e) => eprintln!("[autogit] join: {e}"),
            }
            let _ = this.update(cx, |ws, cx| {
                ws.auto_git_running = false;
                // busy 期间到达的保存：补一轮提交（git 快照会把期间增量
                // 一并收进）。
                if let Some((folder, file)) = ws.auto_git_pending.take() {
                    ws.auto_git_commit(folder, file, cx);
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub fn new_note(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match self.vault.create_note(Path::new("")) {
            Ok(path) => self.open_note(path, window, cx),
            Err(e) => eprintln!("[new] {e}"),
        }
    }

    /// ⌘O 导入：平台文件选择器 → core convert_file_to_markdown →
    /// 转成 vault 内新 .md 文件并打开（上游 useFiles.openAndConvert 语义，
    /// 但落盘为真实文件而非未保存草稿——GPUI 版标签以 path 为保存落点，
    /// 草稿标签需要另建机制，不值得为导入引入）。
    fn import_file(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let rx = cx.prompt_for_paths(gpui::PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("导入文件".into()),
        });
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(paths))) = rx.await else { return };
            let Some(src) = paths.into_iter().next() else { return };
            // 可转换扩展名：与 core convert_file_to_markdown_inner 的分发表
            // 一致（json/xml 也支持；音频格式走 markitdown CLI，桌面端少用，
            // 先不进白名单）。
            let convertible = [
                "docx", "csv", "xlsx", "xls", "html", "htm", "pdf", "pptx", "epub", "json", "xml",
            ];
            let ext = src
                .extension()
                .and_then(|e| e.to_str())
                .map(|e| e.to_ascii_lowercase())
                .unwrap_or_default();
            if !convertible.contains(&ext.as_str()) {
                let _ = this.update_in(cx, |ws, window, _cx| {
                    ws.export_status =
                        Some(format!("不支持的导入格式 .{ext}（docx/csv/xlsx/html/pdf/pptx/epub/json/xml）").into());
                    window.refresh();
                });
                return;
            }
            let src_s = src.to_string_lossy().to_string();
            let md = cx
                .background_executor()
                .spawn(async move { solomd_core::convert::convert_file_to_markdown_inner(src_s) })
                .await;
            let _ = this.update_in(cx, |ws, window, cx| match md {
                Ok(markdown) => {
                    // 落盘：<stem>.md 在 vault 根（重名追加序号），再开标签。
                    let stem = src.file_stem().map(|s| s.to_string_lossy().to_string())
                        .unwrap_or_else(|| "imported".into());
                    let mut target = ws.vault.root.join(format!("{stem}.md"));
                    let mut i = 1;
                    while target.exists() {
                        target = ws.vault.root.join(format!("{stem}-{i}.md"));
                        i += 1;
                    }
                    if let Err(e) = ws.vault.write_note(&target, &markdown) {
                        ws.export_status = Some(format!("导入落盘失败：{e}").into());
                    } else {
                        ws.open_note(target, window, cx);
                        ws.export_status = Some("导入完成".into());
                        // 刷新文件树（新文件落盘后树不知道）。
                        ws.file_tree.update(cx, |_, cx| cx.notify());
                    }
                    window.refresh();
                }
                Err(e) => {
                    ws.export_status = Some(format!("导入失败：{e}").into());
                    window.refresh();
                }
            });
        })
        .detach();
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
            .on_action(cx.listener(|this, _: &ImportFile, window, cx| {
                this.import_file(window, cx);
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
                            Button::new("tasks-panel")
                                .label(if self.right_tab == RightTab::Tasks { "任务 ▾" } else { "任务" })
                                .xsmall()
                                .ghost()
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.right_tab = if this.right_tab == RightTab::Tasks {
                                        RightTab::None
                                    } else {
                                        RightTab::Tasks
                                    };
                                    if this.tasks_panel.is_none() && this.right_tab == RightTab::Tasks {
                                        let panel = cx.new(|cx| {
                                            crate::tasks_panel::TasksPanel::new(
                                                this.vault.clone(),
                                                window,
                                                cx,
                                            )
                                        });
                                        this.tasks_panel = Some(panel.clone());
                                        cx.subscribe_in(
                                            &panel,
                                            window,
                                            |this, _, ev: &crate::tasks_panel::TaskEvent, window, cx| {
                                                match ev {
                                                    crate::tasks_panel::TaskEvent::OpenNote { path, line } => {
                                                        this.open_note_at(path.clone(), *line, window, cx);
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
                            RightTab::Tasks => self
                                .tasks_panel
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
    /// ⌘P 浮层：半透明遮罩 + vendored gpc CommandPalette（搜索/过滤/键控全内置）。
    fn render_palette_overlay(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if !self.palette_open {
            return div().into_any_element();
        }
        // 条目与 palette_paths 按序一一对应；过滤、滚动、Enter/Esc/↑↓ 由
        // Command 组件承担，on_confirm 按 IndexPath.row 取回路径。
        let paths = self.vault.walk_notes();
        self.palette_paths = paths.clone();
        let items: Vec<CommandItem> = paths
            .iter()
            .map(|p| {
                let name = p
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default();
                CommandItem::new()
                    .label(name)
                    .keywords([p.to_string_lossy().to_string()])
            })
            .collect();
        let weak = cx.weak_entity();
        let weak_cancel = weak.clone();
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
                    .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| {
                        cx.stop_propagation();
                    })
                    .child(
                        Command::new(&self.command_state)
                            .placeholder("搜索笔记…")
                            .max_h(px(420.))
                            .items(items)
                            .empty(|_, _, _| {
                                div().px_3().py_2().text_sm().child("无匹配笔记")
                            })
                            .on_confirm(move |ix, window, cx| {
                                let _ = weak.update(cx, |ws, cx| {
                                    if ix.section == 0
                                        && let Some(path) = ws.palette_paths.get(ix.row).cloned()
                                    {
                                        ws.close_palette(cx);
                                        ws.open_note(path, window, cx);
                                    }
                                });
                            })
                            .on_cancel(move |_window, cx| {
                                let _ = weak_cancel.update(cx, |ws, cx| ws.close_palette(cx));
                            }),
                    ),
            )
            .into_any_element()
    }
}
