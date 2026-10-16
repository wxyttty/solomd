//! note_panel.rs — 一篇笔记 = 一个 Dock 面板（gpui-editor + 保存/脏态）。

mod math_edit;
mod prop_edit;

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use gpui::{
    actions, div, px, point, AppContext as _, ClipboardItem, Context, EventEmitter, Focusable,
    InteractiveElement as _, IntoElement, ParentElement as _, Pixels, Render, ScrollHandle,
    SharedString, StatefulInteractiveElement as _, Styled as _, Window,
};
use gpui_component::dock::{BasePanel, Panel, PanelEvent};
use gpui_component::ActiveTheme as _;
use gpui::EntityInputHandler as _;
use zorite_editor::EditorEvent;

use crate::images::ImageStore;
use crate::math::MathStore;
use crate::mermaid::MermaidStore;
use crate::vault::Vault;

actions!(solomd, [SaveNote, NewNote, PasteImage, ToggleFind, CloseFind]);

pub struct NotePanel {
    pub path: PathBuf,
    pub title: String,
    pub dirty: bool,
    pub state: gpui::Entity<zorite_editor::EditorState>,
    vault: Vault,
    _changed_sub: gpui::Subscription,
    on_removed: Option<Box<dyn Fn(&mut gpui::App)>>,
    /// 标签被激活时通知 Workspace（活跃笔记跟踪）。
    on_activated: Option<Box<dyn Fn(&mut gpui::App)>>,
    proof_issues: usize,
    /// 打字机模式：光标行垂直居中（Settings.typewriter 持久化）。
    typewriter: bool,
    /// AutoGit：⌘S 保存后自动提交（Settings.auto_git 持久化；提交在
    /// Workspace 组合层做，见 on_saved 回调）。
    auto_git: bool,
    /// 笔记滚动容器（编辑器 auto-grow，滚动在宿主侧；打字机/补偿器共用）。
    scroll: ScrollHandle,
    /// 公式渲染缓存（Workspace 持有，开窗热身/提交后重渲共用）。
    math: Rc<RefCell<MathStore>>,
    /// 打开中的结构化公式编辑会话（math_edit.rs）。
    math_edit: Option<math_edit::MathEditSession>,
    /// 打开中的属性面板编辑会话（prop_edit.rs）。
    prop_edit: Option<prop_edit::PropEditSession>,
    /// [[wikilink]] 点击 → Workspace 打开/新建目标笔记。
    on_open_note: Option<Box<dyn Fn(&mut Window, &mut gpui::App, PathBuf)>>,
    /// ⌘S 保存成功后通知 Workspace（AutoGit 提交在组合层做，事件链才通）。
    on_saved: Option<Box<dyn Fn(&mut gpui::App, PathBuf)>>,
    /// Esc 关查找条后转发关 ⌘P 浮层（propagate 不会跨 action 类型，
    /// 显式回调接线）。
    on_close_palette: Option<Box<dyn Fn(&mut gpui::App)>>,
    /// ⌘F 查找条：打开时输入框 + 匹配导航（zorite set_search 高亮）。
    find_open: bool,
    /// 查找输入框（首次打开时创建；订阅只挂一次，随实体生命周期）。
    find_input: Option<gpui::Entity<gpui_component::input::InputState>>,
    /// find_input 的 Change 订阅（重复 toggle 不重复挂）。
    _find_sub: Option<gpui::Subscription>,
    find_matches: Vec<std::ops::Range<usize>>,
    find_active: Option<usize>,
    /// 替换输入框 + 替换模式（⌥⌘F 展开；替换当前/全部）。
    replace_input: Option<gpui::Entity<gpui_component::input::InputState>>,
}

impl NotePanel {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        path: PathBuf,
        vault: Vault,
        images: Rc<RefCell<ImageStore>>,
        mermaid: Rc<RefCell<MermaidStore>>,
        math: Rc<RefCell<MathStore>>,
        typewriter: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let content = vault.read_note(&path).unwrap_or_default();
        let scroll = ScrollHandle::new();
        let title = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "untitled".into());
        let auto_git = crate::settings::Settings::load(&vault.root).auto_git;

        let state = cx.new(|cx| {
            let mut ed = zorite_editor::EditorState::new(window, cx).with_text(content.clone());
            let mono = cx.theme().mono_font_family.clone();
            ed.set_markdown_style(crate::theme::warm_light(mono), cx);

            {
                let store = images.clone();
                ed.set_block_image_provider(move |src| {
                    store.borrow_mut().get(src)
                });
            }
            {
                let store = mermaid.clone();
                ed.set_block_mermaid_provider(move |source| {
                    store.borrow().get(&SharedString::from(source))
                });
            }
            {
                let store = math.clone();
                ed.set_block_math_provider(move |source| {
                    store.borrow().get(&SharedString::from(source))
                });
                // 行内 `$…$` 复用块级光栅（按此 em 排版、缩放到文字尺寸）。
                ed.set_block_math_em(crate::math::FONT_SIZE);
            }
            // 滚动锚定：异步块（math/mermaid/图片）在视口上方完成渲染改变高度时，
            // 编辑器把高度差交给宿主，滚动容器吸收之，阅读位置不动。
            {
                let scroll = scroll.clone();
                ed.set_scroll_compensator(move |delta, _, _| {
                    let off = scroll.offset();
                    scroll.set_offset(point(off.x, off.y - delta));
                });
            }
            ed
        });

        // 驱动 mermaid 离线程渲染（图片走 provider 同步解码）。
        {
            let text = content.clone();
            let mermaid = mermaid.clone();
            let svg = cx.svg_renderer();
            cx.spawn_in(window, async move |this, cx| {
                for source in zorite_editor::mermaid_sources(&text) {
                    if !mermaid.borrow_mut().begin(&source) {
                        continue;
                    }
                    let store = mermaid.clone();
                    let src = source.to_string();
                    let svg = svg.clone();
                    let result = cx
                        .background_executor()
                        .spawn(async move {
                            crate::mermaid::render_to_image(&src, &svg, crate::mermaid::RASTER_SCALE)
                        })
                        .await;
                    store.borrow_mut().finish(source, result);
                    let _ = this.update(cx, |_, cx| {
                        cx.notify();
                        cx.refresh_windows();
                    });
                }
            })
            .detach();
        }

        // 驱动数学离线程渲染（块级 + 行内；同 key 只排一次）。
        {
            let text = content.clone();
            let math = math.clone();
            let color = cx.theme().foreground;
            math.borrow_mut().set_color(color);
            cx.spawn_in(window, async move |this, cx| {
                let sources = zorite_editor::math_sources(&text)
                    .into_iter()
                    .chain(zorite_editor::inline_math_sources(&text));
                for source in sources {
                    if !math.borrow_mut().begin(&source) {
                        continue;
                    }
                    let store = math.clone();
                    let src = source.to_string();
                    let result = cx
                        .background_executor()
                        .spawn(async move { crate::math::render_to_image(&src, color) })
                        .await;
                    store.borrow_mut().finish(source, result);
                    let _ = this.update(cx, |_, cx| {
                        cx.notify();
                        cx.refresh_windows();
                    });
                }
            })
            .detach();
        }

        let _changed_sub = cx.subscribe_in(
            &state,
            window,
            |this, _, ev: &EditorEvent, window, cx| match ev {
                EditorEvent::Changed => {
                    if !this.dirty {
                        this.dirty = true;
                        cx.notify();
                    }
                }
                EditorEvent::PreviewImage(src) => {
                    let p = this.vault.resolve(std::path::Path::new(src.as_ref()));
                    let _ = std::process::Command::new("open").arg(p).spawn();
                }
                EditorEvent::OpenLink(url) => {
                    let _ = std::process::Command::new("open").arg(url.as_ref()).spawn();
                }
                EditorEvent::EditMath {
                    range,
                    source,
                    at_end,
                    inline,
                } => {
                    this.open_math_edit(range.clone(), source.clone(), *at_end, *inline, window, cx);
                }
                EditorEvent::OpenWikiLink(target) => {
                    this.open_wiki_target(target.to_string(), window, cx);
                }
                EditorEvent::MathMenu { source, .. } => {
                    // v1：右键公式直接复制 LaTeX（完整菜单——对齐/导出——后置）。
                    cx.write_to_clipboard(ClipboardItem::new_string(source.to_string()));
                }
                EditorEvent::SelectionChanged => {
                    this.typewriter_scroll(window, cx);
                }
                EditorEvent::EditProperties {
                    range,
                    source,
                    at_end,
                    row,
                } => {
                    this.open_prop_edit(range.clone(), source.clone(), *at_end, *row, window, cx);
                }
                _ => {}
            },
        );

        Self {
            path,
            title,
            dirty: false,
            state,
            vault,
            _changed_sub,
            on_removed: None,
            on_activated: None,
            proof_issues: 0,
            typewriter,
            auto_git,
            scroll,
            math,
            math_edit: None,
            prop_edit: None,
            on_open_note: None,
            on_saved: None,
            on_close_palette: None,
            find_open: false,
            find_input: None,
            _find_sub: None,
            find_matches: Vec::new(),
            find_active: None,
            replace_input: None,
        }
    }

    /// Workspace 用来在标签被 UI 关闭时同步注册表。
    pub fn on_removed_notify(&mut self, f: Box<dyn Fn(&mut gpui::App)>) {
        self.on_removed = Some(f);
    }

    /// Workspace 用来接收激活事件（跟踪当前笔记，供历史/AI/导出使用）。
    pub fn on_activated_notify(&mut self, f: Box<dyn Fn(&mut gpui::App)>) {
        self.on_activated = Some(f);
    }

    /// Workspace 用来接收保存事件（AutoGit 提交在组合层监听后触发，
    /// git-committed 事件 → on-commit recipes 的链路才真实可用）。
    pub fn on_saved_notify(&mut self, f: Box<dyn Fn(&mut gpui::App, PathBuf)>) {
        self.on_saved = Some(f);
    }

    /// Workspace 用来在 Esc 关查找条后关 ⌘P 浮层。
    pub fn on_close_palette_notify(&mut self, f: Box<dyn Fn(&mut gpui::App)>) {
        self.on_close_palette = Some(f);
    }

    /// Workspace 用来打开 [[wikilink]] 目标（解析后可能新建草稿）。
    pub fn on_open_note_notify(
        &mut self,
        f: Box<dyn Fn(&mut Window, &mut gpui::App, PathBuf)>,
    ) {
        self.on_open_note = Some(f);
    }

    /// [[链接]] 点击：索引解析（stem → 标题 → 别名 → 路径后缀，上游 10e1860）
    /// → 打开；未命中 → vault 根新建草稿。目录分量视为 vault 路径而非相对路径。
    fn open_wiki_target(&mut self, target: String, window: &mut Window, cx: &mut Context<Self>) {
        let resolved = solomd_core::workspace_index::workspace_index_resolve(target.clone())
            .ok()
            .flatten();
        let path = match resolved {
            Some(p) => PathBuf::from(p),
            None => {
                let stem = target
                    .rsplit(['/', '\\'])
                    .next()
                    .unwrap_or(&target)
                    .trim_end_matches(".md")
                    .to_string();
                if stem.is_empty() {
                    return;
                }
                let p = self.vault.root.join(format!("{stem}.md"));
                if !p.exists() {
                    let _ = self.vault.write_note(&p, &format!("# {stem}\n\n"));
                }
                p
            }
        };
        if let Some(cb) = &self.on_open_note {
            cb(window, cx, path);
        }
    }

    /// 打字机模式：光标行垂直居中。SelectionChanged 驱动；视口高按窗口高
    /// 扣固定 chrome 估算（±20px 偏差对居中体感无影响）。编辑器末次布局
    /// 缓存提供 caret/内容几何，首次绘制前自动跳过。
    fn typewriter_scroll(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.typewriter {
            return;
        }
        const CHROME: f32 = 72.0;
        let viewport_h = window.viewport_size().height - px(CHROME);
        let target = self.state.update(cx, |ed, _| -> Option<Pixels> {
            let caret = ed.caret_screen_bounds()?;
            let top = ed.window_top()?;
            let content_h = ed.content_height()?;
            let content_y = caret.top() - top;
            let max = (content_h - viewport_h).max(px(0.0));
            Some((content_y - viewport_h / 2.0).clamp(px(0.0), max))
        });
        if let Some(target) = target {
            let off = self.scroll.offset();
            if (target - off.y).abs() > px(1.0) {
                self.scroll.set_offset(point(off.x, target));
            }
        }
    }

    /// 中文校对：cjk_proofread 全量检测 → squiggles + 计数。
    pub fn proofread(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let text = self.state.read(cx).text().to_string();
        let issues = solomd_core::cjk_proofread::proofread(&text);
        self.proof_issues = issues.len();
        let diags = issues
            .into_iter()
            .map(|i| zorite_editor::Diagnostic {
                range: i.col_start as usize..i.col_end as usize,
            })
            .collect();
        self.state.update(cx, |ed, cx| ed.set_diagnostics(diags, cx));
        let _ = window;
        cx.notify();
    }

    /// 明暗切换时重装 SyntaxStyle（Workspace 广播调用）。
    pub fn apply_theme(&mut self, dark: bool, cx: &mut Context<Self>) {
        let mono = cx.theme().mono_font_family.clone();
        self.state.update(cx, |ed, cx| {
            ed.set_markdown_style(crate::theme::warm(dark, mono), cx);
        });
        cx.notify();
    }

    /// 跳到字节偏移（大纲/反链跳转）：移光标 + 聚焦。
    pub fn jump_to_offset(&mut self, offset: usize, window: &mut Window, cx: &mut Context<Self>) {
        self.state.update(cx, |ed, cx| ed.set_cursor(offset, cx));
        self.state.update(cx, |ed, cx| ed.focus(window, cx));
        cx.notify();
    }

    /// 当前围栏快照（编辑器缓冲优先）。
    pub fn board_snapshot(&self, cx: &Context<Self>) -> Option<String> {
        let text = self.state.read(cx).text();
        crate::whiteboard::find_board_fence(text)
            .map(|(_, b)| text[b].trim().to_string())
    }

    /// 确保笔记里有白板围栏（没有就在文末补一个并保存），返回初始快照。
    pub fn ensure_board_fence(&mut self, cx: &mut Context<Self>) -> String {
        let text = self.state.read(cx).text().to_string();
        if let Some((_, b)) = crate::whiteboard::find_board_fence(&text) {
            return text[b].trim().to_string();
        }
        let id = format!("board-{}", now_compact());
        let block =
            format!("\n\n```whiteboard id=\"{id}\" height=\"520\"\n{{}}\n```\n");
        let next = text + &block;
        self.state.update(cx, |ed, cx| ed.set_text(next.clone(), cx));
        if let Err(e) = self.vault.write_note(&self.path, &next) {
            eprintln!("[board] 写入围栏失败 {}: {e}", self.path.display());
        }
        self.dirty = false;
        self.board_snapshot(cx).unwrap_or_else(|| "{}".into())
    }

    /// 白板快照回写：重写围栏体（info 行逐字节保留）并落盘。
    pub fn write_board_snapshot(&mut self, body: &str, cx: &mut Context<Self>) {
        let text = self.state.read(cx).text().to_string();
        let Some((info, old)) = crate::whiteboard::find_board_fence(&text) else {
            return;
        };
        let info_line = text[info].to_string();
        let mut next = String::with_capacity(text.len() + body.len());
        next.push_str(&text[..old.start]);
        next.push_str(body);
        if !body.ends_with('\n') {
            next.push('\n');
        }
        next.push_str(&text[old.end..]);
        // 不变量：重写后围栏 info 行必须逐字节一致。
        let preserved = crate::whiteboard::find_board_fence(&next)
            .map(|(info2, _)| next[info2] == info_line)
            .unwrap_or(false);
        if !preserved {
            eprintln!("[board] 围栏 info 行未保留，跳过回写");
            return;
        }
        self.state.update(cx, |ed, cx| ed.set_text(next.clone(), cx));
        if let Err(e) = self.vault.write_note(&self.path, &next) {
            eprintln!("[board] 回写失败 {}: {e}", self.path.display());
            return;
        }
        self.dirty = false;
        cx.notify();
    }

    fn save(&mut self, _: &SaveNote, _: &mut Window, cx: &mut Context<Self>) {
        let text = self.state.read(cx).text().to_string();
        match self.vault.write_note(&self.path, &text) {
            Ok(()) => {
                self.dirty = false;
                // AutoGit 提交不在面板内做（NullSink 会丢 git-committed 事件，
                // on-commit recipes 永不触发）——发保存通知，由 Workspace
                // 组合层执行（见 workspace.rs 的 on_saved 回调）。
                if let Some(cb) = &self.on_saved {
                    let path = self.path.clone();
                    cb(cx, path);
                }
                cx.notify();
            }
            Err(e) => eprintln!("[save] {}: {e}", self.path.display()),
        }
    }

    /// ⌘F：开/关查找条。首次打开时创建输入框并预填当前选区（若有）。
    fn toggle_find(&mut self, _: &ToggleFind, window: &mut Window, cx: &mut Context<Self>) {
        self.find_open = !self.find_open;
        if self.find_open {
            // 首次打开：创建输入框 + 挂一次 Change 订阅（存 Subscription，
            // 重复 toggle 不重复挂——否则每次打开都多一个订阅，run_find
            // 被多次触发）。替换输入框同批创建，⌥⌘F 只是展开显示。
            if self.find_input.is_none() {
                let input = cx.new(|cx| {
                    gpui_component::input::InputState::new(window, cx).placeholder("查找")
                });
                self._find_sub = Some(cx.subscribe_in(
                    &input,
                    window,
                    |this, _, ev: &gpui_component::input::InputEvent, _, cx| {
                        if matches!(ev, gpui_component::input::InputEvent::Change) {
                            this.run_find(cx);
                        }
                    },
                ));
                self.find_input = Some(input);
                self.replace_input = Some(cx.new(|cx| {
                    gpui_component::input::InputState::new(window, cx).placeholder("替换为")
                }));
            }
            if let Some(input) = self.find_input.clone() {
                input.update(cx, |s, cx| s.focus(window, cx));
            }
            self.run_find(cx);
        } else {
            // 关闭：清高亮，焦点回编辑器。
            self.state.update(cx, |ed, cx| ed.set_search(Vec::new(), None, cx));
        }
        cx.notify();
    }

    /// Esc：关闭查找条（焦点在查找输入框时，escape 在输入上下文里被
    /// InputState 消费不到面板级——绑定在 Editor 上下文外全局生效，
    /// 查找条开着时优先关它，再冒泡给编辑器原有 Dismiss 行为）。
    fn close_find(&mut self, _: &CloseFind, window: &mut Window, cx: &mut Context<Self>) {
        if self.find_open {
            self.find_open = false;
            self.state.update(cx, |ed, cx| ed.set_search(Vec::new(), None, cx));
            // 焦点回编辑器。
            self.state.update(cx, |ed, cx| ed.focus(window, cx));
            cx.notify();
            return;
        }
        // 未开查找条：显式派发 ClosePalette（cx.propagate 只继续同一种
        // action，不会变成另一种——之前“Esc 链”的表述是错的）。⌘P 浮层
        // 开着则关它；再未开则 stop（编辑器菜单的 Esc 是输入上下文里的
        // 另一 action，不受影响）。
        if let Some(cb) = &self.on_close_palette {
            cb(cx);
        } else {
            cx.stop_propagation();
        }
    }

    /// ⌘S 保存后 AutoGit 提交的并发防护：连续快速保存会并发跑多个
    /// git_auto_commit 任务访问同一仓库。busy 标记在提交任务结束时清零
    /// （含失败路径），期间新保存只跳过本轮提交（文件已落盘，下一轮
    /// 保存会把增量一并提交——git 快照语义下无丢失）。
    /// （状态存 Workspace.auto_git_running，见 workspace.rs。）

    /// 替换当前匹配：replace_range 后重跑查找（偏移已变，旧匹配全部作废）。
    fn replace_current(&mut self, cx: &mut Context<Self>) {
        let Some(replace) = self.replace_input.clone() else { return };
        let replacement = replace.read(cx).value().to_string();
        let Some(active) = self.find_active else { return };
        let Some(m) = self.find_matches.get(active).cloned() else { return };
        self.state.update(cx, |ed, cx| {
            ed.replace_range(m, &replacement, cx);
        });
        self.dirty = true;
        self.run_find(cx);
    }

    /// 替换全部：从后往前替换（前面的偏移不受影响），一次 undo 语义
    /// 逐次 replace_range（zorite 每次独立记录 undo 步）。
    /// 替换全部：一次性构造新全文 + 单次 replace_range（一个 undo 步，
    /// 上游 replace-all 的交互语义）。从后往前的逐次替换会产生 N 个独立
    /// undo 步，用户要按 N 次 ⌘Z 才能撤销一次“全部替换”。
    fn replace_all(&mut self, cx: &mut Context<Self>) {
        let Some(replace) = self.replace_input.clone() else { return };
        let replacement = replace.read(cx).value().to_string();
        if self.find_matches.is_empty() {
            return;
        }
        let text = self.state.read(cx).text().to_string();
        let matches = self.find_matches.clone();
        // 从后往前拼接：后面的偏移不受前面替换影响。
        let mut new_text = String::with_capacity(text.len());
        let mut cursor = 0usize;
        let mut last_end = 0usize;
        for m in &matches {
            new_text.push_str(&text[last_end..m.start]);
            new_text.push_str(&replacement);
            last_end = m.end;
            cursor = m.start + replacement.len();
        }
        new_text.push_str(&text[last_end..]);
        let full = 0..text.len();
        self.state.update(cx, |ed, cx| {
            ed.replace_range(full, &new_text, cx);
            // 光标落在最后一个替换的尾部（上游行为）。
            ed.set_cursor(cursor, cx);
        });
        self.dirty = true;
        self.run_find(cx);
    }

    /// 重新执行查找：zorite find_in_source（大小写不敏感）→ set_search 高亮
    /// + 跳到第一个匹配（光标落匹配起点）。
    fn run_find(&mut self, cx: &mut Context<Self>) {
        let Some(input) = self.find_input.clone() else { return };
        let query = input.read(cx).value().to_string();
        let text = self.state.read(cx).text().to_string();
        self.find_matches = if query.is_empty() {
            Vec::new()
        } else {
            zorite_editor::find_in_source(&text, &query)
        };
        self.find_active = (!self.find_matches.is_empty()).then_some(0);
        let matches = self.find_matches.clone();
        let active = self.find_active;
        self.state.update(cx, |ed, cx| {
            ed.set_search(matches, active, cx);
        });
        // 光标跳到首个匹配（编辑器会滚动到光标行——打字机/补偿器路径）。
        if let Some(first) = self.find_matches.first() {
            self.state.update(cx, |ed, cx| ed.set_cursor(first.start, cx));
        }
        cx.notify();
    }

    /// 查找条上的 ▲/▼：在匹配间循环导航。
    fn find_step(&mut self, forward: bool, cx: &mut Context<Self>) {
        if self.find_matches.is_empty() {
            return;
        }
        let n = self.find_matches.len();
        let cur = self.find_active.unwrap_or(0);
        let next = if forward {
            (cur + 1) % n
        } else {
            (cur + n - 1) % n
        };
        self.find_active = Some(next);
        let matches = self.find_matches.clone();
        let active = self.find_active;
        self.state.update(cx, |ed, cx| {
            ed.set_search(matches, active, cx);
        });
        if let Some(m) = self.find_matches.get(next) {
            self.state.update(cx, |ed, cx| ed.set_cursor(m.start, cx));
        }
        cx.notify();
    }
}

impl BasePanel for NotePanel {
    fn panel_name(&self) -> &'static str {
        "note"
    }

    fn set_active(&mut self, active: bool, window: &mut Window, cx: &mut Context<Self>) {
        if active {
            self.state.update(cx, |ed, cx| ed.focus(window, cx));
            if let Some(f) = &self.on_activated {
                f(cx);
            }
        }
    }

    fn on_removed(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        if let Some(f) = self.on_removed.take() {
            // Context<NotePanel> derefs 到 App；回调里用它更新 Workspace。
            f(cx);
        }
    }
}

impl Panel for NotePanel {
    fn title(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        if self.dirty {
            format!("• {}", self.title)
        } else {
            self.title.clone()
        }
        .into_any_element()
    }
}

impl Render for NotePanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        use gpui_component::button::{Button, ButtonVariants as _};
        use gpui_component::{h_flex, v_flex, Sizable as _};
        div()
            .id("note-panel")
            .size_full()
            .on_action(cx.listener(Self::save))
            .on_action(cx.listener(Self::on_paste_image))
            .on_action(cx.listener(Self::toggle_find))
            .on_action(cx.listener(Self::close_find))
            .text_size(px(16.))
            .child(
                v_flex()
                    .size_full()
                    .child(
                        h_flex()
                            .px_2()
                            .py_1()
                            .gap_2()
                            .border_b_1()
                            .border_color(cx.theme().border)
                            .child(
                                Button::new("proofread")
                                    .label("中文校对")
                                    .xsmall()
                                    .ghost()
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.proofread(window, cx);
                                    })),
                            )
                            .child(
                                Button::new("open-board")
                                    .label("白板")
                                    .xsmall()
                                    .ghost()
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        let snapshot = this.ensure_board_fence(cx);
                                        let weak = cx.entity().downgrade();
                                        let title = this.title.clone();
                                        crate::whiteboard::open_board(
                                            weak,
                                            title,
                                            snapshot,
                                            cx,
                                        );
                                    })),
                            )
                            .child(
                                Button::new("autogit")
                                    .label(if self.auto_git { "AutoGit ✓" } else { "AutoGit" })
                                    .xsmall()
                                    .ghost()
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.auto_git = !this.auto_git;
                                        let mut s =
                                            crate::settings::Settings::load(&this.vault.root);
                                        s.auto_git = this.auto_git;
                                        s.save(&this.vault.root);
                                        cx.notify();
                                    })),
                            )
                            .child(
                                Button::new("typewriter")
                                    .label(if self.typewriter { "打字机 ✓" } else { "打字机" })
                                    .xsmall()
                                    .ghost()
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.typewriter = !this.typewriter;
                                        let mut s =
                                            crate::settings::Settings::load(&this.vault.root);
                                        s.typewriter = this.typewriter;
                                        s.save(&this.vault.root);
                                        if this.typewriter {
                                            this.typewriter_scroll(window, cx);
                                        }
                                        cx.notify();
                                    })),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(if self.proof_issues > 0 {
                                        format!("{} 处建议（波浪线标注，右键查看）", self.proof_issues)
                                    } else {
                                        String::new()
                                    }),
                            ),
                    )
                    // 查找/替换条（⌘F）：查找输入 + 匹配计数 + ▲/▼ + 替换行。
                    .children(self.find_open.then(|| {
                        let input = self.find_input.clone().unwrap();
                        let n = self.find_matches.len();
                        let cur = self.find_active.map(|i| i + 1).unwrap_or(0);
                        v_flex()
                            .id("find-bar")
                            .border_b_1()
                            .border_color(cx.theme().border)
                            .child(
                                h_flex()
                                    .id("find-row-find")
                                    .px_2()
                                    .py_1()
                                    .gap_1()
                                    .child(
                                        div().flex_1().min_w_0().child(
                                            gpui_component::input::Input::new(&input).small(),
                                        ),
                                    )
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(cx.theme().muted_foreground)
                                            .min_w(px(48.))
                                            .child(if n > 0 {
                                                format!("{cur}/{n}")
                                            } else {
                                                "0/0".to_string()
                                            }),
                                    )
                                    .child(
                                        Button::new("find-prev")
                                            .label("▲")
                                            .xsmall()
                                            .ghost()
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.find_step(false, cx);
                                            })),
                                    )
                                    .child(
                                        Button::new("find-next")
                                            .label("▼")
                                            .xsmall()
                                            .ghost()
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.find_step(true, cx);
                                            })),
                                    )
                                    .child(
                                        Button::new("find-close")
                                            .label("✕")
                                            .xsmall()
                                            .ghost()
                                            .on_click(cx.listener(|this, _, _window, cx| {
                                                this.find_open = false;
                                                this.state.update(cx, |ed, cx| {
                                                    ed.set_search(Vec::new(), None, cx)
                                                });
                                                cx.notify();
                                            })),
                                    ),
                            )
                            // 替换行：输入 + 替换当前 + 全部替换。
                            .child(
                                h_flex()
                                    .id("find-row-replace")
                                    .px_2()
                                    .pb_1()
                                    .gap_1()
                                    .child(
                                        div().flex_1().min_w_0().child(
                                            gpui_component::input::Input::new(
                                                &self.replace_input.clone().unwrap(),
                                            )
                                            .small(),
                                        ),
                                    )
                                    .child(
                                        Button::new("replace-current")
                                            .label("替换")
                                            .xsmall()
                                            .ghost()
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.replace_current(cx);
                                            })),
                                    )
                                    .child(
                                        Button::new("replace-all")
                                            .label("全部")
                                            .xsmall()
                                            .ghost()
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.replace_all(cx);
                                            })),
                                    ),
                            )
                    }))
                    .child(
                        div()
                            .id("note-scroll")
                            .flex_1()
                            .min_h_0()
                            .track_scroll(&self.scroll)
                            .overflow_y_scroll()
                            .child(self.state.clone()),
                    ),
            )
    }
}

impl NotePanel {
    /// ⌘V：剪贴板有图片/图片文件 → 存 `_assets/` → 光标处插入 markdown。
    /// 无图时 propagate 给编辑器原生文本粘贴（crate 对图片剪贴板也放行）。
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

        // Finder 拷贝的图片文件
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
                if let Some(rel) = import_to_assets(path, &self.vault.root) {
                    let name = file_stem(&rel);
                    let md = format!("\n![{name}]({rel})\n");
                    self.state
                        .update(cx, |ed, cx| ed.replace_text_in_range(None, &md, window, cx));
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
        let rel = format!("_assets/paste-{}.{ext}", now_compact());
        // 落盘到 vault 根（Finder 启动时 cwd 是只读的 /，不能依赖 cwd）。
        let abs = self.vault.root.join(&rel);
        match std::fs::create_dir_all(abs.parent().unwrap_or(&self.vault.root))
            .and_then(|()| std::fs::write(&abs, &bytes))
        {
            Ok(()) => {
                let name = file_stem(&rel);
                let md = format!("\n![{name}]({rel})\n");
                self.state
                    .update(cx, |ed, cx| ed.replace_text_in_range(None, &md, window, cx));
                cx.notify();
            }
            Err(e) => {
                eprintln!("[paste] 写盘失败 {}: {e}", abs.display());
                cx.propagate();
            }
        }
    }
}

fn file_stem(p: &str) -> &str {
    std::path::Path::new(p)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("image")
}

fn now_compact() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// 拷贝外部图片文件进 vault 的 `_assets/`（按文件名去重）。
fn import_to_assets(src: &std::path::Path, vault_root: &std::path::Path) -> Option<String> {
    let dest_dir = vault_root.join("_assets");
    std::fs::create_dir_all(&dest_dir).ok()?;
    let name = src.file_name()?.to_str()?.to_string();
    let dest = dest_dir.join(&name);
    if !dest.exists() {
        std::fs::copy(src, &dest).ok()?;
    }
    Some(format!("_assets/{name}"))
}

impl EventEmitter<PanelEvent> for NotePanel {}

impl Focusable for NotePanel {
    fn focus_handle(&self, cx: &gpui::App) -> gpui::FocusHandle {
        self.state.read(cx).focus_handle(cx)
    }
}
