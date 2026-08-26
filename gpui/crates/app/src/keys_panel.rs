//! keys_panel.rs — AI key 管理（M2 收尾）：14 家 provider 的 key 导入/清除。
//!
//! 存储沿用原版约定：系统钥匙串 service="solomd"、user="ai-{provider}"，
//! 与 Tauri 版互通（同机升级零迁移）。

use gpui::prelude::FluentBuilder as _;
use gpui::{
    div, AppContext as _, Context, Entity, Focusable, FocusHandle, FontWeight, IntoElement,
    ParentElement as _, Render, SharedString, Styled as _, Window,
};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::input::{Input, InputState};
use gpui_component::{h_flex, v_flex, ActiveTheme as _, Sizable as _};
use solomd_core::ai_keystore;

/// (id, 展示名)——与 ai_proxy 支持的 provider 对应。
const PROVIDERS: &[(&str, &str)] = &[
    ("openai", "OpenAI"),
    ("anthropic", "Claude"),
    ("deepseek", "DeepSeek"),
    ("gemini", "Gemini"),
    ("qwen", "通义千问"),
    ("glm", "智谱 GLM"),
    ("kimi", "Kimi"),
    ("doubao", "豆包"),
    ("siliconflow", "SiliconFlow"),
    ("openrouter", "OpenRouter"),
    ("mistral", "Mistral"),
    ("groq", "Groq"),
    ("xai", "xAI"),
];

pub struct KeysPanel {
    focus_handle: FocusHandle,
    selected: usize,
    key_input: Entity<InputState>,
}

impl KeysPanel {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        Self {
            focus_handle: cx.focus_handle(),
            selected: 2, // 默认 DeepSeek
            key_input: cx.new(|cx| {
                InputState::new(window, cx).placeholder("粘贴 API Key（存入系统钥匙串）")
            }),
        }
    }

    fn provider_id(&self) -> &'static str {
        PROVIDERS[self.selected].0
    }

    fn save(&mut self, _: &gpui::ClickEvent, window: &mut Window, cx: &mut Context<Self>) {
        let key = self.key_input.read(cx).value().to_string();
        if key.trim().is_empty() {
            return;
        }
        match ai_keystore::set_key(self.provider_id(), key.trim()) {
            Ok(()) => {
                self.key_input.update(cx, |q, cx| q.set_value("", window, cx));
                cx.notify();
            }
            Err(e) => eprintln!("[keys] set failed: {e}"),
        }
    }

    fn clear(&mut self, _: &gpui::ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        let _ = ai_keystore::clear_key(self.provider_id());
        cx.notify();
    }
}

impl Render for KeysPanel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // provider 横向选择 chips + key 输入行 + has_key 状态点。
        let mut chips = h_flex().flex_wrap().gap_1();
        for (ix, (id, display)) in PROVIDERS.iter().enumerate() {
            let selected = ix == self.selected;
            let has_key = ai_keystore::has_key(id).unwrap_or(false);
            let dot = if has_key { "●" } else { "○" };
            chips = chips.child(
                Button::new(SharedString::from(format!("prov-{id}")))
                    .label(format!("{dot} {display}"))
                    .xsmall()
                    .when(selected, |b| b.primary())
                    .when(!selected, |b| b.ghost())
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.selected = ix;
                        cx.notify();
                    })),
            );
        }

        v_flex()
            .size_full()
            .child(
                div()
                    .px_3()
                    .py_1p5()
                    .border_b_1()
                    .border_color(cx.theme().border)
                    .font_weight(FontWeight::SEMIBOLD)
                    .child("AI Key 管理"),
            )
            .child(div().p_3().flex().flex_col().gap_3().child(chips).child(
                h_flex()
                    .gap_2()
                    .child(Input::new(&self.key_input).flex_1().small())
                    .child(
                        Button::new("key-save")
                            .label("保存")
                            .xsmall()
                            .primary()
                            .on_click(cx.listener(Self::save)),
                    )
                    .child(
                        Button::new("key-clear")
                            .label("清除")
                            .xsmall()
                            .ghost()
                            .on_click(cx.listener(Self::clear)),
                    ),
            ))
    }
}

impl Focusable for KeysPanel {
    fn focus_handle(&self, _: &gpui::App) -> gpui::FocusHandle {
        self.focus_handle.clone()
    }
}
