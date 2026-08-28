//! sync_panel.rs — GitHub/Gitea 同步右栏页：token + 状态 + 推/拉。

use gpui::{
    div, AppContext as _, Context, Entity, FocusHandle, Focusable, FontWeight, IntoElement,
    ParentElement as _, Render, SharedString, Styled as _, Window,
};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::input::{Input, InputState};
use gpui_component::{h_flex, v_flex, ActiveTheme as _, Sizable as _};
use solomd_core::github_sync;

pub struct SyncPanel {
    focus_handle: FocusHandle,
    vault: Vault,
    token_input: Entity<InputState>,
    remote_input: Entity<InputState>,
    status: SharedString,
}

use crate::vault::Vault;

impl SyncPanel {
    pub fn new(vault: Vault, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let has_token = github_sync::github_has_token().unwrap_or(false);
        Self {
            focus_handle: cx.focus_handle(),
            vault,
            token_input: cx.new(|cx| {
                InputState::new(window, cx).placeholder(if has_token {
                    "已存有 token（粘贴新值可覆盖）"
                } else {
                    "粘贴 GitHub Personal Access Token"
                })
            }),
            remote_input: cx
                .new(|cx| InputState::new(window, cx).placeholder("git@github.com:user/repo.git")),
            status: if has_token {
                "已配置".into()
            } else {
                "未配置 token".into()
            },
        }
    }

    fn save_token(&mut self, _: &gpui::ClickEvent, window: &mut Window, cx: &mut Context<Self>) {
        let token = self.token_input.read(cx).value().to_string();
        if token.trim().is_empty() {
            return;
        }
        match github_sync::github_set_token(token.trim().to_string()) {
            Ok(()) => {
                self.status = "token 已保存（钥匙串）".into();
                self.token_input.update(cx, |q, cx| {
                    q.set_value("", window, cx);
                });
                cx.notify();
            }
            Err(e) => self.status = format!("保存失败：{e}").into(),
        }
    }

    fn link(&mut self, _: &gpui::ClickEvent, window: &mut Window, cx: &mut Context<Self>) {
        let remote = self.remote_input.read(cx).value().to_string();
        if remote.trim().is_empty() {
            self.status = "请填 remote URL".into();
            cx.notify();
            return;
        }
        self.status = "关联中…".into();
        let folder = self.vault.root.to_string_lossy().to_string();
        cx.spawn_in(window, async move |this, cx| {
            let r = github_sync::github_link_workspace(folder, remote.trim().to_string(), None, None)
                .await;
            let _ = this.update_in(cx, |panel, window, _cx| {
                match r {
                    Ok(_) => panel.status = "已关联远程仓库".into(),
                    Err(e) => panel.status = format!("关联失败：{e}").into(),
                }
                window.refresh();
            });
        })
        .detach();
    }

    fn refresh_status(&mut self, _: &gpui::ClickEvent, window: &mut Window, cx: &mut Context<Self>) {
        let folder = self.vault.root.to_string_lossy().to_string();
        self.status = "查询状态…".into();
        cx.spawn_in(window, async move |this, cx| {
            let r = github_sync::github_sync_status(folder).await;
            let _ = this.update_in(cx, |panel, window, _cx| {
                match r {
                    Ok(st) if st.linked => {
                        panel.status = SharedString::from(format!(
                            "已链接 {} · 领先{} 落后{}{}",
                            st.remote_url,
                            st.ahead,
                            st.behind,
                            if st.dirty { " · 有未提交" } else { "" }
                        ));
                    }
                    Ok(_) => panel.status = "未链接远程".into(),
                    Err(e) => panel.status = format!("状态失败：{e}").into(),
                }
                window.refresh();
            });
        })
        .detach();
    }

    fn push(&mut self, _: &gpui::ClickEvent, window: &mut Window, cx: &mut Context<Self>) {
        let folder = self.vault.root.to_string_lossy().to_string();
        self.status = "推送中…".into();
        cx.spawn_in(window, async move |this, cx| {
            let r = github_sync::github_push(folder, None).await;
            let _ = this.update_in(cx, |panel, window, _cx| {
                match r {
                    Ok(()) => panel.status = "推送完成".into(),
                    Err(e) => panel.status = format!("推送失败：{e}").into(),
                }
                window.refresh();
            });
        })
        .detach();
    }

    fn pull(&mut self, _: &gpui::ClickEvent, window: &mut Window, cx: &mut Context<Self>) {
        let folder = self.vault.root.to_string_lossy().to_string();
        self.status = "拉取中…".into();
        cx.spawn_in(window, async move |this, cx| {
            let r = github_sync::github_pull(folder).await;
            let _ = this.update_in(cx, |panel, window, _cx| {
                match r {
                    Ok(res) => {
                        panel.status = match res.kind.as_str() {
                            "conflicts" => format!("⚠ 冲突：{}", res.conflicts.join(", ")).into(),
                            "up_to_date" => "已是最新".into(),
                            _ => "拉取完成（快进）".into(),
                        };
                    }
                    Err(e) => panel.status = format!("拉取失败：{e}").into(),
                }
                window.refresh();
            });
        })
        .detach();
    }
}

impl Render for SyncPanel {
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
                    .child(div().font_weight(FontWeight::SEMIBOLD).child("GitHub 同步"))
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(self.status.clone()),
                    ),
            )
            .child(
                div()
                    .p_3()
                    .flex()
                    .flex_col()
                    .gap_2p5()
                    .text_sm()
                    // 1. token
                    .child(
                        v_flex()
                            .gap_1()
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(cx.theme().muted_foreground)
                                    .child("Personal Access Token（存钥匙串）"),
                            )
                            .child(Input::new(&self.token_input).small())
                            .child(
                                Button::new("sync-token")
                                    .label("保存 token")
                                    .xsmall()
                                    .ghost()
                                    .on_click(cx.listener(Self::save_token)),
                            ),
                    )
                    // 2. remote 关联
                    .child(
                        v_flex()
                            .gap_1()
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(cx.theme().muted_foreground)
                                    .child("远程仓库 URL"),
                            )
                            .child(Input::new(&self.remote_input).small())
                            .child(
                                h_flex()
                                    .gap_1p5()
                                    .child(
                                        Button::new("sync-link")
                                            .label("关联本库")
                                            .xsmall()
                                            .ghost()
                                            .on_click(cx.listener(Self::link)),
                                    )
                                    .child(
                                        Button::new("sync-status")
                                            .label("刷新状态")
                                            .xsmall()
                                            .ghost()
                                            .on_click(cx.listener(Self::refresh_status)),
                                    ),
                            ),
                    )
                    // 3. 动作
                    .child(
                        h_flex()
                            .gap_1p5()
                            .child(
                                Button::new("sync-push")
                                    .label("⬆ 推送")
                                    .xsmall()
                                    .primary()
                                    .on_click(cx.listener(Self::push)),
                            )
                            .child(
                                Button::new("sync-pull")
                                    .label("⬇ 拉取")
                                    .xsmall()
                                    .ghost()
                                    .on_click(cx.listener(Self::pull)),
                            ),
                    ),
            )
    }
}

impl Focusable for SyncPanel {
    fn focus_handle(&self, _: &gpui::App) -> gpui::FocusHandle {
        self.focus_handle.clone()
    }
}
