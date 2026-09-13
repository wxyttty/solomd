//! warm_theme.rs — SoloMD 暖色设计系统 → zorite gpui-editor `SyntaxStyle` 映射。
//!
//! 色值来源：app/src/styles/tokens.css（light 语义 token，品牌橙 #ff9f40 锁定）。
//! GitHub alert 五色在 SoloMD 调色板中无对应项，取语义近似并注释说明。

use gpui::{rgb, Font, Hsla, SharedString};
use zorite_editor::SyntaxStyle;

fn hex(c: u32) -> Hsla {
    rgb(c).into()
}

/// 浅色暖调 SyntaxStyle。mono_family 取宿主主题的等宽字体族。
pub fn warm_light(mono_family: SharedString) -> SyntaxStyle {
    SyntaxStyle {
        // 标记字符（**、~~、[] …）：text-faint，弱化但不消失。
        marker: hex(0xb4b4b4),
        // 行内代码：--md-code / --md-code-bg（tokens.css L59-60）。
        code: hex(0xc0392b),
        code_bg: hex(0xf3efe7),
        // 链接：--md-link = --accent（品牌橙）。
        link: hex(0xff9f40),
        // 标签：--warning 暖金，与标签 pill 的既有观感一致。
        tag: hex(0xd4a017),
        quote: hex(0x6a6a6a), // --md-quote
        // GitHub alert 五类：SoloMD 无原生对应，取语义近似——
        // note=柔和蓝、tip=success 绿、important=紫、warning=暖金、caution=danger 红。
        alert_note: hex(0x5b8def),
        alert_tip: hex(0x38a169),
        alert_important: hex(0x8b5cf6),
        alert_warning: hex(0xd4a017),
        alert_caution: hex(0xe53e3e),
        alert_icons: None,
        rule: hex(0xe9e9e7), // --border
        // <mark>：品牌橙的柔光底。
        mark_bg: hex(0xff9f40).opacity(0.18),
        block_label: None,
        block_label_gen: 0,
        block_ref_count: None,
        popover_bg: hex(0xf7f6f3),    // --bg-elev
        popover_border: hex(0xe9e9e7), // --border
        popover_fg: hex(0x1a1a1a),     // --accent-fg
        popover_hover: hex(0xff9f40).opacity(0.10), // --accent-soft
        popover_divider: hex(0xe9e9e7),
        popover_danger: hex(0xe53e3e), // --danger
        mono: Font {
            family: mono_family,
            ..Default::default()
        },
        property_icon: None,
    }
}
