//! ai_panel.rs — AI 聊天最小可用面（M2）：输入 + 流式输出。
//!
//! 完整 AgentPanel（provider 设置 UI、工具调用卡片、Insert 按钮、recipes）
//! 属 P3；本面板验证 solomd-core 的 ai_chat 全链路（key 环 + 流式事件 +
//! 成本计量 + trace 落盘）。provider 硬编码 deepseek（key 存好即用），
//! 设置 UI 进 M2 设置面板。

use std::sync::{Arc, Mutex};

use gpui::{
    div, AppContext as _, Context, Entity, Focusable, FontWeight, InteractiveElement as _,
    IntoElement, ParentElement as _, Render, SharedString, Styled as _, Window,
};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::input::{Input, InputState};
use gpui_component::scroll::ScrollableElement as _;
use gpui_component::{h_flex, v_flex, ActiveTheme as _, Sizable as _};
use solomd_core::ai_proxy::{ai_chat, ChatMessage, ChatRequest};
use solomd_core::deps::{ChannelSink, CorePaths, EventSink};

use crate::vault::Vault;

type EventRx = std::sync::mpsc::Receiver<(String, String)>;

pub struct AiPanel {
    focus_handle: gpui::FocusHandle,
    vault: Vault,
    paths: CorePaths,
    sink: Arc<dyn EventSink>,
    _rx: Arc<Mutex<EventRx>>,
    input: Entity<InputState>,
    /// 流式累积的回复文本（Done 时以 full_text 定稿）。
    reply: String,
    request_id: Option<String>,
    status: SharedString,
}

impl AiPanel {
    pub fn new(vault: Vault, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let (tx, rx) = std::sync::mpsc::channel::<(String, String)>();
        let rx_handle = Arc::new(Mutex::new(rx));
        let sink: Arc<dyn EventSink> = Arc::new(ChannelSink::new(tx));
        let paths = CorePaths::default_paths();
        // prime key 环路径（原 tauri 命令入口的职责，启动一次即可）。
        solomd_core::ai_keystore::prime_config_dir(&paths);

        // 事件泵：core 的 emit → 实体状态更新（与白板 IPC 同款模式）。
        let weak = cx.entity().downgrade();
        let pump_rx = rx_handle.clone();
        cx.spawn(async move |_this, cx| loop {
            cx.background_executor()
                .timer(std::time::Duration::from_millis(80))
                .await;
            let Some(entity) = weak.upgrade() else { break };
            let events: Vec<(String, String)> = match pump_rx.lock() {
                Ok(r) => r.try_iter().collect(),
                Err(_) => break,
            };
            if events.is_empty() {
                continue;
            }
            entity.update(cx, |panel, cx| {
                for (topic, payload) in events {
                    let v: serde_json::Value =
                        serde_json::from_str(&payload).unwrap_or_default();
                    match topic.as_str() {
                        "ai-chunk" => {
                            panel.reply.push_str(v["delta"].as_str().unwrap_or_default());
                            panel.status = "生成中…".into();
                        }
                        "ai-done" => {
                            panel.reply =
                                v["full_text"].as_str().unwrap_or_default().to_string();
                            panel.request_id = None;
                            panel.status = "完成".into();
                        }
                        "ai-error" => {
                            panel.status = format!(
                                "错误：{}",
                                v["error"].as_str().unwrap_or_default()
                            )
                            .into();
                            panel.request_id = None;
                        }
                        _ => {}
                    }
                }
                cx.notify();
            });
        })
        .detach();

        Self {
            focus_handle: cx.focus_handle(),
            vault,
            paths,
            sink,
            _rx: rx_handle,
            input: cx.new(|cx| InputState::new(window, cx).placeholder("向 AI 提问…")),
            reply: String::new(),
            request_id: None,
            status: "就绪（provider: deepseek）".into(),
        }
    }

    fn send(&mut self, _: &gpui::ClickEvent, window: &mut Window, cx: &mut Context<Self>) {
        if self.request_id.is_some() {
            return; // 生成中
        }
        let prompt = self.input.read(cx).value().to_string();
        if prompt.trim().is_empty() {
            return;
        }
        let request_id = format!("req-{}", now_ms());
        self.request_id = Some(request_id.clone());
        self.reply.clear();
        self.status = "请求中…".into();

        let req = ChatRequest {
            provider: "deepseek".into(),
            model: "deepseek-chat".into(),
            messages: vec![ChatMessage {
                role: "user".into(),
                content: prompt,
                tool_call_id: None,
            }],
            request_id: Some(request_id),
            workspace: Some(self.vault.root.to_string_lossy().to_string()),
            api_format: None,
            base_url: None,
            tools: None,
            allow_write: None,
            run_id: None,
            tool_loop_cap: None,
        };

        let sink = self.sink.clone();
        let paths = self.paths.clone();
        cx.spawn_in(window, async move |this, cx| {
            if let Err(e) = ai_chat(sink, &paths, req).await {
                let _ = this.update(cx, |panel, cx| {
                    panel.status = format!("错误：{e}").into();
                    panel.request_id = None;
                    cx.notify();
                });
            }
        })
        .detach();
    }
}

fn now_ms() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

impl Render for AiPanel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .size_full()
            .child(
                h_flex()
                    .px_3()
                    .py_1p5()
                    .gap_2()
                    .border_b_1()
                    .border_color(cx.theme().border)
                    .child(div().font_weight(FontWeight::SEMIBOLD).child("AI 助手"))
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(self.status.clone()),
                    ),
            )
            .child(
                div()
                    .id("ai-scroll")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scrollbar()
                    .p_3()
                    .text_sm()
                    .child(if self.reply.is_empty() {
                        div()
                            .text_color(cx.theme().muted_foreground)
                            .child("回复将显示在这里（key 存于系统钥匙串，首次使用需导入）")
                            .into_any_element()
                    } else {
                        div().child(self.reply.clone()).into_any_element()
                    }),
            )
            .child(
                h_flex()
                    .p_2()
                    .gap_2()
                    .border_t_1()
                    .border_color(cx.theme().border)
                    .child(Input::new(&self.input).flex_1().small())
                    .child(
                        Button::new("ai-send")
                            .label(if self.request_id.is_some() { "…" } else { "发送" })
                            .xsmall()
                            .primary()
                            .on_click(cx.listener(Self::send)),
                    ),
            )
    }
}

impl Focusable for AiPanel {
    fn focus_handle(&self, _: &gpui::App) -> gpui::FocusHandle {
        self.focus_handle.clone()
    }
}
