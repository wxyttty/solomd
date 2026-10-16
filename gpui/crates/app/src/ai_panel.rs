//! ai_panel.rs — AI 助手右栏页：多轮聊天 + provider/model 设置 + 复制/插入。
//!
//! provider/model/base_url 持久化在 `<vault>/.solomd/settings.json`
//! （crate::settings）；API key 走 Keys 页（系统钥匙串，与 provider 对应）。
//! tokio 依赖的 core 调用（ai_verify_key 等）经 `solomd_core::rt::spawn`
//! 桥接到进程级 runtime，JoinHandle 在 GPUI executor 上 await。

use std::sync::{Arc, Mutex};

use gpui::{
    div, prelude::FluentBuilder as _, AppContext as _, Context, Entity, EntityInputHandler as _, Focusable, FontWeight, InteractiveElement as _, IntoElement, ParentElement as _,
    Render, SharedString, StatefulInteractiveElement as _, Styled as _, WeakEntity, Window,
};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::input::{Input, InputState};
use gpui_component::scroll::ScrollableElement as _;
use gpui_component::{h_flex, v_flex, ActiveTheme as _, Sizable as _};
use solomd_core::ai_proxy::{ai_chat, ai_verify_key, ChatMessage, ChatRequest};
use solomd_core::deps::{ChannelSink, CorePaths, EventSink};

use crate::note_panel::NotePanel;
use crate::vault::Vault;

type EventRx = std::sync::mpsc::Receiver<(String, String)>;

/// provider 芯片清单（id 沿用 ai_proxy::resolve_provider 的词表）。
const PROVIDERS: &[&str] = &[
    "deepseek", "openai", "anthropic", "gemini", "qwen", "glm", "kimi", "openrouter", "ollama",
    "openai-compat",
];

/// 对话块的形态：普通文本，或一次工具调用卡。
#[derive(PartialEq, Clone, Copy)]
enum TurnKind {
    Text,
    Tool,
}

/// 一个对话块（渲染 + 持久到请求历史）。role: "user" | "assistant" | "tool"。
struct Turn {
    role: String,
    kind: TurnKind,
    text: String,
    /// 工具卡专用字段（kind == Tool 时有效）。
    tool_call_id: Option<String>,
    tool_args: Option<String>,
    tool_result: Option<String>,
    tool_error: Option<String>,
    tool_pending: bool,
    tool_open: bool,
}

impl Turn {
    fn text(role: &str, text: String) -> Self {
        Self {
            role: role.into(),
            kind: TurnKind::Text,
            text,
            tool_call_id: None,
            tool_args: None,
            tool_result: None,
            tool_error: None,
            tool_pending: false,
            tool_open: false,
        }
    }

    fn tool_pending(tool_call_id: String, tool: String, args_json: String) -> Self {
        Self {
            role: "tool".into(),
            kind: TurnKind::Tool,
            text: tool,
            tool_call_id: Some(tool_call_id),
            tool_args: Some(args_json),
            tool_result: None,
            tool_error: None,
            tool_pending: true,
            tool_open: false,
        }
    }
}

pub struct AiPanel {
    focus_handle: gpui::FocusHandle,
    vault: Vault,
    paths: CorePaths,
    sink: Arc<dyn EventSink>,
    _rx: Arc<Mutex<EventRx>>,
    input: Entity<InputState>,
    model_input: Entity<InputState>,
    base_url_input: Entity<InputState>,
    /// 完整对话（发送请求时展开为 messages）。
    turns: Vec<Turn>,
    /// 已完成的 API 消息历史（不含正在生成的）。
    history: Vec<ChatMessage>,
    /// 生成中的那轮用户消息（Done 时随回复一起落入 history）。
    pending_user: Option<ChatMessage>,
    /// 本轮请求是否出现过工具调用（决定 done 时是否以 full_text 覆盖末块）。
    saw_tools: bool,
    request_id: Option<String>,
    status: SharedString,
    /// 生效配置（发送时使用）。
    provider: String,
    model: String,
    base_url: String,
    /// 设置区折叠态。
    config_open: bool,
    /// 附带当前笔记内容作为上下文。
    include_note: bool,
    /// Workspace 推送的当前活跃笔记（读盘取内容）。
    active_note: Option<std::path::PathBuf>,
    /// Workspace 推送的活跃笔记面板（「插入」落点）。
    insert_target: Option<WeakEntity<NotePanel>>,
    /// Agent 允许写工具（上游 agentAllowWrite；默认关——agent 只读）。
    allow_write: bool,
}

impl AiPanel {
    pub fn new(vault: Vault, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let (tx, rx) = std::sync::mpsc::channel::<(String, String)>();
        let rx_handle = Arc::new(Mutex::new(rx));
        let sink: Arc<dyn EventSink> = Arc::new(ChannelSink::new(tx));
        let paths = CorePaths::default_paths();
        // prime key 环路径（原 tauri 命令入口的职责，启动一次即可）。
        solomd_core::ai_keystore::prime_config_dir(&paths);

        // 恢复持久化 AI 配置。
        let settings = crate::settings::Settings::load(&vault.root);
        let provider = settings.ai_provider_or_default();
        let model = settings.ai_model_or_default();
        let base_url = settings.ai_base_url.clone();
        let allow_write = settings.agent_allow_write;

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
                            let delta = v["delta"].as_str().unwrap_or_default().to_string();
                            // 续写最后一块 assistant 文本；工具卡之后的新文本开新块。
                            match panel.turns.last_mut() {
                                Some(t) if t.kind == TurnKind::Text && t.role == "assistant" => {
                                    t.text.push_str(&delta);
                                }
                                _ => {
                                    panel.turns.push(Turn::text("assistant", delta));
                                }
                            }
                            panel.status = "生成中…".into();
                        }
                        "ai-tool-call" => {
                            // C3.2 step 1：工具调用卡（pending 态）。
                            let card = Turn::tool_pending(
                                v["tool_call_id"].as_str().unwrap_or_default().to_string(),
                                v["tool"].as_str().unwrap_or_default().to_string(),
                                pretty_json(&v["args"]),
                            );
                            panel.turns.push(card);
                            panel.saw_tools = true;
                            panel.status = "工具调用中…".into();
                        }
                        "ai-tool-result" => {
                            // C3.2 step 3：回填对应的工具卡。
                            let id = v["tool_call_id"].as_str().unwrap_or_default();
                            let result = v["result"].as_str().map(|s| s.to_string()).unwrap_or_else(|| pretty_json(&v["result"]));
                            let error = v["error"].as_str().map(|s| s.to_string());
                            for t in panel.turns.iter_mut().rev() {
                                if t.kind == TurnKind::Tool
                                    && t.tool_call_id.as_deref() == Some(id)
                                {
                                    t.tool_pending = false;
                                    t.tool_result = Some(result);
                                    t.tool_error = error;
                                    break;
                                }
                            }
                            panel.status = "生成中…".into();
                        }
                        "ai-done" => {
                            let full = v["full_text"].as_str().unwrap_or_default().to_string();
                            // 无工具卡的普通请求：以 full_text 定稿，容忍分段丢失。
                            if panel.saw_tools {
                                if let Some(last) = panel.turns.last_mut() {
                                    if last.kind == TurnKind::Text && last.text.is_empty() {
                                        last.text = full.clone();
                                    }
                                }
                            } else if let Some(last) = panel.turns.last_mut() {
                                if last.kind == TurnKind::Text {
                                    last.text = full.clone();
                                }
                            }
                            if let Some(user) = panel.pending_user.take() {
                                panel.history.push(user);
                            }
                            panel.history.push(ChatMessage {
                                role: "assistant".into(),
                                content: full,
                                tool_call_id: None,
                            });
                            panel.request_id = None;
                            panel.status = "完成".into();
                        }
                        "ai-error" => {
                            // 占位的空 assistant 块收掉，错误进状态行。
                            if panel
                                .turns
                                .last()
                                .map(|t| t.kind == TurnKind::Text && t.role == "assistant" && t.text.is_empty())
                                .unwrap_or(false)
                            {
                                panel.turns.pop();
                            }
                            panel.request_id = None;
                            panel.status = format!(
                                "错误：{}",
                                v["error"].as_str().unwrap_or_default()
                            )
                            .into();
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
            model_input: cx.new(|cx| InputState::new(window, cx).placeholder("模型名")),
            base_url_input: cx
                .new(|cx| InputState::new(window, cx).placeholder("自定义地址（留空 = 默认）")),
            turns: Vec::new(),
            history: Vec::new(),
            pending_user: None,
            saw_tools: false,
            request_id: None,
            status: SharedString::from(format!("就绪 · {provider}")),
            provider,
            model,
            base_url,
            config_open: false,
            include_note: true,
            active_note: None,
            insert_target: None,
            allow_write,
        }
    }

    /// Workspace 在笔记打开/激活时推送。
    pub fn set_active_note(&mut self, path: Option<std::path::PathBuf>) {
        self.active_note = path;
    }

    /// Workspace 在笔记打开/激活时推送（「插入」按钮的落点）。
    pub fn set_insert_target(&mut self, target: Option<WeakEntity<NotePanel>>) {
        self.insert_target = target;
    }

    fn save_config(&mut self, _: &gpui::ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.model = self.model_input.read(cx).value().to_string();
        self.base_url = self.base_url_input.read(cx).value().to_string();

        let mut settings = crate::settings::Settings::load(&self.vault.root);
        settings.ai_provider = self.provider.clone();
        settings.ai_model = self.model.clone();
        settings.ai_base_url = self.base_url.clone();
        settings.agent_allow_write = self.allow_write;
        settings.save(&self.vault.root);
        self.status = SharedString::from(format!("已保存 · {}", self.provider));
        cx.notify();
    }

    fn verify_config(&mut self, _: &gpui::ClickEvent, window: &mut Window, cx: &mut Context<Self>) {
        let provider = self.provider.clone();
        let base = {
            let v = self.base_url_input.read(cx).value().to_string();
            (!v.trim().is_empty()).then_some(v)
        };
        // 无 /models 的端点用 chat ping 兜底，需要模型名（#261）。
        let model = {
            let v = self.model_input.read(cx).value().to_string();
            (!v.trim().is_empty()).then_some(v)
        };
        self.status = "验证中…".into();
        cx.notify();
        cx.spawn_in(window, async move |this, cx| {
            // JoinHandle 可在 GPUI executor 上 await；reqwest 在进程级 runtime 内跑。
            let jh = solomd_core::rt::spawn(async move {
                ai_verify_key(provider, None, None, base, model).await
            });
            let r = jh.await.unwrap_or_else(|e| Err(format!("join: {e}")));
            let _ = this.update_in(cx, |panel, _, cx| {
                panel.status = match r {
                    Ok(msg) => SharedString::from(format!("连接正常：{msg}")),
                    Err(e) => SharedString::from(format!("验证失败：{e}")),
                };
                cx.notify();
            });
        })
        .detach();
    }

    fn stop(&mut self, _: &gpui::ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(id) = self.request_id.take() {
            let _ = solomd_core::ai_proxy::ai_cancel(id);
            self.status = "已停止".into();
            cx.notify();
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
        self.status = "请求中…".into();

        // messages = [系统上下文?] + 历史 + 本轮输入。
        let mut messages: Vec<ChatMessage> = Vec::new();
        if self.include_note {
            if let Some(path) = self.active_note.clone() {
                if let Ok(text) = self.vault.read_note(&path) {
                    if !text.trim().is_empty() {
                        messages.push(ChatMessage {
                            role: "system".into(),
                            content: format!(
                                "用户正在编辑笔记 {}，内容如下：\n\n{}",
                                path.display(),
                                text
                            ),
                            tool_call_id: None,
                        });
                    }
                }
            }
        }
        messages.extend(self.history.iter().cloned());
        messages.push(ChatMessage {
            role: "user".into(),
            content: prompt.clone(),
            tool_call_id: None,
        });

        // UI 流水：用户块 + assistant 占位块。
        self.turns.push(Turn::text("user", prompt.clone()));
        self.turns.push(Turn::text("assistant", String::new()));
        self.saw_tools = false;
        self.pending_user = Some(ChatMessage {
            role: "user".into(),
            content: prompt,
            tool_call_id: None,
        });
        self.input.update(cx, |i, cx| i.set_value("", window, cx));

        let req = ChatRequest {
            provider: self.provider.clone(),
            model: self.model.clone(),
            messages,
            request_id: Some(request_id),
            workspace: Some(self.vault.root.to_string_lossy().to_string()),
            api_format: None,
            base_url: (!self.base_url.trim().is_empty()).then(|| self.base_url.clone()),
            tools: None,
            allow_write: Some(self.allow_write),
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

    fn copy_turn(&mut self, text: &str, cx: &mut Context<Self>) {
        cx.write_to_clipboard(gpui::ClipboardItem::new_string(text.to_string()));
        self.status = "已复制".into();
        cx.notify();
    }

    fn insert_turn(&mut self, text: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(target) = self.insert_target.clone() else {
            self.status = "没有打开的笔记可插入".into();
            cx.notify();
            return;
        };
        let md = format!("\n{text}\n");
        let _ = target.update_in(cx, |np, window, cx| {
            np.state
                .update(cx, |ed, cx| ed.replace_text_in_range(None, &md, window, cx));
            cx.notify();
        });
        self.status = "已插入当前笔记".into();
        let _ = window;
        cx.notify();
    }
}

fn now_ms() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

/// 工具卡展示用：紧凑 pretty-print，超长截断。
fn pretty_json(v: &serde_json::Value) -> String {
    let s = serde_json::to_string_pretty(v).unwrap_or_else(|_| v.to_string());
    s.chars().take(600).collect()
}

impl AiPanel {
    fn render_config(&mut self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let mut chips = h_flex().flex_wrap().gap_1();
        for p in PROVIDERS {
            let id = p.to_string();
            let selected = *p == self.provider;
            chips = chips.child(
                Button::new(SharedString::from(format!("prov-{p}")))
                    .label(*p)
                    .xsmall()
                    .when(selected, |b| b.primary())
                    .when(!selected, |b| b.ghost())
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.provider = id.clone();
                        cx.notify();
                    })),
            );
        }
        v_flex()
            .p_2()
            .gap_2()
            .border_b_1()
            .border_color(cx.theme().border)
            .bg(cx.theme().muted.opacity(0.3))
            .child(
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child("Provider"),
            )
            .child(chips)
            .child(Input::new(&self.model_input).small())
            .child(Input::new(&self.base_url_input).small())
            // Agent 写工具开关（上游 agentAllowWrite；默认关——agent 只读）。
            // 点击即持久化（上游 checkbox 同款即时生效，不等“保存”按钮）。
            .child(
                Button::new("ai-allow-write")
                    .label(if self.allow_write {
                        "写工具 ✓（agent 可写笔记）"
                    } else {
                        "写工具（agent 只读）"
                    })
                    .xsmall()
                    .ghost()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.allow_write = !this.allow_write;
                        let mut s = crate::settings::Settings::load(&this.vault.root);
                        s.agent_allow_write = this.allow_write;
                        s.save(&this.vault.root);
                        cx.notify();
                    })),
            )
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        Button::new("ai-save")
                            .label("保存")
                            .xsmall()
                            .primary()
                            .on_click(cx.listener(Self::save_config)),
                    )
                    .child(
                        Button::new("ai-verify")
                            .label("测试连接")
                            .xsmall()
                            .ghost()
                            .on_click(cx.listener(Self::verify_config)),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child("API key 在 Keys 页管理"),
                    ),
            )
            .into_any_element()
    }

    fn render_turns(&mut self, cx: &mut Context<Self>) -> gpui::AnyElement {
        if self.turns.is_empty() {
            return div()
                .text_sm()
                .text_color(cx.theme().muted_foreground)
                .child("回复将显示在这里。多轮对话保留上下文；「附带笔记」把当前笔记内容作为背景。")
                .into_any_element();
        }
        let mut list = v_flex().gap_2();
        for (ix, turn) in self.turns.iter().enumerate() {
            let is_user = turn.role == "user";
            if turn.kind == TurnKind::Tool {
                // 工具调用卡：折叠态一行（名称+状态），点击展开 args/result。
                let state = if turn.tool_pending {
                    "…"
                } else if turn.tool_error.is_some() {
                    "⚠"
                } else {
                    "🔧"
                };
                let open = turn.tool_open;
                let args = turn.tool_args.clone().unwrap_or_default();
                let result = turn.tool_result.clone().unwrap_or_default();
                let error = turn.tool_error.clone();
                let mut card = v_flex()
                    .rounded_md()
                    .border_1()
                    .border_color(cx.theme().border)
                    .child(
                        div()
                            .id(SharedString::from(format!("tool-{ix}")))
                            .px_2()
                            .py_1p5()
                            .flex()
                            .items_center()
                            .gap_2()
                            .cursor_pointer()
                            .hover(|s| s.bg(cx.theme().accent.opacity(0.12)))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                if let Some(t) = this.turns.get_mut(ix) {
                                    t.tool_open = !t.tool_open;
                                }
                                cx.notify();
                            }))
                            .child(
                                div()
                                    .text_xs()
                                    .font_family("Menlo")
                                    .child(format!("{state} {}", turn.text)),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(if open { "收起" } else { "详情" }),
                            ),
                    );
                if open {
                    card = card
                        .child(
                            div()
                                .px_2()
                                .pb_1()
                                .text_xs()
                                .font_family("Menlo")
                                .text_color(cx.theme().muted_foreground)
                                .child(format!("参数：{args}")),
                        )
                        .child(if let Some(err) = &error {
                            div()
                                .px_2()
                                .pb_1p5()
                                .text_xs()
                                .font_family("Menlo")
                                .text_color(cx.theme().danger)
                                .child(format!("错误：{err}"))
                                .into_any_element()
                        } else {
                            div()
                                .px_2()
                                .pb_1p5()
                                .text_xs()
                                .font_family("Menlo")
                                .text_color(cx.theme().muted_foreground)
                                .child(format!(
                                    "结果：{}",
                                    result.chars().take(400).collect::<String>()
                                ))
                                .into_any_element()
                        });
                }
                list = list.child(card);
                continue;
            }
            let mut block = v_flex()
                .gap_1()
                .child(
                    div()
                        .text_xs()
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(cx.theme().muted_foreground)
                        .child(if is_user { "你" } else { "AI" }),
                )
                .child(
                    div()
                        .text_sm()
                        .child(turn.text.clone()),
                );
            if !is_user && !turn.text.is_empty() {
                let copy_text = turn.text.clone();
                let insert_text = turn.text.clone();
                block = block.child(
                    h_flex()
                        .gap_1()
                        .child(
                            Button::new(SharedString::from(format!("copy-{ix}")))
                                .label("复制")
                                .xsmall()
                                .ghost()
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.copy_turn(&copy_text, cx);
                                })),
                        )
                        .child(
                            Button::new(SharedString::from(format!("insert-{ix}")))
                                .label("插入笔记")
                                .xsmall()
                                .ghost()
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    this.insert_turn(&insert_text, window, cx);
                                })),
                        ),
                );
            }
            list = list.child(block);
        }
        list.into_any_element()
    }
}

impl Render for AiPanel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let generating = self.request_id.is_some();
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
                            .flex_1()
                            .truncate()
                            .text_color(cx.theme().muted_foreground)
                            .child(self.status.clone()),
                    )
                    .child(
                        Button::new("ai-config")
                            .label(if self.config_open { "收起" } else { "设置" })
                            .xsmall()
                            .ghost()
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.config_open = !this.config_open;
                                cx.notify();
                            })),
                    ),
            )
            .when(self.config_open, |this| this.child(self.render_config(cx)))
            .child(
                h_flex()
                    .px_3()
                    .py_1()
                    .gap_2()
                    .child(
                        Button::new("ai-include-note")
                            .label(if self.include_note { "✓ 附带当前笔记" } else { "附带当前笔记" })
                            .xsmall()
                            .when(self.include_note, |b| b.primary())
                            .when(!self.include_note, |b| b.ghost())
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.include_note = !this.include_note;
                                cx.notify();
                            })),
                    ),
            )
            .child(
                div()
                    .id("ai-scroll")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scrollbar()
                    .p_3()
                    .child(self.render_turns(cx)),
            )
            .child(
                h_flex()
                    .p_2()
                    .gap_2()
                    .border_t_1()
                    .border_color(cx.theme().border)
                    .child(Input::new(&self.input).flex_1().small())
                    .when(generating, |this| {
                        this.child(
                            Button::new("ai-stop")
                                .label("停止")
                                .xsmall()
                                .ghost()
                                .on_click(cx.listener(Self::stop)),
                        )
                    })
                    .child(
                        Button::new("ai-send")
                            .label(if generating { "…" } else { "发送" })
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
