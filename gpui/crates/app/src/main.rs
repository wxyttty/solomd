//! SoloMD GPUI 桌面版入口（P2 M1）。
//!
//! 用法：`solomd [vault_dir]`（缺省 `./_vault`，不存在自动创建）。

mod ai_panel;
mod file_tree;
mod keys_panel;
mod search_panel;
mod settings;
mod sync_panel;
mod images;
mod mermaid;
mod note_panel;
mod theme;
mod vault;
mod workspace;

use std::path::PathBuf;

use gpui::{actions, AppContext as _, KeyBinding, WindowOptions};
use gpui_component::Root;
use gpui_component_assets::Assets;

use gpui_component::dock::ClosePanel;

use crate::note_panel::{NewNote, PasteImage, SaveNote};
use crate::vault::Vault;
use crate::workspace::{ClosePalette, TogglePalette, Workspace};

actions!(solomd, [Quit]);

fn main() {
    let vault_path = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("_vault"));
    let vault = match Vault::open(vault_path) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("打开笔记库失败: {e}");
            std::process::exit(1);
        }
    };
    let settings = crate::settings::Settings::load(&vault.root);

    let app = gpui_platform::application().with_assets(Assets);

    let dark = settings.dark;
    app.run(move |cx| {
        gpui_component::init(cx);
        // 启动即恢复上次主题（init 默认 Light）。
        if dark {
            gpui_component::theme::Theme::change(
                gpui_component::theme::ThemeMode::Dark,
                None,
                cx,
            );
        }
        gpui_editor::bind_keys(cx);
        cx.bind_keys([
            KeyBinding::new("cmd-s", SaveNote, None),
            KeyBinding::new("ctrl-s", SaveNote, None),
            KeyBinding::new("cmd-n", NewNote, None),
            KeyBinding::new("ctrl-n", NewNote, None),
            // 图片粘贴：绑真实粘贴键（Editor context），无图时 propagate 给文本粘贴。
            KeyBinding::new("cmd-v", PasteImage, Some("Editor")),
            KeyBinding::new("ctrl-v", PasteImage, Some("Editor")),
            KeyBinding::new("cmd-w", ClosePanel, None),
            KeyBinding::new("ctrl-w", ClosePanel, None),
            KeyBinding::new("cmd-p", TogglePalette, None),
            KeyBinding::new("ctrl-p", TogglePalette, None),
            KeyBinding::new("escape", ClosePalette, None),
            KeyBinding::new("cmd-q", Quit, None),
        ]);

        cx.on_action(|_: &Quit, cx: &mut gpui::App| cx.quit());

        let vault_for_window = vault.clone();
        cx.spawn(async move |cx| {
            cx.open_window(WindowOptions::default(), move |window, cx| {
                let view = cx.new(|cx| Workspace::new(vault_for_window, window, cx));
                // 不烘焙 bg：Root::render 每帧按 cx.theme() 刷背景/文字色，
                // 传入的 refine_style 会覆盖主题导致切换后残留旧色。
                cx.new(|cx| Root::new(view, window, cx))
            })
            .expect("open window");
        })
        .detach();
    });
}
