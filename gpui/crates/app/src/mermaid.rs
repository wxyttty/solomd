//! spike_mermaid.rs — mermaid 围栏 provider 的宿主侧（精简自 zorite src/mermaid.rs）。
//!
//! 管线：mermaid-rs-renderer（zed 同款纯 Rust）→ SVG → gpui SvgRenderer 栅格化
//! → RenderImage。渲染昂贵：状态机（Loading/Ready/Failed）+ off-thread，
//! provider 闭包只读缓存。主题色固定为 SoloMD 暖色浅色系。

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

use gpui::{RenderImage, SharedString, SvgRenderer};

type Image = (Arc<RenderImage>, f32, f32);

/// 纹理→显示比例：1.0 = 按自然 SVG 尺寸栅格化并原尺寸显示（2× 屏自然高清）。
pub const RASTER_SCALE: f32 = 1.0;

#[derive(Default)]
pub struct MermaidStore {
    slots: HashMap<SharedString, Slot>,
}

enum Slot {
    Loading,
    Ready(Image),
    Failed,
}

impl MermaidStore {
    /// provider 入口：只读缓存。
    pub fn get(&self, source: &SharedString) -> Option<Image> {
        match self.slots.get(source) {
            Some(Slot::Ready((image, w, h))) => Some((image.clone(), *w, *h)),
            _ => None,
        }
    }

    #[allow(dead_code)] // 编辑器侧 fallback 分支待接（失败时回退源码显示）
    pub fn failed(&self, source: &SharedString) -> bool {
        matches!(self.slots.get(source), Some(Slot::Failed))
    }

    /// 占坑；返回 false 表示已有槽位（幂等）。
    pub fn begin(&mut self, source: &SharedString) -> bool {
        if self.slots.contains_key(source) {
            return false;
        }
        self.slots.insert(source.clone(), Slot::Loading);
        true
    }

    pub fn finish(&mut self, source: SharedString, result: Result<Arc<RenderImage>, String>) {
        let slot = match result {
            Ok(image) => {
                let size = image.size(0);
                Slot::Ready((image, size.width.0 as f32, size.height.0 as f32))
            }
            Err(e) => {
                eprintln!("[mermaid] render failed: {e}");
                Slot::Failed
            }
        };
        self.slots.insert(source, slot);
    }
}

#[allow(dead_code)]
pub type SharedMermaidStore = Rc<RefCell<MermaidStore>>;

/// SoloMD 暖色浅色主题（hex 字符串，mermaid theme 字段是不透明 #rrggbb）。
fn warm_theme() -> mermaid_rs_renderer::Theme {
    let mut t = mermaid_rs_renderer::Theme::modern();
    t.background = "#ffffff".into();
    t.text_color = "#1a1a1a".into();
    t.primary_color = "#f3efe7".into(); // --md-code-bg 作节点填充
    t.primary_text_color = "#1a1a1a".into();
    t.primary_border_color = "#ff9f40".into(); // 品牌橙描边——白板图一眼 SoloMD
    t.secondary_color = "#f7f6f3".into();
    t.tertiary_color = "#f0eee8".into();
    t.line_color = "#787774".into(); // --text-muted
    t.edge_label_background = "#ffffff".into();
    t.cluster_background = "#f7f6f3".into();
    t.cluster_border = "#e9e9e7".into();
    t
}

/// mermaid → SVG → RenderImage。纯 CPU，可离线程。
pub fn render_to_image(
    source: &str,
    svg: &SvgRenderer,
    scale: f32,
) -> Result<Arc<RenderImage>, String> {
    let options = mermaid_rs_renderer::RenderOptions {
        theme: warm_theme(),
        layout: mermaid_rs_renderer::LayoutConfig::default(),
    };
    let svg_string =
        mermaid_rs_renderer::render_with_options(source, options).map_err(|e| e.to_string())?;
    svg.render_single_frame(svg_string.as_bytes(), scale)
        .map_err(|e| e.to_string())
}
