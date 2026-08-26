//! spike_images.rs — 块级图片 provider 的宿主侧（精简自 zorite src/images.rs）。
//!
//! 职责：解析 `![](src)` 的 src → 读文件 → `image` crate 解码 → 缩到上限
//! → RGBA→BGRA（gpui RenderImage 存直通非预乘 BGRA）→ RenderImage。
//! 同步解码 + HashMap 缓存；真实实现应 off-thread（zorite 模式），spike 先证通路。

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;

use gpui::RenderImage;
use image::Frame;

/// 单边解码上限：防解压炸弹，同时作为缩图目标。
const MAX_IMAGE_EDGE: u32 = 1024;

pub struct ImageStore {
    map: HashMap<String, Arc<RenderImage>>,
    root: PathBuf,
}

impl ImageStore {
    /// root：相对 src 的解析基准（spike 用工作区根）。
    pub fn new(root: PathBuf) -> Self {
        Self {
            map: HashMap::new(),
            root,
        }
    }

    /// provider 入口：命中缓存或同步解码。失败返回 None（行保持源码显示）。
    pub fn get(&mut self, src: &str) -> Option<Arc<RenderImage>> {
        if let Some(img) = self.map.get(src) {
            return Some(img.clone());
        }
        // http(s) 与空 src 不处理（spike 无网络管线）。
        if src.starts_with("http://") || src.starts_with("https://") || src.is_empty() {
            return None;
        }
        let path = self.resolve(src)?;
        let img = decode_scaled(&path)?;
        self.map.insert(src.to_string(), img.clone());
        Some(img)
    }

    fn resolve(&self, src: &str) -> Option<PathBuf> {
        let p = Path::new(src);
        if p.is_absolute() {
            Some(p.to_path_buf())
        } else {
            // 去掉可能的 ./ 前缀后拼到根
            Some(self.root.join(src.trim_start_matches("./")))
        }
    }
}

/// 便捷封装：Rc<RefCell<ImageStore>> 的 provider 闭包形态（off-thread 版本启用时使用）。
#[allow(dead_code)]
pub type SharedImageStore = Rc<RefCell<ImageStore>>;

fn decode_scaled(path: &Path) -> Option<Arc<RenderImage>> {
    let mut limits = image::Limits::default();
    // 单张解码上限 512MB：拒绝病态头部，放过真实照片。
    limits.max_alloc = Some(512 * 1024 * 1024);
    let mut reader = image::ImageReader::open(path).ok()?.with_guessed_format().ok()?;
    reader.limits(limits);
    reader.decode().ok()
        .map(|img| {
            let (w, h) = (img.width(), img.height());
            let small = if w.max(h) > MAX_IMAGE_EDGE {
                let scale = MAX_IMAGE_EDGE as f32 / w.max(h) as f32;
                img.thumbnail(((w as f32 * scale) as u32).max(1), ((h as f32 * scale) as u32).max(1))
            } else {
                img
            };
            let mut rgba = small.into_rgba8();
            // RenderImage 持有非预乘 BGRA；解码产物是 RGBA，交换 R/B。
            for px in rgba.chunks_exact_mut(4) {
                px.swap(0, 2);
            }
            Arc::new(RenderImage::new(vec![Frame::new(rgba)]))
        })
}
