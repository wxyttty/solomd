//! math_edit.rs — 结构化公式编辑会话（ratex_gpui::MathEditor）。
//!
//! gpui-editor 在光标进入 `$…$` / `$$…$$` 时发 `EditorEvent::EditMath`，宿主把
//! `MathEditor` 塞进编辑器的 editing 槽位（块级留全宽空隙、行内原位覆盖，编辑器
//! 自管布局），blur / 方向键越界时提交：`to_latex()` → `replace_range` 回写
//! （记录式、可撤销）→ 落盘 → 重跑该公式渲染。集成模式移植自 zorite app/math.rs；
//! 子模块身份使其可直取 NotePanel 私有字段。简化项：RTL 出向翻转、
//! 「公式上方无内容」守卫未移植（中文优先场景）。

use std::ops::Range;

use gpui::{px, AppContext as _, ClipboardItem, Context, Entity, SharedString, Window};
use gpui_component::ActiveTheme as _;
use ratex_gpui::MathAlign;

use super::NotePanel;

/// 正文字号（mod.rs render 根节点 `.text_size(px(16.))`），行内公式编辑器用它。
const TEXT_SIZE: f32 = 16.0;

pub(super) struct MathEditSession {
    pub(super) editor: Entity<ratex_gpui::MathEditor>,
    pub(super) inline: bool,
    /// 失焦提交（点外面）。持活即可，逻辑在闭包里。
    pub(super) _blur_sub: gpui::Subscription,
    /// 方向键越界流回正文 + 右键复制。持活即可。
    pub(super) _nav_sub: gpui::Subscription,
}

fn to_ratex_align(a: zorite_editor::MathAlign) -> MathAlign {
    match a {
        zorite_editor::MathAlign::Left => MathAlign::Left,
        zorite_editor::MathAlign::Center => MathAlign::Center,
        zorite_editor::MathAlign::Right => MathAlign::Right,
    }
}

fn to_editor_align(a: MathAlign) -> zorite_editor::MathAlign {
    match a {
        MathAlign::Left => zorite_editor::MathAlign::Left,
        MathAlign::Center => zorite_editor::MathAlign::Center,
        MathAlign::Right => zorite_editor::MathAlign::Right,
    }
}

impl NotePanel {
    /// 光标进入公式：先提交打开中的会话，再按 LaTeX 在当前缓冲里重定位该块
    /// （提交可能移动过字节偏移），起 MathEditor 并交给编辑器。
    pub(super) fn open_math_edit(
        &mut self,
        range: Range<usize>,
        latex: SharedString,
        at_end: bool,
        inline: bool,
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
        let found = if inline {
            self.state.read(cx).find_inline_math(&latex, range.start)
        } else {
            self.state.read(cx).find_math_block(&latex, range.start)
        };
        let Some(range) = found else { return };
        // 行内按正文渲染；块级按显示 em + 已存的 `<!-- math:… -->` 对齐。
        let (font_size, align) = if inline {
            (TEXT_SIZE, MathAlign::default())
        } else {
            (
                crate::math::FONT_SIZE,
                to_ratex_align(self.state.read(cx).math_align(range.start)),
            )
        };
        let theme = {
            let t = cx.theme();
            ratex_gpui::MathTheme {
                fg: t.foreground,
                muted: t.muted_foreground,
                panel: t.popover,
                border: t.border,
                accent: t.accent,
                accent_bg: t.accent,
            }
        };
        let editor = cx.new(|cx| {
            ratex_gpui::MathEditor::from_latex(&latex, font_size, at_end, align, theme, cx)
        });
        let focus = editor.read(cx).focus_handle();
        if inline {
            // 编辑器光栅的 PAD 按行内字号烘焙，显示光栅按块 em 烘焙后缩放——
            // 补差避免进入编辑时公式位移（zorite #77；tip 的 4 参签名）。
            let pad_delta = ratex_gpui::render::PAD * (TEXT_SIZE / crate::math::FONT_SIZE - 1.0);
            self.state.update(cx, |e, cx| {
                e.set_editing_inline(
                    range,
                    editor.clone().into(),
                    gpui::point(px(pad_delta), px(pad_delta)),
                    cx,
                )
            });
        } else {
            let height = self
                .math
                .borrow()
                .get(&SharedString::from(latex.as_str()))
                .map_or(px(56.0), |(_, _, h)| px(h + 16.0));
            self.state.update(cx, |e, cx| {
                e.set_editing_block(range, editor.clone().into(), height, cx)
            });
        }
        // 失焦提交（点外面）；身份守卫防旧 blur 提交新会话。
        let weak = cx.entity().downgrade();
        let editor_id = editor.entity_id();
        let blur_sub = window.on_focus_out(&focus, cx, move |_ev, _window, cx| {
            weak.update(cx, |this, cx| {
                if this
                    .math_edit
                    .as_ref()
                    .is_some_and(|m| m.editor.entity_id() == editor_id)
                {
                    this.commit_math_edit(cx);
                }
            })
            .ok();
        });
        // 方向键越界 → 光标流回正文；右键 → v1 直接复制 LaTeX（完整菜单后置）。
        let nav_sub = cx.subscribe_in(
            &editor,
            window,
            |this, editor, ev: &ratex_gpui::MathNav, window, cx| match ev {
                ratex_gpui::MathNav::Exit { after } => this.exit_math_edit(*after, window, cx),
                ratex_gpui::MathNav::ContextMenu { .. } => {
                    let latex = editor.read(cx).to_latex();
                    cx.write_to_clipboard(ClipboardItem::new_string(latex));
                }
            },
        );
        self.math_edit = Some(MathEditSession {
            editor,
            inline,
            _blur_sub: blur_sub,
            _nav_sub: nav_sub,
        });
        window.focus(&focus, cx);
        cx.notify();
    }

    /// 提交：序列化 LaTeX → splice 回缓冲（replace_range，可撤销）→ 落盘 →
    /// 重跑该公式渲染。返回公式的新字节范围（供出向定位光标）。
    pub(super) fn commit_math_edit(&mut self, cx: &mut Context<Self>) -> Option<Range<usize>> {
        let edit = self.math_edit.take()?;
        let latex = edit.editor.read(cx).to_latex();
        let new_range = if edit.inline {
            let range = self
                .state
                .update(cx, |e, cx| e.end_editing_inline(cx))?;
            if !self.state.read(cx).is_inline_math_range(&range) {
                cx.notify();
                return None;
            }
            let content = self.state.read(cx).text().to_string();
            // 同行 `$$…$$` 对（展示数学）保留双定界符；普通行内单 `$`。
            let dollars = if content[range.clone()].starts_with("$$") {
                "$$"
            } else {
                "$"
            };
            let pair = format!("{dollars}{latex}{dollars}");
            // `$$…$$` 是展示数学、不与文字混排：同行有别的文字时把该行拆成
            // 文字/公式/文字三行（zorite #54）；单 `$` 原位。
            let line_start = content[..range.start].rfind('\n').map_or(0, |p| p + 1);
            let line_end = content[range.end..]
                .find('\n')
                .map_or(content.len(), |p| range.end + p);
            let before = content[line_start..range.start].trim_end();
            let after = content[range.end..line_end].trim_start();
            let (edit_range, replacement, pair_start) =
                if dollars == "$$" && (!before.is_empty() || !after.is_empty()) {
                    let mut repl = String::new();
                    if !before.is_empty() {
                        repl.push_str(before);
                        repl.push('\n');
                    }
                    let ps = repl.len();
                    repl.push_str(&pair);
                    if !after.is_empty() {
                        repl.push('\n');
                        repl.push_str(after);
                    }
                    (line_start..line_end, repl, ps)
                } else {
                    (range.clone(), pair.clone(), 0)
                };
            let nr = edit_range.start + pair_start..edit_range.start + pair_start + pair.len();
            self.state
                .update(cx, |e, cx| e.replace_range(edit_range, &replacement, cx));
            nr
        } else {
            let align = to_editor_align(edit.editor.read(cx).align());
            let range = self.state.update(cx, |e, cx| e.end_editing_block(cx))?;
            // 只在范围仍是 `$$` 块头时拼接——过期/漂移的范围会插错位置损坏文档。
            if !self.state.read(cx).is_math_block_range(&range) {
                cx.notify();
                return None;
            }
            // 单行源保单行形；多行 LaTeX 总要围栏。
            let was_one_line = !self.state.read(cx).text()[range.clone()].contains('\n');
            let block = if was_one_line && !latex.contains('\n') {
                format!("$${latex}$$")
            } else {
                format!("$$\n{latex}\n$$")
            };
            // 对齐标记折进同一次记录式编辑（替换块 + 其上既有 marker 行）。
            let (full_range, prefix) = self.state.read(cx).math_marker_edit(range, align);
            let replacement = format!("{prefix}{block}");
            let block_start = full_range.start + prefix.len();
            let nr = block_start..block_start + block.len();
            // replace_range 停光标在拼接尾；blur 提交时点击已落好光标——按拼接
            // 增量平移保留之，若仍落块内则跳到块后一行。
            let old_caret = self.state.read(cx).cursor();
            self.state
                .update(cx, |e, cx| e.replace_range(full_range.clone(), &replacement, cx));
            let len = self.state.read(cx).text().len();
            let delta = replacement.len() as isize - (full_range.end - full_range.start) as isize;
            let caret = if old_caret >= full_range.end {
                (old_caret as isize + delta).max(0) as usize
            } else {
                old_caret
            };
            let caret = if caret >= nr.start && caret <= nr.end {
                nr.end + 1
            } else {
                caret
            };
            self.state.update(cx, |e, cx| e.set_cursor(caret.min(len), cx));
            nr
        };
        let new = self.state.read(cx).text().to_string();
        if let Err(e) = self.vault.write_note(&self.path, &new) {
            eprintln!("[math] 回写失败 {}: {e}", self.path.display());
            return Some(new_range); // 缓冲已改；盘上失败不阻断会话收尾
        }
        self.dirty = false;
        self.warm_math(SharedString::from(latex), cx);
        cx.notify();
        Some(new_range)
    }

    /// 方向键越过公式边界：提交并把正文光标落到公式旁（键盘路径出去）。
    /// 行内落在 span 同侧旁；块级交给 `exit_math`（编辑器自管落到邻行）。
    fn exit_math_edit(&mut self, after: bool, window: &mut Window, cx: &mut Context<Self>) {
        let inline = self.math_edit.as_ref().is_some_and(|m| m.inline);
        // `$$` 块上方无内容（文档开头，或只有自己的对齐 marker）时向上出向会把
        // 正文光标落在开头围栏行上、露出原始源码——留在公式里（zorite 同款守卫）。
        if !after
            && !inline
            && let Some(range) = self.state.read(cx).editing_block_range()
        {
            let text = self.state.read(cx).text().to_string();
            let above = text[..range.start.min(text.len())].trim_end_matches('\n');
            if above.is_empty() || (!above.contains('\n') && above.starts_with("<!-- math:")) {
                return;
            }
        }
        if let Some(block) = self.commit_math_edit(cx) {
            self.state.update(cx, |e, cx| {
                if inline {
                    e.focus(window, cx);
                    e.set_cursor(if after { block.end } else { block.start }, cx);
                } else {
                    e.exit_math(block, after, window, cx);
                }
            });
        }
    }

    /// 单条公式热身（提交后新 LaTeX 立刻排版；开窗全量热身在 mod.rs）。
    fn warm_math(&mut self, source: SharedString, cx: &mut Context<Self>) {
        let color = cx.theme().foreground;
        {
            let mut store = self.math.borrow_mut();
            store.set_color(color);
            if !store.begin(&source) {
                return; // 已在渲染 / 就绪 / 失败
            }
        }
        let store = self.math.clone();
        cx.spawn(async move |this, cx| {
            let src = source.to_string();
            let result = cx
                .background_executor()
                .spawn(async move { crate::math::render_to_image(&src, color) })
                .await;
            store.borrow_mut().finish(source, result);
            let _ = this.update(cx, |_, cx| {
                cx.notify();
                cx.refresh_windows();
            });
        })
        .detach();
    }
}
