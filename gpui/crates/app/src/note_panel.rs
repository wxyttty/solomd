//! note_panel.rs — 一篇笔记 = 一个 Dock 面板（gpui-editor + 保存/脏态）。

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use gpui::{
    actions, div, px, AppContext as _, Context, EventEmitter, Focusable,
    InteractiveElement as _, IntoElement, ParentElement as _, Render, SharedString,
    Styled as _, Window,
};
use gpui_component::dock::{Panel, PanelEvent};
use gpui_component::ActiveTheme as _;
use gpui::EntityInputHandler as _;
use gpui_editor::EditorEvent;

use crate::images::ImageStore;
use crate::mermaid::MermaidStore;
use crate::vault::Vault;

actions!(solomd, [SaveNote, NewNote, PasteImage]);

pub struct NotePanel {
    pub path: PathBuf,
    pub title: String,
    pub dirty: bool,
    pub state: gpui::Entity<gpui_editor::EditorState>,
    vault: Vault,
    _changed_sub: gpui::Subscription,
    on_removed: Option<Box<dyn Fn(&mut gpui::App)>>,
}

impl NotePanel {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        path: PathBuf,
        vault: Vault,
        images: Rc<RefCell<ImageStore>>,
        mermaid: Rc<RefCell<MermaidStore>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let content = vault.read_note(&path).unwrap_or_default();
        let title = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "untitled".into());

        let state = cx.new(|cx| {
            let mut ed = gpui_editor::EditorState::new(window, cx).with_text(content.clone());
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
            ed
        });

        // 驱动 mermaid 离线程渲染（图片走 provider 同步解码）。
        {
            let text = content.clone();
            let mermaid = mermaid.clone();
            let svg = cx.svg_renderer();
            cx.spawn_in(window, async move |this, cx| {
                for source in gpui_editor::mermaid_sources(&text) {
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

        let _changed_sub = cx.subscribe(&state, |this, _, ev: &EditorEvent, cx| match ev {
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
            _ => {}
        });

        Self {
            path,
            title,
            dirty: false,
            state,
            vault,
            _changed_sub,
            on_removed: None,
        }
    }

    /// Workspace 用来在标签被 UI 关闭时同步注册表。
    pub fn on_removed_notify(&mut self, f: Box<dyn Fn(&mut gpui::App)>) {
        self.on_removed = Some(f);
    }

    /// 明暗切换时重装 SyntaxStyle（Workspace 广播调用）。
    pub fn apply_theme(&mut self, dark: bool, cx: &mut Context<Self>) {
        let mono = cx.theme().mono_font_family.clone();
        self.state.update(cx, |ed, cx| {
            ed.set_markdown_style(crate::theme::warm(dark, mono), cx);
        });
        cx.notify();
    }

    fn save(&mut self, _: &SaveNote, _: &mut Window, cx: &mut Context<Self>) {
        let text = self.state.read(cx).text().to_string();
        match self.vault.write_note(&self.path, &text) {
            Ok(()) => {
                self.dirty = false;
                cx.notify();
            }
            Err(e) => eprintln!("[save] {}: {e}", self.path.display()),
        }
    }
}

impl Panel for NotePanel {
    fn panel_name(&self) -> &'static str {
        "note"
    }

    fn title(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        if self.dirty {
            format!("• {}", self.title)
        } else {
            self.title.clone()
        }
        .into_any_element()
    }

    fn set_active(&mut self, active: bool, window: &mut Window, cx: &mut Context<Self>) {
        if active {
            self.state.update(cx, |ed, cx| ed.focus(window, cx));
        }
    }

    fn on_removed(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        if let Some(f) = self.on_removed.take() {
            // Context<NotePanel> derefs 到 App；回调里用它更新 Workspace。
            f(cx);
        }
    }
}

impl Render for NotePanel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .id("note-panel")
            .size_full()
            .on_action(cx.listener(Self::save))
            .on_action(cx.listener(Self::on_paste_image))
            .text_size(px(16.))
            .child(self.state.clone())
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
                if let Some(rel) = import_to_assets(path) {
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
        match std::fs::write(&rel, &bytes) {
            Ok(()) => {
                let name = file_stem(&rel);
                let md = format!("\n![{name}]({rel})\n");
                self.state
                    .update(cx, |ed, cx| ed.replace_text_in_range(None, &md, window, cx));
                cx.notify();
            }
            Err(e) => {
                eprintln!("[paste] 写盘失败 {rel}: {e}");
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
fn import_to_assets(src: &std::path::Path) -> Option<String> {
    let dest_dir = std::path::Path::new("_assets");
    std::fs::create_dir_all(dest_dir).ok()?;
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
