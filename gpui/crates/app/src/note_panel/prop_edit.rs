//! prop_edit.rs — `key:: value` 属性面板的结构化编辑会话。
//!
//! zorite 编辑器把 `key:: value` 连续行渲染成两列面板（WYSIWYG），光标
//! 点击/方向键进入时发 `EditorEvent::EditProperties`。宿主义务（API.md
//! seat/commit 协议，与 math_edit 同款）：
//!   1. 用 `source` 构建编辑视图，`set_editing_block` 座入块位；
//!   2. 提交时 `end_editing_block` 取回 range，`replace_range` 整块覆写；
//!   3. 失焦提交（点外面）；Enter 在最后一行 = 提交并出向。
//!
//! 编辑视图：每行一对 key/value 输入框（gpui-component InputState），
//! 保留原行的前缀（缩进 + 列表标记，Logseq `- key:: value` 形态）。
//! 序列化回写时逐行 `prefix + key + ":: " + value`。

use std::ops::Range;

use gpui::{
    div, prelude::FluentBuilder as _, App, AppContext as _, Context, Entity, Focusable,
    InteractiveElement as _, IntoElement, ParentElement as _, Render, SharedString, Styled as _,
    Window,
};
use gpui_component::input::{InputEvent, InputState};
use gpui_component::{h_flex, v_flex, ActiveTheme as _, Sizable as _};
use zorite_markdown::syntax::prefixed_property;

use super::NotePanel;

/// 座入 zorite 的属性编辑视图：行列表 + 提交信号（Enter 越界 = Exit）。
pub(super) struct PropEditor {
    rows: Vec<(Entity<InputState>, Entity<InputState>)>,
    /// 每行是否解析成功（key:: value 结构）。失败的行 serialize 时原样
    /// 写回原文，不进结构化路径——防止进入又退出就静默删文/改文。
    parsed: Vec<bool>,
    /// 每行的原样行文本（解析失败行写回用；追加行 = 空串）。
    raw_lines: Vec<String>,
    /// 每行的前缀（缩进/列表标记），写回时拼回。
    prefixes: Vec<String>,
    /// 聚焦行（EditProperties.row / at_end 决定初始焦点）。
    focus_row: usize,
    focus_value: bool,
    /// 容器级焦点句柄：blur 监听挂这里（字段间切焦不触发提交，
    /// 整个表单失焦才提交——单字段句柄会在 key↔value 切换时误提交）。
    container_focus: gpui::FocusHandle,
}

/// PropEditor → NotePanel 的导航事件（与 ratex MathNav 同款语义）。
pub(super) enum PropNav {
    /// Enter 在最后一行（value 尾）= 提交并把光标落到块后。
    Exit,
}

impl PropEditor {
    fn new(
        source: &str,
        row: Option<usize>,
        at_end: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut rows = Vec::new();
        let mut prefixes = Vec::new();
        let mut raw_lines = Vec::new();
        let mut parsed = Vec::new();
        for line in source.lines() {
            raw_lines.push(line.to_string());
            // prefixed_property 语义：prefix 原样保留（含缩进与列表标记）。
            // 解析失败的行：key/value 留空、parsed=false，serialize 原样写回。
            match prefixed_property(line) {
                Some((prefix, key, value)) => {
                    let k = cx.new(|cx| InputState::new(window, cx).placeholder("key"));
                    k.update(cx, |s, cx| s.set_value(key, window, cx));
                    let v = cx.new(|cx| InputState::new(window, cx).placeholder("value"));
                    v.update(cx, |s, cx| s.set_value(value, window, cx));
                    rows.push((k, v));
                    prefixes.push(prefix.to_string());
                    parsed.push(true);
                }
                None => {
                    let k = cx.new(|cx| InputState::new(window, cx).placeholder("key"));
                    let v = cx.new(|cx| InputState::new(window, cx).placeholder("value"));
                    rows.push((k, v));
                    prefixes.push(String::new());
                    parsed.push(false);
                }
            }
        }
        if rows.is_empty() {
            // 空块（不该发生，property_block_at 只返回非空区域）：给一行空行。
            let k = cx.new(|cx| InputState::new(window, cx).placeholder("key"));
            let v = cx.new(|cx| InputState::new(window, cx).placeholder("value"));
            rows.push((k, v));
            prefixes.push(String::new());
            raw_lines.push(String::new());
            parsed.push(true);
        }
        // 初始焦点：点击/Enter 带具体行；方向键进入按 at_end（末行 value / 首行 key）。
        let focus_row = row.unwrap_or(if at_end { rows.len() - 1 } else { 0 });
        let focus_value = at_end || row.is_some();
        let this = Self {
            rows,
            parsed,
            raw_lines,
            prefixes,
            focus_row,
            focus_value,
            container_focus: cx.focus_handle(),
        };
        // Enter 提交：订阅每个输入框的 PressEnter（最后一行 value = Exit，
        // 其余 = 焦点跳下一行——简化 v1：全部 Exit，Enter 即提交出向）。
        let row_count = this.rows.len();
        for (k, v) in &this.rows {
            for input in [k, v] {
                cx.subscribe_in(input, window, move |this, _, ev: &InputEvent, window, cx| {
                    if matches!(ev, InputEvent::PressEnter { .. }) {
                        let _ = row_count;
                        cx.emit(PropNav::Exit);
                        let _ = (this, window);
                    }
                })
                .detach();
            }
        }
        this
    }

    /// 序列化回 `key:: value` 行块。空 key 行 = 用户主动清空 = 删除该行；
    /// 但解析失败的原行（key 输入框预填了整行文本却不含 `::`）原样保留，
    /// 防止进入又退出就静默删文。
    fn serialize(&self, cx: &App) -> String {
        let mut out = String::new();
        for (ix, (k, v)) in self.rows.iter().enumerate() {
            // 解析失败的行：原样写回原文（不进结构化路径——防止进入又
            // 退出就静默删文；用户在 key/value 里输入的内容丢弃，因为该
            // 行本就不是属性结构）。
            if !self.parsed[ix] {
                if !self.raw_lines[ix].is_empty() {
                    out.push_str(&self.raw_lines[ix]);
                    out.push('\n');
                }
                continue;
            }
            let key = k.read(cx).value().trim().to_string();
            let value = v.read(cx).value().to_string();
            if key.is_empty() {
                continue; // 空 key 行丢弃（用户删除属性行的方式）
            }
            out.push_str(&self.prefixes[ix]);
            out.push_str(&key);
            out.push_str(":: ");
            out.push_str(value.trim());
            out.push('\n');
        }
        // 去掉末尾换行：replace_range 覆写的块不含尾随换行（原块同样不含）。
        out.trim_end_matches('\n').to_string()
    }
}

impl gpui::EventEmitter<PropNav> for PropEditor {}

impl Render for PropEditor {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        // 容器 track_focus：blur 监听挂容器句柄，字段间切焦不触发提交。
        let mut list = v_flex()
            .track_focus(&self.container_focus)
            .gap_1()
            .p_2()
            .rounded_md()
            .border_1()
            .border_color(theme.border)
            .bg(theme.background);
        for (ix, (k, v)) in self.rows.iter().enumerate() {
            list = list.child(
                h_flex()
                    .gap_1()
                    .child(
                        div()
                            .w(gpui::px(120.))
                            .min_w_0()
                            .child(gpui_component::input::Input::new(k).small()),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .child(gpui_component::input::Input::new(v).small()),
                    )
                    .when(ix + 1 == self.rows.len(), |row| {
                        // 末行加「+ 行」按钮：追加空属性行。
                        row.child(
                            gpui_component::button::Button::new("prop-add-row")
                                .label("＋")
                                .xsmall()
                                .on_click(cx.listener(|this, _, window, cx| {
                                    let k = cx.new(|cx| InputState::new(window, cx).placeholder("key"));
                                    let v = cx.new(|cx| InputState::new(window, cx).placeholder("value"));
                                    // 新行的 Enter 订阅（与 new 里同款：Enter = 提交出向）。
                                    cx.subscribe_in(&v, window, |_this, _, ev: &InputEvent, _window, cx| {
                                        if matches!(ev, InputEvent::PressEnter { .. }) {
                                            cx.emit(PropNav::Exit);
                                        }
                                    })
                                    .detach();
                                    // 三组数据同步追加（raw_lines 缺失会让 serialize 越界）。
                                    this.prefixes.push(String::new());
                                    this.raw_lines.push(String::new());
                                    this.rows.push((k, v));
                                    cx.notify();
                                })),
                        )
                    }),
            );
        }
        list
    }
}

impl Focusable for PropEditor {
    fn focus_handle(&self, _cx: &App) -> gpui::FocusHandle {
        // 容器句柄（blur 监听挂这里）。初始焦点在 new 后由 open_prop_edit
        // 直接 focus 到具体输入框。
        self.container_focus.clone()
    }
}

/// 会话状态（math_edit::MathEditSession 同款：编辑器实体 + 持活订阅）。
pub(super) struct PropEditSession {
    pub(super) editor: Entity<PropEditor>,
    /// 座入时的块原文：提交前验证目标范围仍等于它（编辑期间外部改动
    /// 会让范围过期——盲覆写会破坏错位文本）。过期时拒绝覆写并保留
    /// 会话内容（用户编辑不丢，可重试）。
    pub(super) source: SharedString,
    /// 失焦提交。持活即可。
    pub(super) _blur_sub: gpui::Subscription,
    /// Enter 提交出向。持活即可。
    pub(super) _nav_sub: gpui::Subscription,
}

impl NotePanel {
    /// 光标/点击进入属性面板：座入 PropEditor（API.md seat/commit 协议）。
    pub(super) fn open_prop_edit(
        &mut self,
        range: Range<usize>,
        source: SharedString,
        at_end: bool,
        row: Option<usize>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // 互斥：公式与属性共用 zorite 的 editing_block 槽位——先提交对方
        // 会话，避免覆盖仍在编辑的视图。
        if self.prop_edit.is_some() {
            self.commit_prop_edit(cx);
        }
        if self.math_edit.is_some() {
            self.commit_math_edit(cx);
        }
        // 提交可能移动过字节偏移——先验证原范围内容，再按 source 附近
        // 重定位。两者都失败（范围过期且找不到原文）拒绝座入，而不是
        // 盲用旧 range 覆写错误文本。优先原范围：避免附近另一个相同
        // 属性块被 match_indices 误选。
        let text = self.state.read(cx).text().to_string();
        let range = if text.get(range.clone()).is_some_and(|seg| seg == source.as_str()) {
            range
        } else {
            match text
                .match_indices(source.as_str())
                .find(|(pos, _)| *pos >= range.start.saturating_sub(256) && *pos < range.end + 256)
                .map(|(pos, seg)| pos..pos + seg.len())
            {
                Some(r) => r,
                None => {
                    eprintln!("[prop] 范围定位失败，拒绝座入");
                    return;
                }
            }
        };

        let editor = cx.new(|cx| PropEditor::new(&source, row, at_end, window, cx));
        // 块高：行数 × 行高估算（输入框 small ≈ 28px + gap 4px + padding）。
        let n = editor.read(cx).rows.len();
        let height = gpui::px(40.0 + n as f32 * 32.0);
        self.state.update(cx, |e, cx| {
            e.set_editing_block(range, editor.clone().into(), height, cx)
        });

        // 失焦提交（整个表单失焦，不是单字段——key↔value 切焦不触发）；
        // 身份守卫防旧 blur 提交新会话。
        let weak = cx.entity().downgrade();
        let editor_id = editor.entity_id();
        let focus = editor.read(cx).focus_handle(cx);
        let blur_sub = window.on_focus_out(&focus, cx, move |_ev, _window, cx| {
            weak.update(cx, |this, cx| {
                if this
                    .prop_edit
                    .as_ref()
                    .is_some_and(|m| m.editor.entity_id() == editor_id)
                {
                    this.commit_prop_edit(cx);
                }
            })
            .ok();
        });
        // Enter = 提交并把正文光标落到块后下一行（zorite exit_math 同款：
        // 落块尾行内会露出原始源码；块在文末时补新行）。
        let nav_sub = cx.subscribe_in(
            &editor,
            window,
            |this, _editor, _ev: &PropNav, window, cx| {
                if let Some(block) = this.commit_prop_edit(cx) {
                    this.state.update(cx, |e, cx| {
                        e.focus(window, cx);
                        e.exit_math(block, true, window, cx);
                    });
                    // exit_math 文末补行会改缓冲（replace_range 插 \n）：
                    // 提交已写盘标 clean，这里补一次落盘 + 保持 clean，
                    // 否则缓冲与盘不一致且脏态丢失。
                    let new = this.state.read(cx).text().to_string();
                    if let Err(e) = this.vault.write_note(&this.path, &new) {
                        eprintln!("[prop] 出向补行落盘失败 {}: {e}", this.path.display());
                        this.dirty = true;
                    }
                    cx.notify();
                }
            },
        );
        self.prop_edit = Some(PropEditSession {
            editor: editor.clone(),
            source: source.clone(),
            _blur_sub: blur_sub,
            _nav_sub: nav_sub,
        });
        // 初始焦点：具体输入框（focus_handle 现在返回容器句柄，blur 监听用）。
        let (k, v) = {
            let ed = editor.read(cx);
            let ix = ed.focus_row.min(ed.rows.len() - 1);
            (ed.rows[ix].0.clone(), ed.rows[ix].1.clone())
        };
        let input = if editor.read(cx).focus_value { v } else { k };
        let input_focus = input.read(cx).focus_handle(cx);
        window.focus(&input_focus, cx);
        cx.notify();
    }

    /// 提交：序列化 → end_editing_block 取回 range → replace_range 覆写
    /// → 落盘。返回新块 range（None = 无会话）。
    /// 提交：序列化 → end_editing_block 取回 range → **验证范围内容仍
    /// 等于座入时的 source**（编辑期间外部改动会让范围过期，盲覆写破坏
    /// 错位文本；过期时拒绝覆写、恢复会话供重试）→ replace_range → 落盘。
    /// 返回新块 range（写盘失败时也返回实际新范围——缓冲已换内容，
    /// 导航必须按真实范围，不能用旧 range）。
    pub(super) fn commit_prop_edit(&mut self, cx: &mut Context<Self>) -> Option<Range<usize>> {
        let edit = self.prop_edit.take()?;
        let new_block = edit.editor.read(cx).serialize(cx);
        let range = self
            .state
            .update(cx, |e, cx| e.end_editing_block(cx))?;
        // 范围内容校验：目标必须是座入时的原文。
        let text = self.state.read(cx).text().to_string();
        let valid = text
            .get(range.clone())
            .is_some_and(|seg| seg == edit.source.as_str());
        if !valid {
            eprintln!("[prop] 范围已过期（内容不匹配），拒绝覆写");
            // 恢复会话：用户编辑不丢。不重新座入（范围已漂移，重座入
            // 需重新定位——留给下一次进入事件）。
            self.prop_edit = Some(PropEditSession {
                editor: edit.editor,
                source: edit.source,
                _blur_sub: edit._blur_sub,
                _nav_sub: edit._nav_sub,
            });
            cx.notify();
            return None;
        }
        self.state
            .update(cx, |e, cx| e.replace_range(range.clone(), &new_block, cx));
        let new_range = range.start..range.start + new_block.len();
        let new = self.state.read(cx).text().to_string();
        if let Err(e) = self.vault.write_note(&self.path, &new) {
            // 缓冲已改但盘上失败：保持脏态，用户 ⌘S 重试（不能静默吞错后
            // 又标 clean，那会让用户以为已保存）。返回实际新范围供导航。
            eprintln!("[prop] 回写失败 {}: {e}", self.path.display());
            self.dirty = true;
            cx.notify();
            return Some(new_range);
        }
        self.dirty = false;
        cx.notify();
        Some(new_range)
    }
}
