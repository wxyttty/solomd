//! history_panel.rs — Git 历史右栏页：当前笔记的提交列表 + 展开 diff + 恢复。
//!
//! 走 core 的 `git_history::*_inner` 同步函数（GPUI background_executor 上
//! 执行，与搜索面板同款；async 包装属于 tokio 桥接，这里用不上）。
//! 恢复 = 回写历史版本到磁盘：打开中的干净标签由 Workspace 的外部变更
//! 轮询（2s）自动重载；脏标签不动（编辑器版本优先，⌘S 会覆盖恢复结果）。

use std::path::PathBuf;

use gpui::{
    div, px, prelude::FluentBuilder as _, Context, FocusHandle, Focusable, FontWeight,
    InteractiveElement as _, IntoElement, ParentElement as _, Render, SharedString,
    StatefulInteractiveElement as _, Styled as _, Window,
};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::{h_flex, v_flex, ActiveTheme as _, Sizable as _};
use solomd_core::git_history::{
    git_auto_commit_inner, git_file_diff_inner, git_file_history_inner, git_init_workspace_inner,
    git_rollback_file_inner, git_workspace_status_inner, CommitMeta, WorkspaceStatus,
};

use crate::vault::Vault;

pub struct HistoryPanel {
    focus_handle: FocusHandle,
    vault: Vault,
    /// Workspace 推送的当前活跃笔记（绝对路径）。
    active: Option<PathBuf>,
    /// None = 状态查询中。
    initialized: Option<bool>,
    branch: Option<String>,
    commits: Vec<CommitMeta>,
    /// 展开中的提交 sha。
    expanded: Option<String>,
    diff_unified: Option<String>,
    diff_err: Option<String>,
    busy: bool,
    status: SharedString,
}

impl HistoryPanel {
    pub fn new(vault: Vault, _window: &mut Window, cx: &mut Context<Self>) -> Self {
        let mut this = Self {
            focus_handle: cx.focus_handle(),
            vault,
            active: None,
            initialized: None,
            branch: None,
            commits: Vec::new(),
            expanded: None,
            diff_unified: None,
            diff_err: None,
            busy: false,
            status: "选择笔记后显示历史".into(),
        };
        this.refresh(cx);
        this
    }

    /// Workspace 在笔记打开/激活时推送。
    pub fn set_active(&mut self, path: Option<PathBuf>, cx: &mut Context<Self>) {
        if self.active == path {
            return;
        }
        self.active = path;
        self.refresh(cx);
    }

    fn refresh(&mut self, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        let Some(path) = self.active.clone() else {
            self.initialized = None;
            self.commits.clear();
            self.status = "没有打开的笔记".into();
            cx.notify();
            return;
        };
        self.busy = true;
        self.status = "读取中…".into();
        cx.notify();

        let folder = self.vault.root.to_string_lossy().to_string();
        let file = path.to_string_lossy().to_string();
        cx.spawn(async move |this, cx| {
            let f_status = folder.clone();
            let st = cx
                .background_executor()
                .spawn(async move { git_workspace_status_inner(f_status) })
                .await;
            let history = cx
                .background_executor()
                .spawn(async move { git_file_history_inner(folder, file, 50) })
                .await;
            let _ = this.update(cx, |panel, cx| {
                panel.busy = false;
                match st {
                    Ok(WorkspaceStatus {
                        initialized, branch, ..
                    }) => {
                        panel.initialized = Some(initialized);
                        panel.branch = branch;
                    }
                    Err(e) => {
                        panel.initialized = Some(false);
                        panel.status = SharedString::from(format!("状态失败：{e}"));
                    }
                }
                match history {
                    Ok(commits) => {
                        panel.commits = commits;
                        if panel.status.starts_with("读取中") {
                            panel.status = if panel.commits.is_empty() {
                                "还没有提交记录".into()
                            } else {
                                SharedString::from(format!("{} 个提交", panel.commits.len()))
                            };
                        }
                    }
                    Err(e) => {
                        panel.commits.clear();
                        panel.status = SharedString::from(format!("历史失败：{e}"));
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn init_git(&mut self, _: &gpui::ClickEvent, window: &mut Window, cx: &mut Context<Self>) {
        let folder = self.vault.root.to_string_lossy().to_string();
        self.status = "初始化中…".into();
        cx.notify();
        cx.spawn_in(window, async move |this, cx| {
            let r = cx
                .background_executor()
                .spawn(async move { git_init_workspace_inner(folder, None, None) })
                .await;
            let _ = this.update_in(cx, |panel, window, cx| {
                match r {
                    Ok(()) => panel.status = "Git 仓库已初始化".into(),
                    Err(e) => panel.status = SharedString::from(format!("初始化失败：{e}")),
                }
                panel.refresh(cx);
                window.refresh();
            });
        })
        .detach();
    }

    /// 提交当前文件（给当前内容留档）。
    fn commit_file(&mut self, _: &gpui::ClickEvent, window: &mut Window, cx: &mut Context<Self>) {
        let Some(path) = self.active.clone() else { return };
        let folder = self.vault.root.to_string_lossy().to_string();
        let file = path.to_string_lossy().to_string();
        self.status = "提交中…".into();
        cx.notify();
        cx.spawn_in(window, async move |this, cx| {
            let r = cx
                .background_executor()
                .spawn(async move { git_auto_commit_inner(folder, Some(file), None) })
                .await;
            let _ = this.update_in(cx, |panel, window, cx| {
                panel.status = match r {
                    Ok(Some(sha)) => {
                        SharedString::from(format!("已提交 {}", &sha[..7.min(sha.len())]))
                    }
                    Ok(None) => "没有可提交的修改".into(),
                    Err(e) => SharedString::from(format!("提交失败：{e}")),
                };
                panel.refresh(cx);
                window.refresh();
            });
        })
        .detach();
    }

    fn toggle_expand(&mut self, sha: String, window: &mut Window, cx: &mut Context<Self>) {
        if self.expanded.as_deref() == Some(sha.as_str()) {
            self.expanded = None;
            self.diff_unified = None;
            self.diff_err = None;
            cx.notify();
            return;
        }
        let Some(path) = self.active.clone() else { return };
        self.expanded = Some(sha.clone());
        self.diff_unified = None;
        self.diff_err = None;
        self.status = "载入 diff…".into();
        cx.notify();

        let folder = self.vault.root.to_string_lossy().to_string();
        let file = path.to_string_lossy().to_string();
        cx.spawn_in(window, async move |this, cx| {
            let r = cx
                .background_executor()
                .spawn(async move { git_file_diff_inner(folder, file, sha) })
                .await;
            let _ = this.update_in(cx, |panel, window, _cx| {
                match r {
                    Ok(d) => panel.diff_unified = Some(d.unified),
                    Err(e) => panel.diff_err = Some(e),
                }
                panel.status = "就绪".into();
                window.refresh();
            });
        })
        .detach();
    }

    /// 恢复：历史版本回写磁盘（干净标签 2s 内自动重载，见模块注释）。
    fn restore(&mut self, sha: String, window: &mut Window, cx: &mut Context<Self>) {
        let Some(path) = self.active.clone() else { return };
        let folder = self.vault.root.to_string_lossy().to_string();
        let file = path.to_string_lossy().to_string();
        cx.spawn_in(window, async move |this, cx| {
            let r = cx
                .background_executor()
                .spawn(async move { git_rollback_file_inner(folder, file, sha) })
                .await;
            let _ = this.update_in(cx, |panel, window, _cx| {
                panel.status = match r {
                    Ok(()) => "已恢复；打开中的干净标签将自动重载".into(),
                    Err(e) => SharedString::from(format!("恢复失败：{e}")),
                };
                window.refresh();
            });
        })
        .detach();
    }
}

fn time_ago(secs: i64) -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let d = (now - secs).max(0);
    if d < 60 {
        "刚刚".into()
    } else if d < 3600 {
        format!("{} 分钟前", d / 60)
    } else if d < 86400 {
        format!("{} 小时前", d / 3600)
    } else {
        format!("{} 天前", d / 86400)
    }
}

impl Render for HistoryPanel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let mut body = v_flex().p_2().gap_1p5();

        match self.initialized {
            None => {
                body = body.child(
                    div()
                        .text_sm()
                        .text_color(cx.theme().muted_foreground)
                        .child(self.status.clone()),
                );
            }
            Some(false) => {
                body = body
                    .child(
                        div()
                            .text_sm()
                            .text_color(cx.theme().muted_foreground)
                            .child("此笔记库还没有 Git 历史。初始化后可提交/回看/恢复文件版本。"),
                    )
                    .child(
                        Button::new("git-init")
                            .label("初始化 Git 历史")
                            .xsmall()
                            .primary()
                            .on_click(cx.listener(Self::init_git)),
                    );
            }
            Some(true) => {
                if self.commits.is_empty() {
                    body = body.child(
                        div()
                            .text_sm()
                            .text_color(cx.theme().muted_foreground)
                            .child("此文件还没有提交记录。"),
                    );
                }
                let expanded = self.expanded.clone();
                let diff_unified = self.diff_unified.clone();
                let diff_err = self.diff_err.clone();
                for c in &self.commits {
                    let sha = c.sha.clone();
                    let is_open = expanded.as_deref() == Some(c.sha.as_str());
                    body = body.child(
                        v_flex()
                            .rounded_md()
                            .border_1()
                            .border_color(cx.theme().border)
                            .child(
                                div()
                                    .id(SharedString::from(format!("commit-{}", c.sha)))
                                    .px_2()
                                    .py_1p5()
                                    .cursor_pointer()
                                    .hover(|s| s.bg(cx.theme().accent.opacity(0.12)))
                                    .on_click({
                                        let sha = sha.clone();
                                        cx.listener(move |this, _, window, cx| {
                                            this.toggle_expand(sha.clone(), window, cx);
                                        })
                                    })
                                    .child(
                                        div()
                                            .text_sm()
                                            .font_weight(FontWeight::MEDIUM)
                                            .child(c.message.clone()),
                                    )
                                    .child(
                                        h_flex()
                                            .gap_2()
                                            .child(
                                                div()
                                                    .text_xs()
                                                    .text_color(cx.theme().muted_foreground)
                                                    .child(c.short_sha.clone()),
                                            )
                                            .child(
                                                div()
                                                    .text_xs()
                                                    .text_color(cx.theme().muted_foreground)
                                                    .child(time_ago(c.time)),
                                            )
                                            .child(
                                                div()
                                                    .text_xs()
                                                    .text_color(cx.theme().muted_foreground)
                                                    .child(c.author.clone()),
                                            ),
                                    ),
                            )
                            .when(is_open, |this| {
                                let diff_view: gpui::AnyElement =
                                    if let Some(unified) = &diff_unified {
                                        render_diff(unified, cx)
                                    } else if let Some(err) = &diff_err {
                                        div()
                                            .text_xs()
                                            .text_color(cx.theme().danger)
                                            .child(err.clone())
                                            .into_any_element()
                                    } else {
                                        div()
                                            .text_xs()
                                            .text_color(cx.theme().muted_foreground)
                                            .child("…")
                                            .into_any_element()
                                    };
                                this.child(
                                    v_flex()
                                        .px_2()
                                        .pb_2()
                                        .gap_1()
                                        .child(diff_view)
                                        .child(
                                            Button::new(SharedString::from(format!(
                                                "restore-{}",
                                                c.sha
                                            )))
                                            .label("恢复此版本")
                                            .xsmall()
                                            .ghost()
                                            .on_click({
                                                let sha = c.sha.clone();
                                                cx.listener(move |this, _, window, cx| {
                                                    this.restore(sha.clone(), window, cx);
                                                })
                                            }),
                                        ),
                                )
                            }),
                    );
                }
            }
        }

        v_flex()
            .size_full()
            .child(
                h_flex()
                    .px_3()
                    .py_1p5()
                    .gap_2()
                    .border_b_1()
                    .border_color(cx.theme().border)
                    .child(div().font_weight(FontWeight::SEMIBOLD).child("历史"))
                    .child(
                        div()
                            .text_xs()
                            .flex_1()
                            .truncate()
                            .text_color(cx.theme().muted_foreground)
                            .child(match &self.branch {
                                Some(b) => SharedString::from(format!("{b} · {}", self.status)),
                                None => self.status.clone(),
                            }),
                    )
                    .when(self.initialized == Some(true), |this| {
                        this.child(
                            Button::new("git-commit")
                                .label("提交当前文件")
                                .xsmall()
                                .ghost()
                                .on_click(cx.listener(Self::commit_file)),
                        )
                        .child(
                            Button::new("git-refresh")
                                .label("刷新")
                                .xsmall()
                                .ghost()
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.refresh(cx);
                                })),
                        )
                    }),
            )
            .child(
                div()
                    .id("history-scroll")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .child(body),
            )
    }
}

fn render_diff(unified: &str, cx: &Context<HistoryPanel>) -> gpui::AnyElement {
    let mut lines: Vec<gpui::AnyElement> = Vec::new();
    for line in unified.lines() {
        let el = if line.starts_with('+') {
            div()
                .text_xs()
                .font_family("Menlo")
                .text_color(cx.theme().success)
                .child(line.to_string())
                .into_any_element()
        } else if line.starts_with('-') {
            div()
                .text_xs()
                .font_family("Menlo")
                .text_color(cx.theme().danger)
                .child(line.to_string())
                .into_any_element()
        } else if line.starts_with("@@") {
            div()
                .text_xs()
                .font_family("Menlo")
                .text_color(cx.theme().muted_foreground)
                .child(line.to_string())
                .into_any_element()
        } else {
            div()
                .text_xs()
                .font_family("Menlo")
                .text_color(cx.theme().foreground)
                .child(line.to_string())
                .into_any_element()
        };
        lines.push(el);
    }
    v_flex()
        .id("diff-scroll")
        .rounded_sm()
        .p_1()
        .bg(cx.theme().muted.opacity(0.25))
        .max_h(px(240.))
        .overflow_y_scroll()
        .children(lines)
        .into_any_element()
}

impl Focusable for HistoryPanel {
    fn focus_handle(&self, _: &gpui::App) -> FocusHandle {
        self.focus_handle.clone()
    }
}
