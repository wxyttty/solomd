//! SoloMD GPUI 桌面版入口（P2 M1）。
//!
//! 用法：`solomd [vault_dir]`（缺省 `./_vault`，不存在自动创建）。

mod ai_panel;
#[cfg(debug_assertions)]
mod dev_bridge;
mod file_tree;
mod history_panel;
mod info_panel;
mod keys_panel;
mod recipes_panel;
mod search_panel;
mod settings;
mod single_instance;
mod sync_panel;
mod images;
mod mermaid;
mod note_panel;
mod theme;
mod vault;
mod whiteboard;
mod workspace;

use std::path::PathBuf;

use gpui::{
    actions, AppContext as _, KeyBinding, Menu, MenuItem, WindowOptions,
};
use gpui_component::Root;
use gpui_component_assets::Assets;

use gpui_component::dock::ClosePanel;

use crate::note_panel::{NewNote, PasteImage, SaveNote};
use crate::vault::Vault;
use crate::workspace::{ClosePalette, TogglePalette, ToggleTheme, Workspace};

actions!(solomd, [Quit]);
// 编辑菜单：GPUI 编辑器是自绘文本视图，原生剪贴板/撤销 selector 打不到它，
// 菜单项直接派发编辑器动作（经焦点路由）。
use gpui_editor as editor_actions;

fn main() {
    crate::single_instance::ensure_single_instance();

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

    // debug 构建启动原生命令调试桥（release 不编译）。
    #[cfg(debug_assertions)]
    crate::dev_bridge::start(solomd_core::deps::CorePaths::default_paths());

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
            KeyBinding::new("cmd-shift-t", ToggleTheme, None),
            KeyBinding::new("ctrl-shift-t", ToggleTheme, None),
            KeyBinding::new("escape", ClosePalette, None),
            KeyBinding::new("cmd-q", Quit, None),
        ]);
        cx.set_menus(build_menus());

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

/// 原生菜单（macOS）：每项都落到已存在的动作，无装饰性菜单。
fn build_menus() -> Vec<Menu> {
    vec![
        Menu::new("SoloMD").items([MenuItem::action("退出 SoloMD", Quit)]),
        Menu::new("文件").items([
            MenuItem::action("新建笔记", NewNote),
            MenuItem::action("保存", SaveNote),
            MenuItem::separator(),
            MenuItem::action("关闭标签", ClosePanel),
        ]),
        Menu::new("编辑").items([
            MenuItem::action("撤销", editor_actions::Undo),
            MenuItem::action("重做", editor_actions::Redo),
            MenuItem::separator(),
            MenuItem::action("剪切", editor_actions::Cut),
            MenuItem::action("拷贝", editor_actions::Copy),
            MenuItem::action("粘贴", editor_actions::Paste),
            MenuItem::action("全选", editor_actions::SelectAll),
        ]),
        Menu::new("查看").items([
            MenuItem::action("切换笔记面板", TogglePalette),
            MenuItem::action("切换深浅主题", ToggleTheme),
        ]),
    ]
}
