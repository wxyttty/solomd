//! math.rs — 数学公式渲染：`$$…$$` 块与行内 `$…$` 的 LaTeX 由 ratex-gpui
//! （RaTeX 引擎，纯 Rust、KaTeX 字体内嵌）排成 `gpui::RenderImage`。
//! 渲染昂贵 → off-thread + 缓存，keyed by LaTeX —— 照抄 `mermaid::MermaidStore`
//! 模式（store 结构移植自 zorite app 的 math.rs）。
//!
//! RaTeX 光栅按 DPR=2 输出、逻辑尺寸回传；raster 是黑字白底，ratex-gpui 的
//! render_latex 已按宿主文字色重着色到透明底，主题切换后新渲染自动跟随
//! （已缓存槽位不追色，重开笔记刷新——与 mermaid 同策略）。

use std::collections::HashMap;
use std::sync::Arc;

use gpui::{Hsla, RenderImage, SharedString};

/// 排版字号（px/em）——显示数学比正文略大。
pub const FONT_SIZE: f32 = 22.0;
/// 光栅化 DPR（按逻辑尺寸显示，2× 保证 Retina 清晰）。
pub const DPR: f32 = 2.0;

/// 一张就绪的公式：位图 + 逻辑（pre-DPR）宽高。
type Image = (Arc<RenderImage>, f32, f32);

/// 公式缓存，keyed by LaTeX 源文。
#[derive(Default)]
pub struct MathStore {
    slots: HashMap<SharedString, Slot>,
}

enum Slot {
    Loading,
    Ready(Image),
    Failed,
}

impl MathStore {
    /// `source` 的排版结果（位图 + 逻辑宽高），未就绪返回 None。
    pub fn get(&self, source: &SharedString) -> Option<Image> {
        match self.slots.get(source) {
            Some(Slot::Ready((image, w, h))) => Some((image.clone(), *w, *h)),
            _ => None,
        }
    }

    /// 认领 `source` 开始渲染；已有槽位（进行中/就绪/失败）返回 false，
    /// 保证同一公式只起一次渲染。
    pub fn begin(&mut self, source: &SharedString) -> bool {
        if self.slots.contains_key(source) {
            return false;
        }
        self.slots.insert(source.clone(), Slot::Loading);
        true
    }

    /// 记录渲染结果（就绪或失败；失败回退显示原始 LaTeX）。
    pub fn finish(&mut self, source: SharedString, result: Option<Image>) {
        let slot = match result {
            Some(img) => Slot::Ready(img),
            None => {
                eprintln!("[math] 渲染失败: {source}");
                Slot::Failed
            }
        };
        self.slots.insert(source, slot);
    }
}

/// off-thread 渲染入口：LaTeX → `(RenderImage, 逻辑宽, 逻辑高)`。
/// 解析/排版/光栅失败一律 None（宿主回退显示源文）。
pub fn render_to_image(source: &str, color: Hsla) -> Option<Image> {
    let rendered = ratex_gpui::render::render_latex(source, FONT_SIZE, DPR, color)?;
    Some((rendered.image, rendered.width, rendered.height))
}
