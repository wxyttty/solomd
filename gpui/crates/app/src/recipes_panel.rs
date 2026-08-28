//! recipes_panel.rs — recipes 右栏页：列表 + 立即运行 + 运行状态 + 可视化编辑。
//!
//! 编辑 = 表单驱动 `recipes::save_recipe`（core 侧 recipe_to_yaml 生成
//! canonical YAML；新建写 `<vault>/.solomd/agents/<slug>.yml`，编辑回写源文件）。

use std::sync::Arc;

use gpui::{
    div, px, prelude::FluentBuilder as _, AppContext as _, Context, Entity, FocusHandle,
    Focusable, FontWeight, InteractiveElement as _, IntoElement, ParentElement as _, Render,
    SharedString, StatefulInteractiveElement as _, Styled as _, Window,
};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::input::{Input, InputState, Textarea, TextareaState};
use gpui_component::{h_flex, v_flex, ActiveTheme as _, Sizable as _};
use solomd_core::deps::{ChannelSink, EventSink};
use solomd_core::recipes::{Recipe, TriggerKind};
use solomd_core::recipe_runner;

use crate::vault::Vault;

const TRIGGERS: &[(&str, TriggerKind)] = &[
    ("手动", TriggerKind::Manual),
    ("保存时", TriggerKind::OnSave),
    ("提交时", TriggerKind::OnCommit),
    ("定时", TriggerKind::Schedule),
    ("打标签", TriggerKind::OnTagAdd),
];

pub struct RecipesPanel {
    focus_handle: FocusHandle,
    vault: Vault,
    recipes: Vec<Recipe>,
    running: Option<String>,
    status: SharedString,
    sink: Arc<dyn EventSink>,
    /// 编辑态：None = 列表；Some = 表单（index None = 新建）。
    edit: Option<EditForm>,
}

struct EditForm {
    /// 编辑中的 recipe 在列表里的下标（None = 新建）。
    index: Option<usize>,
    trigger: TriggerKind,
    allow_write: bool,
    name: Entity<InputState>,
    schedule: Entity<InputState>,
    match_glob: Entity<InputState>,
    tag: Entity<InputState>,
    prompt: Entity<TextareaState>,
    provider: Entity<InputState>,
    model: Entity<InputState>,
    base_url: Entity<InputState>,
    tools: Entity<InputState>,
    write_cap: Entity<InputState>,
}

fn make_input(
    window: &mut Window,
    placeholder: &str,
    value: &str,
    cx: &mut Context<RecipesPanel>,
) -> Entity<InputState> {
    cx.new(|cx| {
        let mut s = InputState::new(window, cx).placeholder(placeholder);
        if !value.is_empty() {
            s = s.default_value(value);
        }
        s
    })
}

fn make_textarea(
    window: &mut Window,
    placeholder: &str,
    value: &str,
    cx: &mut Context<RecipesPanel>,
) -> Entity<TextareaState> {
    cx.new(|cx| {
        let mut s = TextareaState::new(window, cx)
            .auto_grow(4, 16)
            .placeholder(placeholder);
        if !value.is_empty() {
            s = s.default_value(value);
        }
        s
    })
}

impl RecipesPanel {
    pub fn new(vault: Vault, _window: &mut Window, cx: &mut Context<Self>) -> Self {
        let (tx, rx) = std::sync::mpsc::channel::<(String, String)>();
        // 事件泵：recipes-run-finished → 状态刷新
        let weak = cx.entity().downgrade();
        let pump_rx = rx;
        cx.spawn(async move |_this, cx| loop {
            cx.background_executor()
                .timer(std::time::Duration::from_millis(200))
                .await;
            let Some(entity) = weak.upgrade() else { break };
            let mut finished = false;
            if let Ok((topic, _)) = pump_rx.try_recv() {
                if topic == "recipes-run-finished" {
                    finished = true;
                }
            }
            if finished {
                let _ = entity.update(cx, |panel, cx| {
                    panel.status = "运行完成".into();
                    panel.refresh_list(cx);
                    cx.notify();
                });
            }
        })
        .detach();

        let sink: Arc<dyn EventSink> = Arc::new(ChannelSink::new(tx));
        let mut this = Self {
            focus_handle: cx.focus_handle(),
            vault,
            recipes: Vec::new(),
            running: None,
            status: "就绪".into(),
            sink,
            edit: None,
        };
        this.refresh_list(cx);
        this
    }

    fn refresh_list(&mut self, cx: &mut Context<Self>) {
        let ws = self.vault.root.clone();
        let (recipes, errors) = solomd_core::recipes::load_recipes(&ws);
        self.recipes = recipes;
        if !errors.is_empty() {
            self.status = SharedString::from(format!("{} 个 recipe 解析失败", errors.len()));
        }
        let _ = cx;
    }

    fn run_now(&mut self, slug: String, window: &mut Window, cx: &mut Context<Self>) {
        if self.running.is_some() {
            return;
        }
        self.running = Some(slug.clone());
        self.status = SharedString::from(format!("运行中：{slug}"));
        cx.notify();

        let sink = self.sink.clone();
        let ws = self.vault.root.to_string_lossy().to_string();
        cx.spawn_in(window, async move |this, cx| {
            let r = recipe_runner::recipes_run_now(sink, ws, slug.clone()).await;
            let _ = this.update_in(cx, |panel, window, _cx| {
                panel.running = None;
                match r {
                    Ok(run_id) => {
                        panel.status = SharedString::from(format!("已启动 run {run_id}"));
                    }
                    Err(e) => panel.status = SharedString::from(format!("失败：{e}")),
                }
                window.refresh();
            });
        })
        .detach();
    }

    // ------------------------------------------------------------------
    // 可视化编辑
    // ------------------------------------------------------------------

    fn start_edit(&mut self, index: Option<usize>, window: &mut Window, cx: &mut Context<Self>) {
        let form = match index.map(|i| &self.recipes[i]) {
            Some(r) => EditForm {
                index: Some(index.unwrap()),
                trigger: r.trigger.clone(),
                allow_write: r.allow_write,
                name: make_input(window, "名称", &r.name, cx),
                schedule: make_input(window, "cron 表达式", r.schedule.as_deref().unwrap_or(""), cx),
                match_glob: make_input(window, "匹配 glob", r.match_glob.as_deref().unwrap_or(""), cx),
                tag: make_input(window, "标签", r.tag.as_deref().unwrap_or(""), cx),
                prompt: make_textarea(window, "提示词", &r.prompt, cx),
                provider: make_input(window, "provider（空 = 默认）", &r.provider, cx),
                model: make_input(window, "model（空 = 默认）", &r.model, cx),
                base_url: make_input(window, "base_url（空 = 默认）", r.base_url.as_deref().unwrap_or(""), cx),
                tools: make_input(window, "tools，逗号分隔（空 = 只读工具）", &r.tools.join(", "), cx),
                write_cap: make_input(window, "写上限", &r.write_cap.to_string(), cx),
            },
            None => EditForm {
                index: None,
                trigger: TriggerKind::Manual,
                allow_write: false,
                name: make_input(window, "名称", "", cx),
                schedule: make_input(window, "cron 表达式", "", cx),
                match_glob: make_input(window, "匹配 glob", "", cx),
                tag: make_input(window, "标签", "", cx),
                prompt: make_textarea(window, "提示词", "", cx),
                provider: make_input(window, "provider（空 = 默认）", "", cx),
                model: make_input(window, "model（空 = 默认）", "", cx),
                base_url: make_input(window, "base_url（空 = 默认）", "", cx),
                tools: make_input(window, "tools，逗号分隔（空 = 只读工具）", "", cx),
                write_cap: make_input(window, "写上限", "5", cx),
            },
        };
        self.edit = Some(form);
        cx.notify();
    }

    fn cancel_edit(&mut self, _: &gpui::ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.edit = None;
        self.status = "已取消".into();
        cx.notify();
    }

    fn save_edit(&mut self, _: &gpui::ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        let Some(form) = &self.edit else { return };
        let name = form.name.read(cx).value().trim().to_string();
        if name.is_empty() {
            self.status = "名称不能为空".into();
            cx.notify();
            return;
        }
        let val = |e: &Entity<InputState>| e.read(cx).value().trim().to_string();
        let val_ta = |e: &Entity<TextareaState>| e.read(cx).value().trim().to_string();
        let non_empty = |e: &Entity<InputState>| {
            let v = val(e);
            (!v.is_empty()).then_some(v)
        };
        let tools: Vec<String> = val(&form.tools)
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();

        // 编辑已有 recipe：保留原 slug/文件路径，改了名也回写原文件。
        let original = form.index.map(|i| &self.recipes[i]);
        let trigger = form.trigger.clone();
        let recipe = Recipe {
            name: name.clone(),
            slug: original
                .map(|r| r.slug.clone())
                .unwrap_or_else(|| Recipe::slugify(&name)),
            path: original.and_then(|r| r.path.clone()),
            trigger: trigger.clone(),
            schedule: if trigger == TriggerKind::Schedule {
                non_empty(&form.schedule)
            } else {
                None
            },
            match_glob: non_empty(&form.match_glob),
            tag: non_empty(&form.tag),
            prompt: val_ta(&form.prompt),
            allow_write: form.allow_write,
            write_cap: val(&form.write_cap).parse::<u32>().unwrap_or(5),
            provider: val(&form.provider),
            model: val(&form.model),
            base_url: non_empty(&form.base_url),
            tools,
        };

        match solomd_core::recipes::save_recipe(&self.vault.root, &recipe) {
            Ok(path) => {
                self.status = SharedString::from(format!("已保存 {}", path.display()));
                self.edit = None;
                self.refresh_list(cx);
            }
            Err(e) => self.status = SharedString::from(format!("保存失败：{e}")),
        }
        cx.notify();
    }

    fn delete_recipe_at(&mut self, index: usize, cx: &mut Context<Self>) {
        let slug = self.recipes[index].slug.clone();
        match solomd_core::recipes::delete_recipe(&self.vault.root, &slug) {
            Ok(()) => {
                self.status = SharedString::from(format!("已删除 {slug}"));
                self.refresh_list(cx);
            }
            Err(e) => self.status = SharedString::from(format!("删除失败：{e}")),
        }
        cx.notify();
    }

    // ------------------------------------------------------------------
    // 渲染
    // ------------------------------------------------------------------

    fn render_edit(&mut self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let Some(form) = &self.edit else {
            return div().into_any_element();
        };
        let mut trigger_chips = h_flex().flex_wrap().gap_1();
        for (label, kind) in TRIGGERS {
            let selected = *kind == form.trigger;
            let k = *kind;
            trigger_chips = trigger_chips.child(
                Button::new(SharedString::from(format!("trig-{}", label)))
                    .label(*label)
                    .xsmall()
                    .when(selected, |b| b.primary())
                    .when(!selected, |b| b.ghost())
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if let Some(f) = &mut this.edit {
                            f.trigger = k;
                        }
                        cx.notify();
                    })),
            );
        }
        let allow_write = form.allow_write;
        let is_schedule = form.trigger == TriggerKind::Schedule;
        let show_glob = matches!(form.trigger, TriggerKind::OnSave | TriggerKind::OnCommit);
        let show_tag = form.trigger == TriggerKind::OnTagAdd;

        v_flex()
            .p_2()
            .gap_2()
            .child(
                h_flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_sm()
                            .child(if form.index.is_some() {
                                "编辑 Recipe"
                            } else {
                                "新建 Recipe"
                            }),
                    )
                    .child(
                        Button::new("recipe-cancel")
                            .label("取消")
                            .xsmall()
                            .ghost()
                            .on_click(cx.listener(Self::cancel_edit)),
                    ),
            )
            .child(Input::new(&form.name).small())
            .child(trigger_chips)
            .when(is_schedule, |this| {
                this.child(Input::new(&form.schedule).small())
            })
            .when(show_glob, |this| this.child(Input::new(&form.match_glob).small()))
            .when(show_tag, |this| this.child(Input::new(&form.tag).small()))
            .child(
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child("提示词"),
            )
            .child(Textarea::new(&form.prompt))
            .child(
                h_flex()
                    .gap_2()
                    .child(Input::new(&form.provider).flex_1().small())
                    .child(Input::new(&form.model).flex_1().small()),
            )
            .child(Input::new(&form.base_url).small())
            .child(
                h_flex()
                    .gap_2()
                    .child(Input::new(&form.tools).flex_1().small())
                    .child(
                        Button::new("recipe-allow-write")
                            .label(if allow_write { "✓ 允许写" } else { "只读" })
                            .xsmall()
                            .when(allow_write, |b| b.primary())
                            .when(!allow_write, |b| b.ghost())
                            .on_click(cx.listener(|this, _, _, cx| {
                                if let Some(f) = &mut this.edit {
                                    f.allow_write = !f.allow_write;
                                }
                                cx.notify();
                            })),
                    )
                    .child(Input::new(&form.write_cap).small()),
            )
            .child(
                Button::new("recipe-save")
                    .label("保存")
                    .small()
                    .primary()
                    .on_click(cx.listener(Self::save_edit)),
            )
            .into_any_element()
    }

    fn render_list(&mut self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let mut list = v_flex().gap_1p5();
        if self.recipes.is_empty() {
            list = list.child(
                div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child("vault 里没有 recipes。点「＋ 新建」或在 <vault>/.solomd/agents/ 下放 YAML 文件。"),
            );
        }
        for (ix, r) in self.recipes.iter().enumerate() {
            let slug = r.name.clone();
            let trigger = format!("{:?}", r.trigger);
            let desc = r.prompt.chars().take(60).collect::<String>();
            let running = self.running.as_deref() == Some(r.name.as_str());
            list = list.child(
                v_flex()
                    .px_2()
                    .py_1p5()
                    .rounded_md()
                    .border_1()
                    .border_color(cx.theme().border)
                    .gap_1()
                    .child(
                        h_flex()
                            .items_center()
                            .justify_between()
                            .child(
                                div()
                                    .text_sm()
                                    .font_weight(FontWeight::MEDIUM)
                                    .child(r.name.clone()),
                            )
                            .child(
                                h_flex()
                                    .gap_1()
                                    .child(
                                        Button::new(SharedString::from(format!(
                                            "edit-{}",
                                            r.slug
                                        )))
                                        .label("编辑")
                                        .xsmall()
                                        .ghost()
                                        .on_click(
                                            cx.listener(move |this, _, window, cx| {
                                                this.start_edit(Some(ix), window, cx);
                                            }),
                                        ),
                                    )
                                    .child(
                                        Button::new(SharedString::from(format!(
                                            "del-{}",
                                            r.slug
                                        )))
                                        .label("删除")
                                        .xsmall()
                                        .ghost()
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            this.delete_recipe_at(ix, cx);
                                        })),
                                    )
                                    .child(if running {
                                        Button::new(SharedString::from(format!(
                                            "run-{}",
                                            r.slug
                                        )))
                                        .label("…")
                                        .xsmall()
                                        .ghost()
                                    } else {
                                        Button::new(SharedString::from(format!(
                                            "run-{}",
                                            r.slug
                                        )))
                                        .label("▶ 运行")
                                        .xsmall()
                                        .ghost()
                                        .on_click({
                                            let slug = slug.clone();
                                            cx.listener(move |this, _, window, cx| {
                                                this.run_now(slug.clone(), window, cx);
                                            })
                                        })
                                    }),
                            ),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(format!(
                                "触发：{trigger} · 写:{}{}",
                                if r.allow_write { "允许" } else { "只读" },
                                match &r.schedule {
                                    Some(s) => format!(" · {s}"),
                                    None => String::new(),
                                }
                            )),
                    )
                    .when(!desc.is_empty(), |this| {
                        this.child(
                            div()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child(desc),
                        )
                    }),
            );
        }
        list.into_any_element()
    }
}

impl Render for RecipesPanel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let body = if self.edit.is_some() {
            self.render_edit(cx)
        } else {
            self.render_list(cx)
        };

        v_flex()
            .size_full()
            .child(
                h_flex()
                    .px_3()
                    .py_1p5()
                    .gap_2()
                    .border_b_1()
                    .border_color(cx.theme().border)
                    .child(div().font_weight(FontWeight::SEMIBOLD).child("Recipes"))
                    .child(
                        div()
                            .text_xs()
                            .flex_1()
                            .truncate()
                            .text_color(cx.theme().muted_foreground)
                            .child(self.status.clone()),
                    )
                    .when(self.edit.is_none(), |this| {
                        this.child(
                            Button::new("recipe-new")
                                .label("＋ 新建")
                                .xsmall()
                                .ghost()
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.start_edit(None, window, cx);
                                })),
                        )
                    }),
            )
            .child(
                div()
                    .id("recipes-list")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .p_2()
                    .child(body),
            )
    }
}

impl Focusable for RecipesPanel {
    fn focus_handle(&self, _: &gpui::App) -> FocusHandle {
        self.focus_handle.clone()
    }
}
