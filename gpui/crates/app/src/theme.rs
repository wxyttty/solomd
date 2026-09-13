//! theme.rs — SoloMD 暖色设计系统 → zorite gpui-editor `SyntaxStyle` 映射。
//!
//! 色值来源：app/src/styles/tokens.css + main.css（品牌橙 #ff9f40/#ffa54d 锁定）。
//! GitHub alert 五色在 SoloMD 调色板中无对应项，取语义近似并注释说明。

use gpui::{rgb, Font, Hsla, SharedString};
use zorite_editor::SyntaxStyle;

fn hex(c: u32) -> Hsla {
    rgb(c).into()
}

/// 浅色暖调 SyntaxStyle。
pub fn warm_light(mono_family: SharedString) -> SyntaxStyle {
    SyntaxStyle {
        marker: hex(0xb4b4b4),        // --text-faint
        code: hex(0xc0392b),          // --md-code
        code_bg: hex(0xf3efe7),       // --md-code-bg
        link: hex(0xff9f40),          // --md-link = --accent
        tag: hex(0xd4a017),           // --warning
        quote: hex(0x6a6a6a),         // --md-quote
        // GitHub alert 五类：语义近似（note 蓝 / tip 绿 / important 紫 / warning 金 / caution 红）。
        alert_note: hex(0x5b8def),
        alert_tip: hex(0x38a169),
        alert_important: hex(0x8b5cf6),
        alert_warning: hex(0xd4a017),
        alert_caution: hex(0xe53e3e),
        alert_icons: None,
        rule: hex(0xe9e9e7),          // --border
        mark_bg: hex(0xff9f40).opacity(0.18),
        block_label: None,
        block_label_gen: 0,
        block_ref_count: None,
        popover_bg: hex(0xf7f6f3),     // --bg-elev
        popover_border: hex(0xe9e9e7), // --border
        popover_fg: hex(0x1a1a1a),     // --accent-fg
        popover_hover: hex(0xff9f40).opacity(0.10),
        popover_divider: hex(0xe9e9e7),
        popover_danger: hex(0xe53e3e), // --danger
        mono: Font {
            family: mono_family,
            ..Default::default()
        },
        property_icon: None,
    }
}

/// 深色暖调 SyntaxStyle（tokens.css dark 段：bg #1f1e1b / text #e6e1d8 /
/// muted #b8b1a6 / faint #7f776d / code #ff9f9f on #2a2a2a / accent #ffa54d）。
pub fn warm_dark(mono_family: SharedString) -> SyntaxStyle {
    SyntaxStyle {
        marker: hex(0x7f776d),
        code: hex(0xff9f9f),
        code_bg: hex(0x2a2a2a),
        link: hex(0xffa54d),
        tag: hex(0xe0b341),
        quote: hex(0x888888),
        alert_note: hex(0x7aa5f0),
        alert_tip: hex(0x4cb782),
        alert_important: hex(0xa78bfa),
        alert_warning: hex(0xe0b341),
        alert_caution: hex(0xf07168),
        alert_icons: None,
        rule: hex(0x34322d),
        mark_bg: hex(0xffa54d).opacity(0.22),
        block_label: None,
        block_label_gen: 0,
        block_ref_count: None,
        popover_bg: hex(0x23221f),
        popover_border: hex(0x34322d),
        popover_fg: hex(0xe6e1d8),
        popover_hover: hex(0xffa54d).opacity(0.14),
        popover_divider: hex(0x34322d),
        popover_danger: hex(0xf07168),
        mono: Font {
            family: mono_family,
            ..Default::default()
        },
        property_icon: None,
    }
}

/// 按暗色开关取对应 SyntaxStyle。
pub fn warm(dark: bool, mono_family: SharedString) -> SyntaxStyle {
    if dark {
        warm_dark(mono_family)
    } else {
        warm_light(mono_family)
    }
}
