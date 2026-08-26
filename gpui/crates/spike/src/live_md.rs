//! live_md.rs — 基于 tree-sitter-md 的 caret-reveal 装饰决策层（P1 步骤3）。
//!
//! 使用 crate 官方的 `MarkdownParser` 封装：一次 `parse` 得到块树 + 每个
//! `inline` 节点对应的行内树（`inline_tree(&inline_node)`），字节偏移全文档一致。
//!
//! caret-reveal 规则：光标所在行的标记字符（atx_*_marker、emphasis_delimiter、
//! code_span_delimiter）正常显示；其余行 fade_out 隐藏。内容按最近样式化祖先
//! 加粗/斜体/底色/删除线。

use tree_sitter::Node;
use tree_sitter_md::{MarkdownParser, MarkdownTree};

/// 块级 + 行内的联合解析器。
pub struct MdParsers {
    parser: MarkdownParser,
}

impl MdParsers {
    pub fn new() -> Self {
        Self {
            parser: MarkdownParser::default(),
        }
    }

    pub fn parse(&mut self, text: &str) -> Option<MarkdownTree> {
        self.parser.parse(text.as_bytes(), None)
    }
}

/// 内容样式类别（由最近的样式化祖先决定）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ContentStyle {
    Heading,
    Strong,
    Emphasis,
    CodeSpan,
    Strikethrough,
}

impl ContentStyle {
    fn from_kind(kind: &str) -> Option<Self> {
        Some(match kind {
            k if k.starts_with("atx_heading") || k.starts_with("setext_heading") => Self::Heading,
            "strong_emphasis" => Self::Strong,
            "emphasis" => Self::Emphasis,
            "code_span" => Self::CodeSpan,
            "strikethrough" => Self::Strikethrough,
            _ => return None,
        })
    }
}

fn is_marker_kind(kind: &str) -> bool {
    // 块级：atx_h1_marker…/block_quote_marker；行内：*_delimiter。
    kind.ends_with("_marker") || kind.ends_with("_delimiter")
}

struct Collector<'a> {
    src: &'a str,
    cursor: usize,
    mdtree: &'a MarkdownTree,
    items: Vec<(std::ops::Range<usize>, bool, Option<ContentStyle>)>,
}

impl<'a> Collector<'a> {
    fn line_of(&self, byte: usize) -> usize {
        self.src[..byte.min(self.src.len())].matches('\n').count()
    }

    /// 标记字符的 reveal 判定放宽到"光标在该标记所在行"。
    fn reveal_at(&self, start: usize, end: usize) -> bool {
        let ls = self.line_of(start);
        let le = self.line_of(end.saturating_sub(1).max(start));
        let cl = self.line_of(self.cursor);
        cl >= ls && cl <= le
    }

    fn walk(&mut self, node: Node, styles: &mut Vec<ContentStyle>, can_inline: bool) {
        let kind = node.kind();
        let abs = node.start_byte()..node.end_byte();

        if is_marker_kind(kind) && !abs.is_empty() {
            let reveal = self.reveal_at(abs.start, abs.end);
            self.items.push((abs.clone(), reveal, None));
        }

        let pushed = if let Some(style) = ContentStyle::from_kind(kind) {
            styles.push(style);
            true
        } else {
            false
        };

        if kind == "inline" {
            // 行内子树来自 mdtree 的行内层；其根节点也叫 "inline"，
            // 用 can_inline 守卫防止无限递归。
            if can_inline {
                if let Some(itree) = self.mdtree.inline_tree(&node) {
                    let root = itree.root_node();
                    let n = root.child_count();
                    for i in 0..n {
                        if let Some(child) = root.child(i as u32) {
                            self.walk(child, styles, false);
                        }
                    }
                }
            }
        } else {
            let n = node.child_count();
            for i in 0..n {
                if let Some(child) = node.child(i as u32) {
                    self.walk(child, styles, can_inline);
                }
            }
        }

        // 内容样式作用于整个节点范围；标记字符已被单独隐藏，重叠无害。
        if pushed && !abs.is_empty() {
            let style = *styles.last().expect("just pushed");
            self.items.push((abs, false, Some(style)));
        }

        if pushed {
            styles.pop();
        }
    }
}

/// caret-reveal 装饰计算结果。
pub struct Decorations {
    /// (range, hidden)：hidden=true 时 fade_out 隐藏。
    pub markers: Vec<(std::ops::Range<usize>, bool)>,
    /// (range, style) 内容样式。
    pub styles: Vec<(std::ops::Range<usize>, ContentStyle)>,
}

pub fn compute(parsers: &mut MdParsers, text: &str, cursor: usize) -> Decorations {
    let Some(mdtree) = parsers.parse(text) else {
        return Decorations { markers: vec![], styles: vec![] };
    };
    let mut collector = Collector {
        src: text,
        cursor,
        mdtree: &mdtree,
        items: Vec::new(),
    };
    collector.walk(mdtree.block_tree().root_node(), &mut Vec::new(), true);

    let mut markers = Vec::new();
    let mut styles_out = Vec::new();
    for (range, reveal, style) in collector.items {
        match style {
            None => markers.push((range, !reveal)),
            Some(s) => styles_out.push((range, s)),
        }
    }
    Decorations {
        markers,
        styles: styles_out,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "# Title One\n\nplain **bold** and `code` and ~~gone~~.\n\n```rust\nfn x() {}\n```\n";

    #[test]
    fn inline_and_block_nodes_present() {
        let mut p = MdParsers::new();
        let d = compute(&mut p, SAMPLE, 0);
        let kinds_dump = format!("{:?}\n{:?}", d.markers, d.styles);
        println!("{kinds_dump}");

        // 块级标记：标题 # 被识别为 marker
        assert!(d.markers.iter().any(|(r, _)| SAMPLE[r.clone()].trim() == "#"));
        // 行内命名节点：strong_emphasis / code_span / strikethrough 产生内容样式
        assert!(d.styles.iter().any(|(_, s)| *s == ContentStyle::Strong));
        assert!(d.styles.iter().any(|(_, s)| *s == ContentStyle::CodeSpan));
        assert!(d.styles.iter().any(|(_, s)| *s == ContentStyle::Strikethrough));
        assert!(d.styles.iter().any(|(_, s)| *s == ContentStyle::Heading));
        // 行内 delimiter 是 marker（每个 * 单独成区间）
        assert!(d
            .markers
            .iter()
            .any(|(r, _)| &SAMPLE[r.clone()] == "*"));
    }

    #[test]
    fn caret_reveal_toggles_by_line() {
        let mut p = MdParsers::new();
        // 光标在第 3 行（**bold** 所在段）→ 该行 delimiter 不隐藏
        let cursor_on_para = SAMPLE.find("bold").unwrap();
        let d_on = compute(&mut p, SAMPLE, cursor_on_para);
        let hidden_on = d_on
            .markers
            .iter()
            .filter(|(r, hidden)| *hidden && SAMPLE[r.clone()].contains('*'))
            .count();
        // 光标移到第 1 行标题上 → 段落行的 ** 全部隐藏
        let d_off = compute(&mut p, SAMPLE, 2);
        let hidden_off = d_off
            .markers
            .iter()
            .filter(|(r, hidden)| *hidden && SAMPLE[r.clone()].contains('*'))
            .count();
        assert_eq!(hidden_on, 0, "聚焦行不应隐藏 **");
        assert!(hidden_off >= 4, "非聚焦行应隐藏全部 ** 字符");
    }
}
