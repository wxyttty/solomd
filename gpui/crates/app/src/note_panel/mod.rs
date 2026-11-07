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
    /// 文件名（标题开关关闭时直接用它）。
    pub file_name: String,
    /// 标签显示标题（开关开时取文档首行，随编辑刷新）。
    pub title: String,
    /// 标题是否取文档首行（Settings.tab_title_from_first_line 快照）。
    title_from_line: bool,
    pub dirty: bool,
    pub state: gpui::Entity<zorite_editor::EditorState>,
    vault: Vault,
    _changed_sub: gpui::Subscription,
    on_removed: Option<Box<dyn Fn(&mut gpui::App)>>,
    /// 标签被激活时通知 Workspace（活跃笔记跟踪）。
    on_activated: Option<Box<dyn Fn(&mut gpui::App)>>,
    proof_issues: usize,
    /// LanguageTool 语法检查：开关状态 + 最近一批问题（含替换建议）。
    lt_enabled: bool,
    lt_matches: Vec<LtIssue>,
    /// 语法检查运行中（防重入 + 状态显示）。
    lt_running: bool,
    /// 已排队但未执行的检查（编辑期间防抖；在途时置位，完成后补跑）。
    lt_pending: bool,
    /// 防抖代数（每次编辑 +1；定时器醒来只有最新一代才执行）。
    lt_debounce: u64,
    /// 本次会话已确认过“原文件不存在仍要保存”（避免每次 ⌘S 都弹）。
    save_missing_confirmed: bool,
    /// Settings.confirm_save_missing_file 快照（构造时读一次）。
    confirm_save_missing: bool,
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

/// 一条 LanguageTool 问题（已转成字节偏移，可直接喂编辑器诊断）。
#[derive(Clone)]
pub struct LtIssue {
    pub start: usize,
    pub end: usize,
    pub message: String,
    pub replacements: Vec<String>,
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
        let file_name = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "untitled".into());
        let settings = crate::settings::Settings::load(&vault.root);
        let auto_git = settings.auto_git;
        let confirm_save_missing = settings.confirm_save_missing_file;
        let title_from_line = settings.tab_title_from_first_line;
        // 标题：开关开且首行能推出标题时用它，否则回退文件名。
        let title = if title_from_line {
            let derived = solomd_core::tab_title::extract_first_line_title(&content);
            if derived.is_empty() {
                file_name.clone()
            } else {
                derived
            }
        } else {
            file_name.clone()
        };

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
                    // 标题随首行刷新（仅开关开启时；extract 只扫 8KB，够快）。
                    if this.title_from_line {
                        let text = this.state.read(cx).text().to_string();
                        let derived = solomd_core::tab_title::extract_first_line_title(&text);
                        let next = if derived.is_empty() {
                            this.file_name.clone()
                        } else {
                            derived
                        };
                        if next != this.title {
                            this.title = next;
                            // dock 的 tab 标题由 Panel::title 每帧拉取——
                            // 需要刷新窗口才能重画 tab 栏。
                            cx.refresh_windows();
                        }
                    }
                    if !this.dirty {
                        this.dirty = true;
                        cx.notify();
                    }
                    // 语法检查开启时：编辑停止 1.5s 后补跑一次（防抖，
                    // 避免每次按键都打 LT；public 模式还有 20 次/分钟的限流）。
                    if this.lt_enabled {
                        let generation = this.lt_debounce.wrapping_add(1);
                        this.lt_debounce = generation;
                        let weak = cx.entity().downgrade();
                        cx.spawn_in(window, async move |_, cx| {
                            cx.background_executor()
                                .timer(std::time::Duration::from_millis(1500))
                                .await;
                            let Some(panel) = weak.upgrade() else { return };
                            let _ = panel.update_in(cx, |panel, window, cx| {
                                // 期间又有编辑 → 更新的一轮会自己跑。
                                if panel.lt_enabled && panel.lt_debounce == generation {
                                    panel.run_languagetool(window, cx);
                                }
                            });
                        })
                        .detach();
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
            file_name,
            title,
            title_from_line,
            dirty: false,
            state,
            vault,
            _changed_sub,
            on_removed: None,
            on_activated: None,
            proof_issues: 0,
            lt_enabled: false,
            lt_matches: Vec::new(),
            lt_running: false,
            lt_pending: false,
            lt_debounce: 0,
            save_missing_confirmed: false,
            confirm_save_missing,
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

    /// LanguageTool 语法/拼写检查：后台跑一次，把结果落成编辑器诊断。
    /// 文档整体作为单段纯文本发给 LT（不做 markup 标注——GPUI 侧拿不到
    /// 编辑器的 markdown 分词，代价是偶尔会把 Markdown 标记本身报出来；
    /// 该功能默认关闭、由用户显式开启，注释里如实标注了这个边界）。
    pub fn run_languagetool(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.lt_running {
            self.lt_pending = true;
            return;
        }
        let text = self.state.read(cx).text().to_string();
        if text.trim().is_empty() {
            self.lt_matches.clear();
            self.state.update(cx, |ed, cx| ed.set_diagnostics(Vec::new(), cx));
            cx.notify();
            return;
        }
        self.lt_running = true;
        cx.notify();

        let settings = crate::settings::Settings::load(&self.vault.root);
        let config = solomd_core::languagetool::LtConfig {
            mode: if settings.lt_mode.trim().is_empty() {
                "local".into()
            } else {
                settings.lt_mode.clone()
            },
            base_url: (!settings.lt_base_url.trim().is_empty())
                .then(|| settings.lt_base_url.clone()),
            username: (!settings.lt_username.trim().is_empty())
                .then(|| settings.lt_username.clone()),
        };
        let language = if settings.lt_language.trim().is_empty() {
            "auto".to_string()
        } else {
            settings.lt_language.clone()
        };

        cx.spawn_in(window, async move |this, cx| {
            let req = solomd_core::languagetool::CheckRequest {
                config,
                annotation: vec![solomd_core::languagetool::Segment {
                    text: Some(text.clone()),
                    markup: None,
                    interpret_as: None,
                }],
                language,
                mother_tongue: None,
                preferred_variants: None,
                level: Some(
                    if settings.lt_level.trim().is_empty() {
                        "default".into()
                    } else {
                        settings.lt_level.clone()
                    },
                ),
                disabled_rules: Vec::new(),
            };
            let result = cx
                .background_executor()
                .spawn(async move {
                    solomd_core::languagetool::languagetool_check(req).await
                })
                .await;
            let _ = this.update_in(cx, |panel, window, cx| {
                panel.lt_running = false;
                match result {
                    Ok(resp) => {
                        // UTF-16（LT/Java）→ 字节（zorite Diagnostic）。
                        let issues: Vec<LtIssue> = resp
                            .matches
                            .into_iter()
                            .filter_map(|m| {
                                solomd_core::languagetool::utf16_range_to_bytes(
                                    &text, m.offset, m.length,
                                )
                                .map(|(start, end)| LtIssue {
                                    start,
                                    end,
                                    message: m.message,
                                    replacements: m.replacements,
                                })
                            })
                            .collect();
                        let diags = issues
                            .iter()
                            .map(|i| zorite_editor::Diagnostic {
                                range: i.start..i.end,
                            })
                            .collect();
                        panel.lt_matches = issues;
                        panel.state.update(cx, |ed, cx| ed.set_diagnostics(diags, cx));
                    }
                    Err(e) => {
                        panel.lt_matches.clear();
                        panel.state.update(cx, |ed, cx| ed.set_diagnostics(Vec::new(), cx));
                        eprintln!("[languagetool] {e}");
                    }
                }
                let rerun = panel.lt_pending;
                panel.lt_pending = false;
                window.refresh();
                cx.notify();
                if rerun && panel.lt_enabled {
                    panel.run_languagetool(window, cx);
                }
            });
        })
        .detach();
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

    fn save(&mut self, _: &SaveNote, window: &mut Window, cx: &mut Context<Self>) {
        use gpui_component::WindowExt as _;
        // 上游 #9：原文件被外部删除/移动时先确认，避免“以为在改已有笔记，
        // 实际在悄悄新建一个”。开关默认关（confirmSaveMissingFile）。
        if self.confirm_save_missing
            && !self.save_missing_confirmed
            && !solomd_core::fs_file_exists(
                self.path.to_string_lossy().to_string(),
            )
        {
            let name = self.file_name.clone();
            let path = self.path.to_string_lossy().to_string();
            let weak = cx.entity().downgrade();
            window.open_alert_dialog(cx, move |alert, _, _| {
                let weak = weak.clone();
                let path = path.clone();
                let alert = alert
                    .title("原文件已不存在")
                    .description(format!(
                        "“{name}” 在磁盘上已被删除或移动。
仍要保存并重新创建它吗？
{path}"
                    ))
                    .show_cancel(true)
                    .on_ok(move |_, _, cx| {
                        if let Some(panel) = weak.upgrade() {
                            let _ = panel.update(cx, |panel, cx| {
                                panel.save_missing_confirmed = true;
                                panel.dirty = true;
                                // 直接写盘（不再走回车确认）。
                                let text = panel.state.read(cx).text().to_string();
                                match panel.vault.write_note(&panel.path, &text) {
                                    Ok(()) => {
                                        panel.dirty = false;
                                        if let Some(cb) = &panel.on_saved {
                                            let p = panel.path.clone();
                                            cb(cx, p);
                                        }
                                    }
                                    Err(e) => {
                                        eprintln!(
                                            "[save] {}: {e}",
                                            panel.path.display()
                                        )
                                    }
                                }
                                cx.notify();
                            });
                        }
                        true
                    });
                alert
            });
            return;
        }
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
                                Button::new("languagetool")
                                    .label(if self.lt_enabled {
                                        "语法检查 ✓"
                                    } else {
                                        "语法检查"
                                    })
                                    .xsmall()
                                    .ghost()
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.lt_enabled = !this.lt_enabled;
                                        if this.lt_enabled {
                                            this.run_languagetool(window, cx);
                                        } else {
                                            this.lt_matches.clear();
                                            this.state.update(cx, |ed, cx| {
                                                ed.set_diagnostics(Vec::new(), cx)
                                            });
                                        }
                                        cx.notify();
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
                                Button::new("title-from-line")
                                    .label(if self.title_from_line {
                                        "首行标题 ✓"
                                    } else {
                                        "首行标题"
                                    })
                                    .xsmall()
                                    .ghost()
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.title_from_line = !this.title_from_line;
                                        let mut s =
                                            crate::settings::Settings::load(&this.vault.root);
                                        s.tab_title_from_first_line = this.title_from_line;
                                        s.save(&this.vault.root);
                                        // 立刻按新开关重算当前标题。
                                        this.title = if this.title_from_line {
                                            let text = this.state.read(cx).text().to_string();
                                            let derived =
                                                solomd_core::tab_title::extract_first_line_title(&text);
                                            if derived.is_empty() {
                                                this.file_name.clone()
                                            } else {
                                                derived
                                            }
                                        } else {
                                            this.file_name.clone()
                                        };
                                        cx.refresh_windows();
                                        cx.notify();
                                    })),
                            )
                            .child(
                                Button::new("confirm-missing")
                                    .label(if self.confirm_save_missing {
                                        "保存前校验 ✓"
                                    } else {
                                        "保存前校验"
                                    })
                                    .xsmall()
                                    .ghost()
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.confirm_save_missing = !this.confirm_save_missing;
                                        let mut s =
                                            crate::settings::Settings::load(&this.vault.root);
                                        s.confirm_save_missing_file = this.confirm_save_missing;
                                        s.save(&this.vault.root);
                                        cx.notify();
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
                                    .child({
                                        let mut bits: Vec<String> = Vec::new();
                                        if self.proof_issues > 0 {
                                            bits.push(format!(
                                                "{} 处中文建议",
                                                self.proof_issues
                                            ));
                                        }
                                        if self.lt_running {
                                            bits.push("语法检查中…".into());
                                        } else if self.lt_enabled {
                                            bits.push(format!(
                                                "{} 处语法建议",
                                                self.lt_matches.len()
                                            ));
                                        }
                                        bits.join(" · ")
                                    }),
                            ),
                    )
                    // 语法建议行（LanguageTool）：前 3 条 + 一键替换。
                    .children((self.lt_enabled && !self.lt_matches.is_empty()).then(|| {
                        let theme = cx.theme().clone();
                        let mut row = v_flex()
                            .id("lt-suggestions")
                            .px_2()
                            .py_1()
                            .gap_0p5()
                            .border_b_1()
                            .border_color(theme.border)
                            .text_xs();
                        for (ix, issue) in self.lt_matches.iter().take(3).enumerate() {
                            let start = issue.start;
                            let end = issue.end;
                            let mut line = h_flex().gap_1().min_w_0();
                            line = line.child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .truncate()
                                    .text_color(theme.muted_foreground)
                                    .child(issue.message.clone()),
                            );
                            for (rk, repl) in issue.replacements.iter().take(2).enumerate() {
                                let repl = repl.clone();
                                line = line.child(
                                    Button::new(SharedString::from(format!(
                                        "lt-fix-{ix}-{rk}"
                                    )))
                                    .label(repl.clone())
                                    .xsmall()
                                    .on_click(cx.listener(move |this, _, window, cx| {
                                        // 应用建议：替换 → 重跑检查刷新诊断。
                                        this.state.update(cx, |ed, cx| {
                                            ed.replace_range(start..end, &repl, cx)
                                        });
                                        this.dirty = true;
                                        this.run_languagetool(window, cx);
                                        cx.notify();
                                    })),
                                );
                            }
                            row = row.child(line);
                        }
                        row
                    }))
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

        // 富文本粘贴：剪贴板里若有 HTML flavor（浏览器 / Word / 备忘录 /
        // Excel 都会放），转成 Markdown 插入——纯文本 flavor 会丢掉标题、
        // 粗体、列表、表格。
        //
        // GPUI 的 ClipboardEntry 拿不到 HTML，所以走 clipboard_html 直连
        // AppKit（见该模块文档）；拿不到就落到后面的纯文本路径。
        if clip_image(cx).is_none()
            && let Some(html) = crate::clipboard_html::read_html()
            && let Ok(md) = solomd_core::convert::html_to_markdown(&html)
            && !md.trim().is_empty()
            // 只有「确实带格式」时才接管：纯文字 HTML（每个富文本源都有）
            // 不该改写粘贴结果，避免把用户预期的原文换掉。
            && solomd_core::convert::html_has_formatting(&html)
        {
            self.state
                .update(cx, |ed, cx| ed.replace_text_in_range(None, &md, window, cx));
            self.dirty = true;
            cx.notify();
            return;
        }

        // 表格粘贴（上游 #460）：Excel / WPS / Numbers / Sheets 的选区在
        // text/plain 上是 TSV。没有 HTML flavor 的场合（如纯文本编辑器
        // 复制的表格）靠这条兜底。
        if clip_image(cx).is_none()
            && let Some(text) = cx
                .read_from_clipboard()
                .and_then(|c| c.text())
                .map(|s| s.to_string())
            && let Some(table) = solomd_core::table_paste::tsv_to_markdown(&text)
        {
            self.state
                .update(cx, |ed, cx| ed.replace_text_in_range(None, &table, window, cx));
            self.dirty = true;
            cx.notify();
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
