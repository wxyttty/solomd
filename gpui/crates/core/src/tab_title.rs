//! tab_title.rs — 标签标题取文档首个非空行（上游 `app/src/lib/tab-title.ts` 移植）。//!
//! 用途：SoloMD 被第三方工具当外部编辑器调用时，常收到 UUID/哈希文件名
//! （`a8f3c91b-4d2e.md`）——标签上显示 `# 会议纪要` 比显示文件名有用得多。
//! 设置项 `Settings.tab_title_from_first_line` 控制开关（默认开）。
//!
//! 纯函数、零 IO，便于单测；解析顺序与上游一致：
//!   1. YAML frontmatter 的 `title:` 优先（若有）；
//!   2. 否则取首个非空正文行，剥掉 Markdown 块级/行内标记；
//!   3. 超过 48 字符截断加省略号。

/// 标题最大字符数（上游 MAX_TITLE_CHARS）。
const MAX_TITLE_CHARS: usize = 48;
/// 只扫前 8KB（与上游一致，避免大文件为了标题整篇扫描）。
const SCAN_BYTES: usize = 8192;

/// 从文档内容推导标题；无法推导时返回空串（调用方回退到文件名）。
pub fn extract_first_line_title(content: &str) -> String {
    if content.is_empty() {
        return String::new();
    }
    // 字节切片要落在字符边界上：min(SCAN_BYTES, len) 向下对齐到边界。
    let mut end = content.len().min(SCAN_BYTES);
    while end > 0 && !content.is_char_boundary(end) {
        end -= 1;
    }
    let head = &content[..end];

    let lines: Vec<&str> = head.split('\n').map(|l| l.trim_end_matches('\r')).collect();
    let mut i = 0usize;
    let mut fm_title = String::new();

    // 1. YAML frontmatter：`---` 开头则找闭合的 `---` / `...`，顺手取 title。
    if lines.first().map(|l| l.trim()) == Some("---") {
        i = 1;
        let mut closed = false;
        while i < lines.len() {
            let t = lines[i].trim();
            if t == "---" || t == "..." {
                closed = true;
                i += 1;
                break;
            }
            if fm_title.is_empty() {
                if let Some(v) = yaml_title_value(t) {
                    fm_title = v;
                }
            }
            i += 1;
        }
        // 未闭合：当作普通正文，从第一行重新扫（上游同款容错）。
        if !closed {
            i = 1;
        }
    }

    // 2. 首个非空正文行。
    while i < lines.len() {
        let raw = lines[i].trim();
        i += 1;
        if raw.is_empty() {
            continue;
        }
        if is_fence_line(raw) || is_thematic_break(raw) {
            continue;
        }
        let cleaned = strip_markdown_prefixes(raw);
        if cleaned.is_empty() {
            continue;
        }
        return truncate(&cleaned);
    }

    // 3. 正文全空时回退 frontmatter title。
    if !fm_title.is_empty() {
        return truncate(&fm_title);
    }
    String::new()
}

/// `title: 值` / `title: "值"` → 值（去首尾引号）。大小写不敏感（上游 `/i`）。
fn yaml_title_value(line: &str) -> Option<String> {
    let lower = line.to_ascii_lowercase();
    let rest = lower.strip_prefix("title")?;
    let rest_orig = &line[line.len() - rest.len()..];
    let after_colon = rest_orig.strip_prefix(':')?.trim();
    if after_colon.is_empty() {
        return None;
    }
    Some(
        after_colon
            .trim_matches(|c| c == '"' || c == '\'')
            .trim()
            .to_string(),
    )
}

fn is_fence_line(s: &str) -> bool {
    s.starts_with("```") || s.starts_with("~~~")
}

fn is_thematic_break(s: &str) -> bool {
    let t = s.trim();
    t == "---" || t == "***" || t == "___"
}

/// 剥掉块级（引用/标题/列表/任务）与行内（图片/链接/wiki/HTML/强调）标记。
fn strip_markdown_prefixes(raw: &str) -> String {
    let mut s = raw;
    // 引用前缀（可多重）。
    loop {
        let t = s.trim_start();
        if let Some(rest) = t.strip_prefix('>') {
            s = rest;
        } else {
            s = t;
            break;
        }
    }
    // ATX 标题。
    let bytes = s.as_bytes();
    let mut hashes = 0usize;
    while hashes < bytes.len() && bytes[hashes] == b'#' && hashes < 6 {
        hashes += 1;
    }
    if hashes > 0 && bytes.len() > hashes && bytes[hashes] == b' ' {
        s = &s[hashes + 1..];
    }
    // 尾部闭合的 ` ###`。
    let trimmed = s.trim_end();
    if trimmed.ends_with('#') {
        if let Some(cut) = trimmed.trim_end_matches('#').len().checked_sub(0) {
            let candidate = &trimmed[..cut];
            if candidate.ends_with(' ') {
                s = candidate.trim_end();
            }
        }
    }
    // 列表 / 任务标记。
    s = strip_list_marker(s);

    // 行内标记。
    let mut out = replace_image_and_link(s);
    if let Some(rest) = strip_html_tags(&out) {
        out = rest;
    }
    for m in ["**", "__", "==", "~~", "`"] {
        out = out.replace(m, "");
    }
    out.trim_matches('*').trim().to_string()
}

/// `- x` / `* x` / `+ x` / `1. x` / `1) x` → `x`；任务项 `- [ ] x` → `x`。
fn strip_list_marker(s: &str) -> &str {
    let t = s.trim_start();
    let mut rest = None;
    for m in ["- ", "* ", "+ "] {
        if let Some(r) = t.strip_prefix(m) {
            rest = Some(r);
            break;
        }
    }
    if rest.is_none() {
        let digits = t.bytes().take_while(u8::is_ascii_digit).count();
        if digits > 0 && t.len() > digits + 1 {
            let b = t.as_bytes();
            if (b[digits] == b'.' || b[digits] == b')') && b.get(digits + 1) == Some(&b' ') {
                rest = Some(&t[digits + 2..]);
            }
        }
    }
    let Some(r) = rest else { return t };
    // 任务框 `[ ] ` / `[x] `。
    let r = r.trim_start();
    if r.len() >= 3 && r.starts_with('[') {
        let close = r.as_bytes()[2];
        if close == b']' && (&r[1..2] == " " || &r[1..2] == "x" || &r[1..2] == "X") {
            return r[3..].trim_start();
        }
    }
    r
}

/// `![alt](url)` → `alt`；`[label](url)` → `label`；`[[t|alias]]` → alias/t。
fn replace_image_and_link(s: &str) -> String {
    // 按 `](` 定位，反查该行的 `[`（支持前缀 `!` = 图片）。
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(bs) = rest.find("](") {
        let before = &rest[..bs];
        let Some(open) = before.rfind('[') else {
            break;
        };
        let after_open = &rest[bs + 2..];
        let Some(close) = after_open.find(')') else {
            break;
        };
        // `![` 的 `[` 前一个字节是 `!`，一并丢弃。
        let bracket_start = if open > 0 && before.as_bytes()[open - 1] == b'!' {
            open - 1
        } else {
            open
        };
        out.push_str(&rest[..bracket_start]);
        out.push_str(&before[open + 1..]);
        rest = &after_open[close + 1..];
    }
    out.push_str(rest);

    // `[[target|alias]]` / `[[target#heading]]` / `[[target]]`。
    let mut wiki = String::with_capacity(out.len());
    let mut r3 = out.as_str();
    while let Some(bs) = r3.find("[[") {
        match r3[bs + 2..].find("]]") {
            Some(rel) => {
                let inner = &r3[bs + 2..bs + 2 + rel];
                let label = inner
                    .split_once('|')
                    .map(|(_, alias)| alias)
                    .unwrap_or_else(|| inner.split_once('#').map(|(t, _)| t).unwrap_or(inner));
                wiki.push_str(&r3[..bs]);
                wiki.push_str(label);
                r3 = &r3[bs + 2 + rel + 2..];
            }
            None => break,
        }
    }
    wiki.push_str(r3);
    wiki
}

/// 去掉 HTML 标签（`<b>` 等）；返回 None 表示无标签（省一次分配）。
fn strip_html_tags(s: &str) -> Option<String> {
    if !s.contains('<') {
        return None;
    }
    let mut out = String::with_capacity(s.len());
    let mut depth = false;
    for ch in s.chars() {
        match ch {
            '<' => depth = true,
            '>' if depth => depth = false,
            _ if !depth => out.push(ch),
            _ => {}
        }
    }
    Some(out)
}

fn truncate(s: &str) -> String {
    let count = s.chars().count();
    if count <= MAX_TITLE_CHARS {
        return s.to_string();
    }
    let cut: String = s.chars().take(MAX_TITLE_CHARS).collect();
    format!("{}…", cut.trim_end())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn heading_is_stripped() {
        assert_eq!(extract_first_line_title("# 会议纪要\n\n正文"), "会议纪要");
    }

    #[test]
    fn skips_fence_marker_lines_only() {
        // 上游语义：只跳围栏标记行，围栏内容会被当作首行（与 tab-title.ts 一致）。
        assert_eq!(
            extract_first_line_title("\n\n```rust\nfn main() {}\n```\n实际标题"),
            "fn main() {}"
        );
        assert_eq!(extract_first_line_title("```\n```\n真正标题"), "真正标题");
    }

    #[test]
    fn frontmatter_title_wins_after_empty_body() {
        assert_eq!(
            extract_first_line_title("---\ntitle: \"周报\"\n---\n\n"),
            "周报"
        );
    }

    #[test]
    fn frontmatter_skipped_and_body_used() {
        assert_eq!(
            extract_first_line_title("---\ntags: [x]\n---\n## 正文标题"),
            "正文标题"
        );
    }

    #[test]
    fn list_and_task_markers_stripped() {
        assert_eq!(extract_first_line_title("- [ ] 待办事项"), "待办事项");
        assert_eq!(extract_first_line_title("1. 第一步"), "第一步");
        assert_eq!(extract_first_line_title("> 引用文字"), "引用文字");
    }

    #[test]
    fn inline_markup_stripped() {
        assert_eq!(extract_first_line_title("**加粗** 与 `代码`"), "加粗 与 代码");
        assert_eq!(extract_first_line_title("[链接](https://x) 标题"), "链接 标题");
        assert_eq!(extract_first_line_title("[[目标|别名]] 说明"), "别名 说明");
    }

    #[test]
    fn long_title_truncated_with_ellipsis() {
        let long = "很".repeat(60);
        let got = extract_first_line_title(&long);
        assert_eq!(got.chars().count(), MAX_TITLE_CHARS + 1);
        assert!(got.ends_with('…'));
    }

    #[test]
    fn empty_and_blank_return_empty() {
        assert_eq!(extract_first_line_title(""), "");
        assert_eq!(extract_first_line_title("\n\n   \n"), "");
    }

    #[test]
    fn utf8_boundary_safe_on_huge_cjk_doc() {
        // 8KB 截断点可能落在多字节字符中间——必须不 panic。
        let doc = "中".repeat(10_000);
        let got = extract_first_line_title(&doc);
        assert!(!got.is_empty());
    }
}
