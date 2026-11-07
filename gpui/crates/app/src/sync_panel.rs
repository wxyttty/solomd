//! sync_panel.rs — GitHub/Gitea 同步右栏页：token + 状态 + 推/拉。

use gpui::{
    div, AppContext as _, Context, Entity, FocusHandle, Focusable, FontWeight, IntoElement,
    ParentElement as _, Render, SharedString, Styled as _, Window,
};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::input::{Input, InputState};
use gpui_component::{h_flex, v_flex, ActiveTheme as _, Sizable as _};
use solomd_core::github_sync;
use solomd_core::webdav_sync;

pub struct SyncPanel {
    focus_handle: FocusHandle,
    vault: Vault,
    token_input: Entity<InputState>,
    remote_input: Entity<InputState>,
    status: SharedString,
    /// WebDAV 同步（上游 #458）：服务器 / 账号 / 密码 / 远程目录 / 加密口令。
    wd_url: Entity<InputState>,
    wd_user: Entity<InputState>,
    wd_pass: Entity<InputState>,
    wd_path: Entity<InputState>,
    wd_passphrase: Entity<InputState>,
    wd_status: SharedString,
    /// WebDAV 操作进行中（防重入）。
    wd_busy: bool,
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
            wd_url: cx.new(|cx| {
                InputState::new(window, cx)
                    .placeholder("https://dav.jianguoyun.com/dav/")
            }),
            wd_user: cx.new(|cx| InputState::new(window, cx).placeholder("账号（邮箱）")),
            wd_pass: cx.new(|cx| {
                InputState::new(window, cx).placeholder("应用密码 / App password")
            }),
            wd_path: cx.new(|cx| InputState::new(window, cx).placeholder("远程目录，如 Notes")),
            wd_passphrase: cx.new(|cx| {
                InputState::new(window, cx).placeholder("端到端加密口令（留空 = 不加密）")
            }),
            wd_status: "未配置".into(),
            wd_busy: false,
        }
    }

    /// 从界面读 WebDAV 配置（url, user, pass, path, passphrase）。
    fn webdav_config(&self, cx: &Context<Self>) -> (String, String, String, String, String) {
        (
            self.wd_url.read(cx).value().to_string(),
            self.wd_user.read(cx).value().to_string(),
            self.wd_pass.read(cx).value().to_string(),
            self.wd_path.read(cx).value().to_string(),
            self.wd_passphrase.read(cx).value().to_string(),
        )
    }

    /// WebDAV：测试连接（PROPFIND Depth:0，404 时自动建目录）。
    fn webdav_test(&mut self, _: &gpui::ClickEvent, window: &mut Window, cx: &mut Context<Self>) {
        if self.wd_busy {
            return;
        }
        let (url, user, pass, path, _) = self.webdav_config(cx);
        if url.trim().is_empty() {
            self.wd_status = "请填服务器地址".into();
            cx.notify();
            return;
        }
        self.wd_busy = true;
        self.wd_status = "连接中…".into();
        cx.notify();
        cx.spawn_in(window, async move |this, cx| {
            let r = solomd_core::rt::spawn(async move {
                webdav_sync::webdav_test_connection(url, user, pass, path).await
            })
            .await;
            let _ = this.update_in(cx, |panel, window, cx| {
                panel.wd_busy = false;
                panel.wd_status = match r {
                    Ok(Ok(msg)) => SharedString::from(msg),
                    Ok(Err(e)) => SharedString::from(format!("失败：{e}")),
                    Err(e) => SharedString::from(format!("任务失败：{e}")),
                };
                window.refresh();
                cx.notify();
            });
        })
        .detach();
    }

    /// WebDAV：上传整个笔记库。
    fn webdav_push(&mut self, _: &gpui::ClickEvent, window: &mut Window, cx: &mut Context<Self>) {
        if self.wd_busy {
            return;
        }
        let (url, user, pass, path, passphrase) = self.webdav_config(cx);
        let workspace = self.vault.root.to_string_lossy().to_string();
        let encrypted = !passphrase.trim().is_empty();
        self.wd_busy = true;
        self.wd_status = if encrypted {
            "加密上传中…".into()
        } else {
            "上传中…".into()
        };
        cx.notify();
        cx.spawn_in(window, async move |this, cx| {
            let r = solomd_core::rt::spawn(async move {
                webdav_sync::webdav_push_workspace(
                    workspace,
                    url,
                    user,
                    pass,
                    path,
                    Some(encrypted),
                    Some(passphrase),
                )
                .await
            })
            .await;
            let _ = this.update_in(cx, |panel, window, cx| {
                panel.wd_busy = false;
                panel.wd_status = match r {
                    Ok(Ok(res)) => SharedString::from(res.message),
                    Ok(Err(e)) => SharedString::from(format!("上传失败：{e}")),
                    Err(e) => SharedString::from(format!("任务失败：{e}")),
                };
                window.refresh();
                cx.notify();
            });
        })
        .detach();
    }

    /// WebDAV：下载整个笔记库（覆盖同名本地文件）。
    fn webdav_pull(&mut self, _: &gpui::ClickEvent, window: &mut Window, cx: &mut Context<Self>) {
        if self.wd_busy {
            return;
        }
        let (url, user, pass, path, passphrase) = self.webdav_config(cx);
        let workspace = self.vault.root.to_string_lossy().to_string();
        let encrypted = !passphrase.trim().is_empty();
        self.wd_busy = true;
        self.wd_status = "下载中…".into();
        cx.notify();
        cx.spawn_in(window, async move |this, cx| {
            let r = solomd_core::rt::spawn(async move {
                webdav_sync::webdav_pull_workspace(
                    workspace,
                    url,
                    user,
                    pass,
                    path,
                    Some(encrypted),
                    Some(passphrase),
                )
                .await
            })
            .await;
            let _ = this.update_in(cx, |panel, window, cx| {
                panel.wd_busy = false;
                panel.wd_status = match r {
                    Ok(Ok(res)) => SharedString::from(res.message),
                    Ok(Err(e)) => SharedString::from(format!("下载失败：{e}")),
                    Err(e) => SharedString::from(format!("任务失败：{e}")),
                };
                window.refresh();
                cx.notify();
            });
        })
        .detach();
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
                    )
                    // ---- WebDAV 同步（坚果云 / Nextcloud / AList …）----
                    .child(
                        div()
                            .mt_3()
                            .pt_3()
                            .border_t_1()
                            .border_color(cx.theme().border),
                    )
                    .child(
                        h_flex()
                            .gap_2()
                            .child(div().font_weight(FontWeight::SEMIBOLD).child("WebDAV 同步"))
                            .child(
                                div()
                                    .text_xs()
                                    .min_w_0()
                                    .truncate()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(self.wd_status.clone()),
                            ),
                    )
                    .child(
                        v_flex()
                            .gap_1()
                            .child(Input::new(&self.wd_url).small())
                            .child(Input::new(&self.wd_user).small())
                            .child(Input::new(&self.wd_pass).small())
                            .child(Input::new(&self.wd_path).small())
                            .child(Input::new(&self.wd_passphrase).small()),
                    )
                    .child(
                        h_flex()
                            .gap_1p5()
                            .child(
                                Button::new("webdav-test")
                                    .label(if self.wd_busy { "…" } else { "测试连接" })
                                    .xsmall()
                                    .ghost()
                                    .on_click(cx.listener(Self::webdav_test)),
                            )
                            .child(
                                Button::new("webdav-push")
                                    .label("⬆ 上传笔记库")
                                    .xsmall()
                                    .primary()
                                    .on_click(cx.listener(Self::webdav_push)),
                            )
                            .child(
                                Button::new("webdav-pull")
                                    .label("⬇ 下载笔记库")
                                    .xsmall()
                                    .ghost()
                                    .on_click(cx.listener(Self::webdav_pull)),
                            ),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child("填了口令 = 端到端加密（远程只存 .enc）；留空 = 明文同步。"),
                    ),
            )
    }
}

impl Focusable for SyncPanel {
    fn focus_handle(&self, _: &gpui::App) -> gpui::FocusHandle {
        self.focus_handle.clone()
    }
}
