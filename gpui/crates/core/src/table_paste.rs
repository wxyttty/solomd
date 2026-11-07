//! table_paste.rs — 剪贴板表格 → GFM Markdown 表格（上游 `app/src/lib/table-paste.ts` 移植）。
//!
//! Excel / WPS / Numbers / Google Sheets 粘贴时会在 `text/html` 里放
//! `<table>`，同时 `text/plain` 是制表符分隔（TSV）。若不识别，粘贴结果要么
//! 是一张位图（走图片粘贴分支），要么是一堆裸 tab 文本。
//!
//! 上游用正则解析 HTML；这里改手写扫描（`regex-lite` 不支持反向引用
//! `<(th|td)…</\1>`，且手写对脏 HTML 更宽容）：
//!   - 只取第一个 `<table>`；内嵌 `<table>`（布局表格）放弃；
//!   - 支持 colspan / rowspan（行跨度用占位空单元格补齐）；
//!   - 单元格内 `<br>` / 相邻块闭合 → `<br>`（GFM 单元格内换行）；
//!   - 表头对齐取 `align=` / `style="text-align:"`。
//!
//! 拒绝条件（与上游一致）：1×1 单格、全空、TSV 列数不一致、缩进代码里的 tab。

/// 列对齐。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ColumnAlign {
    Left,
    Center,
    Right,
}

/// 剪贴板表格 → Markdown；无可用表格返回 None。
pub fn clipboard_table_to_markdown(html: Option<&str>, plain_text: Option<&str>) -> Option<String> {
    if let Some(h) = html {
        if let Some(md) = html_table_to_markdown(h) {
            return Some(md);
        }
    }
    if let Some(t) = plain_text {
        if let Some(md) = tsv_to_markdown(t) {
            return Some(md);
        }
    }
    None
}

/// 2D 单元格 → GFM 表格。
pub fn format_markdown_table(rows: &[Vec<String>], aligns: &[Option<ColumnAlign>]) -> Option<String> {
    if rows.is_empty() {
        return None;
    }
    let col_count = rows.iter().map(|r| r.len()).max().unwrap_or(0);
    if col_count == 0 {
        return None;
    }
    // 1×1 是散文不是表格。
    if rows.len() == 1 && col_count < 2 {
        return None;
    }
    if !rows.iter().any(|r| r.iter().any(|c| !c.trim().is_empty())) {
        return None;
    }

    let mut norm: Vec<Vec<String>> = rows
        .iter()
        .map(|row| {
            (0..col_count)
                .map(|c| escape_markdown_cell(row.get(c).map(String::as_str).unwrap_or("")))
                .collect()
        })
        .collect();

    // 只有一行（例如从 Excel 复制的单行选区）：补一个空行，GFM 才认它是表格。
    if norm.len() == 1 {
        norm.push(vec![String::new(); col_count]);
    }

    let sep: Vec<&str> = (0..col_count)
        .map(|ix| match aligns.get(ix).copied().flatten() {
            Some(ColumnAlign::Center) => ":---:",
            Some(ColumnAlign::Right) => "---:",
            Some(ColumnAlign::Left) => ":---",
            None => "---",
        })
        .collect();

    let mut lines = Vec::with_capacity(norm.len() + 1);
    lines.push(format!("| {} |", norm[0].join(" | ")));
    lines.push(format!("| {} |", sep.join(" | ")));
    for row in &norm[1..] {
        lines.push(format!("| {} |", row.join(" | ")));
    }
    Some(lines.join("\n"))
}

/// 提取首个 `<table>` 并转换；无表格 / 嵌套表格 / 内容不足返回 None。
pub fn html_table_to_markdown(html: &str) -> Option<String> {
    let open = find_tag(html, "table", 0)?;
    let inner_start = open.tag_end;
    let close = find_close_tag(html, "table", inner_start)?;
    let inner = &html[inner_start..close];
    // 内嵌 `<table>` = 布局表格，放弃（与上游一致）。
    if find_tag(inner, "table", 0).is_some() {
        return None;
    }

    let mut rows: Vec<Vec<String>> = Vec::new();
    let mut aligns: Vec<Option<ColumnAlign>> = Vec::new();
    // 跨行占位：列号 → 剩余行数。
    let mut rowspans: Vec<(usize, usize)> = Vec::new();

    let mut pos = 0usize;
    // 逐行扫描 `<tr>`。
    loop {
        let Some(tr) = find_tag(inner, "tr", pos) else { break };
        let Some(tr_end) = find_close_tag(inner, "tr", tr.tag_end) else { break };
        let tr_inner = &inner[tr.tag_end..tr_end];
        let mut row: Vec<String> = Vec::new();
        let mut col = 0usize;

        // 行首的跨行占位先补齐。
        flush_rowspans(&mut row, &mut col, &mut rowspans);

        let mut cpos = 0usize;
        loop {
            let Some(cell) = find_cell(tr_inner, cpos) else { break };
            let Some(cell_end) = find_cell_close(tr_inner, cell.name, cell.tag_end) else {
                break;
            };
            flush_rowspans(&mut row, &mut col, &mut rowspans);

            let content = clean_cell_html(&tr_inner[cell.tag_end..cell_end]);
            let colspan = attr_number(&cell.attrs, "colspan").clamp(1, 32);
            let rowspan = attr_number(&cell.attrs, "rowspan").clamp(1, 128);
            let align = parse_cell_align(&cell.attrs);

            for c in 0..colspan {
                let target = col + c;
                if aligns.len() <= target {
                    aligns.resize(target + 1, None);
                }
                if aligns[target].is_none() {
                    aligns[target] = align;
                }
                row.push(if c == 0 { content.clone() } else { String::new() });
                if rowspan > 1 {
                    rowspans.push((target, rowspan - 1));
                }
            }
            col += colspan;
            cpos = cell_end + cell.name.len() + 3; // `</td>` 长度
        }
        flush_rowspans(&mut row, &mut col, &mut rowspans);
        if !row.is_empty() {
            rows.push(row);
        }
        pos = tr_end + "</tr>".len();
    }

    format_markdown_table(&rows, &aligns)
}

/// TSV（制表符分隔）→ GFM 表格。
pub fn tsv_to_markdown(plain: &str) -> Option<String> {
    if !plain.contains('\t') {
        return None;
    }
    let normalised = plain.replace("\r\n", "\n").replace('\r', "\n");
    let trimmed = normalised.trim_end_matches('\n');
    let lines: Vec<&str> = trimmed.split('\n').collect();
    if lines.len() < 2 {
        return None;
    }
    // 拒绝缩进代码：tab 只出现在行首（`\treturn 1;`）。
    let has_interior_tab = lines.iter().any(|l| {
        let stripped = l.trim_start_matches('\t');
        stripped.contains('\t')
    });
    if !has_interior_tab {
        return None;
    }
    // 来自 Excel/WPS/Sheets 的选区每一行都至少一个 tab。
    if !lines.iter().all(|l| l.contains('\t')) {
        return None;
    }
    let rows: Vec<Vec<String>> = lines
        .iter()
        .map(|l| l.split('\t').map(|s| s.to_string()).collect())
        .collect();
    let first_cols = rows[0].len();
    if first_cols < 2 {
        return None;
    }
    if !rows.iter().all(|r| r.len() == first_cols) {
        return None;
    }
    format_markdown_table(&rows, &[])
}

// ---------------------------------------------------------------------------
// HTML 扫描辅助
// ---------------------------------------------------------------------------

struct Tag {
    /// `<tag ...>` 之后的位置。
    tag_end: usize,
    /// 属性原文。
    attrs: String,
}

/// 找 `<name`（大小写不敏感，跳过 `</name>`）。
fn find_tag(html: &str, name: &'static str, from: usize) -> Option<Tag> {
    let lower = html.to_ascii_lowercase();
    let needle = format!("<{name}");
    let mut pos = from;
    while let Some(rel) = lower[pos..].find(&needle) {
        let start = pos + rel;
        // 后面必须是空白或 `>`，避免 `<table` 匹配到 `<tablex`。
        let after_name = start + needle.len();
        match lower.as_bytes().get(after_name) {
            Some(b'>') | Some(b' ') | Some(b'\t') | Some(b'\n') | Some(b'\r') | Some(b'/') => {
                let Some(gt) = html[after_name..].find('>') else {
                    return None;
                };
                let tag_end = after_name + gt + 1;
                return Some(Tag {
                    tag_end,
                    attrs: html[after_name..after_name + gt].to_string(),
                });
            }
            _ => pos = after_name,
        }
    }
    None
}

/// 找 `</name>` 的起始位置。
fn find_close_tag(html: &str, name: &str, from: usize) -> Option<usize> {
    let lower = html.to_ascii_lowercase();
    let needle = format!("</{name}");
    lower[from..].find(&needle).map(|rel| from + rel)
}

struct CellTag {
    /// `"td"` 或 `"th"`。
    name: &'static str,
    tag_end: usize,
    attrs: String,
}

fn find_cell(html: &str, from: usize) -> Option<CellTag> {
    let td = find_tag(html, "td", from).map(|t| (t.tag_end, t.attrs, "td"));
    let th = find_tag(html, "th", from).map(|t| (t.tag_end, t.attrs, "th"));
    let pick = match (td, th) {
        (Some(a), Some(b)) => Some(if a.0 <= b.0 { a } else { b }),
        (Some(a), None) => Some(a),
        (None, Some(b)) => Some(b),
        (None, None) => None,
    }?;
    Some(CellTag {
        name: pick.2,
        tag_end: pick.0,
        attrs: pick.1,
    })
}

fn find_cell_close(html: &str, name: &'static str, from: usize) -> Option<usize> {
    find_close_tag(html, name, from)
}

/// 把当前列的跨行占位补成空单元格，并递减剩余行数。
fn flush_rowspans(row: &mut Vec<String>, col: &mut usize, spans: &mut Vec<(usize, usize)>) {
    loop {
        let Some(ix) = spans.iter().position(|(c, _)| *c == *col) else {
            break;
        };
        let (_, remaining) = spans[ix];
        row.push(String::new());
        if remaining <= 1 {
            spans.remove(ix);
        } else {
            spans[ix].1 = remaining - 1;
        }
        *col += 1;
    }
}

fn attr_number(attrs: &str, key: &str) -> usize {
    let lower = attrs.to_ascii_lowercase();
    let Some(pos) = lower.find(key) else { return 1 };
    let rest = &attrs[pos + key.len()..];
    let rest = rest.trim_start();
    let rest = rest.strip_prefix('=').unwrap_or(rest).trim_start();
    let rest = rest.trim_start_matches(['"', '\'']);
    let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
    digits.parse().unwrap_or(1)
}

fn parse_cell_align(attrs: &str) -> Option<ColumnAlign> {
    let lower = attrs.to_ascii_lowercase();
    // `align="right"` / `align=right`
    if let Some(pos) = lower.find("align") {
        let rest = lower[pos + 5..].trim_start();
        let rest = rest.strip_prefix('=').unwrap_or(rest).trim_start();
        let rest = rest.trim_start_matches(['"', '\'']);
        if rest.starts_with("left") {
            return Some(ColumnAlign::Left);
        }
        if rest.starts_with("center") {
            return Some(ColumnAlign::Center);
        }
        if rest.starts_with("right") {
            return Some(ColumnAlign::Right);
        }
    }
    // `style="text-align:right"`
    if let Some(pos) = lower.find("text-align") {
        let rest = lower[pos + 10..].trim_start();
        let rest = rest.strip_prefix(':').unwrap_or(rest).trim_start();
        if rest.starts_with("left") {
            return Some(ColumnAlign::Left);
        }
        if rest.starts_with("center") {
            return Some(ColumnAlign::Center);
        }
        if rest.starts_with("right") {
            return Some(ColumnAlign::Right);
        }
    }
    None
}

/// 单元格 HTML → 纯文本（`<br>` 保成 `<br>`，供 GFM 单元格内换行）。
fn clean_cell_html(raw: &str) -> String {
    // 去注释、style/script/meta/link/xml 块（Excel 会塞这些）。
    let mut text = remove_comments(raw);
    text = remove_element_blocks(&text, "style");
    text = remove_element_blocks(&text, "script");
    text = remove_element_blocks(&text, "xml");
    text = remove_self_closing(&text, "meta");
    text = remove_self_closing(&text, "link");
    text = remove_self_closing(&text, "style");
    // 换行标记先转 \n，再统一成 <br>。
    text = replace_tag_ci(&text, "br", "\n");
    text = normalize_block_breaks(&text);
    text = strip_all_tags(&text);
    let text = decode_entities(&text).replace("\r\n", "\n").replace('\r', "\n");

    let mut lines: Vec<String> = text
        .split('\n')
        .map(|l| collapse_ws(l).trim().to_string())
        .collect();
    while lines.first().is_some_and(|l| l.is_empty()) {
        lines.remove(0);
    }
    while lines.last().is_some_and(|l| l.is_empty()) {
        lines.pop();
    }
    let out: Vec<String> = lines
        .iter()
        .map(|s| s.as_str())
        .collect::<Vec<_>>()
        .into_iter()
        .map(|s| s.to_string())
        .collect();
    out.join("<br>")
}

fn escape_markdown_cell(cell: &str) -> String {
    let normalised = cell.replace("\r\n", "\n").replace('\r', "\n");
    let mut lines: Vec<String> = normalised
        .split('\n')
        .map(|l| l.trim().to_string())
        .collect();
    while lines.first().is_some_and(|l| l.is_empty()) {
        lines.remove(0);
    }
    while lines.last().is_some_and(|l| l.is_empty()) {
        lines.pop();
    }
    lines.join("<br>").replace('|', "\\|")
}

fn remove_comments(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(start) = rest.find("<!--") {
        out.push_str(&rest[..start]);
        match rest[start + 4..].find("-->") {
            Some(end) => rest = &rest[start + 4 + end + 3..],
            None => {
                rest = "";
                break;
            }
        }
    }
    out.push_str(rest);
    out
}

/// 去掉 `<tag ...>…</tag>` 块（大小写不敏感）。
fn remove_element_blocks(s: &str, tag: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s.to_string();
    loop {
        let lower = rest.to_ascii_lowercase();
        let open = format!("<{tag}");
        let Some(start) = lower.find(&open) else { break };
        // 确认是标签边界。
        let after = start + open.len();
        if !matches!(
            lower.as_bytes().get(after),
            Some(b'>') | Some(b' ') | Some(b'\t') | Some(b'\n') | Some(b'\r') | Some(b'/')
        ) {
            // 不是该标签，往下找。
            out.push_str(&rest[..after]);
            rest = rest[after..].to_string();
            continue;
        }
        let close = format!("</{tag}>");
        match lower[after..].find(&close) {
            Some(rel) => {
                out.push_str(&rest[..start]);
                rest = rest[after + rel + close.len()..].to_string();
            }
            None => {
                // 未闭合：整段丢弃（与正则 `[\s\S]*?</style>` 的容错不同，
                // 但未闭合 style 本身就是坏 HTML）。
                out.push_str(&rest[..start]);
                rest.clear();
            }
        }
    }
    out.push_str(&rest);
    out
}

fn remove_self_closing(s: &str, tag: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s.to_string();
    loop {
        let lower = rest.to_ascii_lowercase();
        let open = format!("<{tag}");
        let Some(start) = lower.find(&open) else { break };
        let after = start + open.len();
        if !matches!(
            lower.as_bytes().get(after),
            Some(b'>') | Some(b' ') | Some(b'\t') | Some(b'\n') | Some(b'\r') | Some(b'/')
        ) {
            out.push_str(&rest[..after]);
            rest = rest[after..].to_string();
            continue;
        }
        match lower[after..].find('>') {
            Some(gt) => {
                out.push_str(&rest[..start]);
                rest = rest[after + gt + 1..].to_string();
            }
            None => {
                out.push_str(&rest[..start]);
                rest.clear();
            }
        }
    }
    out.push_str(&rest);
    out
}

/// `<br>` / `<br/>` / `<BR />` → 替换文本。
fn replace_tag_ci(s: &str, tag: &str, with: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s.to_string();
    loop {
        let lower = rest.to_ascii_lowercase();
        let open = format!("<{tag}");
        let Some(start) = lower.find(&open) else { break };
        let after = start + open.len();
        if !matches!(
            lower.as_bytes().get(after),
            Some(b'>') | Some(b' ') | Some(b'\t') | Some(b'\n') | Some(b'\r') | Some(b'/')
        ) {
            out.push_str(&rest[..after]);
            rest = rest[after..].to_string();
            continue;
        }
        match lower[after..].find('>') {
            Some(gt) => {
                out.push_str(&rest[..start]);
                out.push_str(with);
                rest = rest[after + gt + 1..].to_string();
            }
            None => {
                out.push_str(&rest[..start]);
                rest.clear();
            }
        }
    }
    out.push_str(&rest);
    out
}

/// `</p><p>` / `</div><div>` / `</li><li>` 之间插换行。
fn normalize_block_breaks(s: &str) -> String {
    let lower = s.to_ascii_lowercase();
    let mut out = String::with_capacity(s.len());
    let bytes = s.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        if lower.as_bytes()[i] == b'<' {
            // `</(p|div|li|tr)>\s*<(p|div|li)`
            let close_hit = ["</p>", "</div>", "</li>", "</tr>"]
                .iter()
                .find(|c| lower[i..].starts_with(*c));
            if let Some(c) = close_hit {
                let mut j = i + c.len();
                while j < bytes.len() && bytes[j].is_ascii_whitespace() {
                    j += 1;
                }
                let open_hit = ["<p", "<div", "<li"]
                    .iter()
                    .find(|o| lower[j..].starts_with(*o));
                if open_hit.is_some() {
                    out.push_str("\n");
                    i = j;
                    continue;
                }
            }
        }
        // 逐字符推进（保证 UTF-8 边界）。
        let ch_len = s[i..].chars().next().map(char::len_utf8).unwrap_or(1);
        out.push_str(&s[i..i + ch_len]);
        i += ch_len;
    }
    out
}

/// 去掉所有标签（保留文字）。
fn strip_all_tags(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_tag = false;
    for ch in s.chars() {
        match ch {
            '<' => in_tag = true,
            '>' if in_tag => in_tag = false,
            _ if !in_tag => out.push(ch),
            _ => {}
        }
    }
    out
}

/// 折叠连续空白（空格 / tab / nbsp）。
fn collapse_ws(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut prev_ws = false;
    for ch in s.chars() {
        let is_ws = ch == ' ' || ch == '\t' || ch == '\u{00a0}';
        if is_ws {
            if !prev_ws {
                out.push(' ');
            }
            prev_ws = true;
        } else {
            out.push(ch);
            prev_ws = false;
        }
    }
    out
}

fn decode_entities(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(amp) = rest.find('&') {
        out.push_str(&rest[..amp]);
        let after = &rest[amp..];
        let Some(semi) = after.find(';') else {
            out.push_str(after);
            return out;
        };
        let entity = &after[1..semi];
        let replacement = match entity.to_ascii_lowercase().as_str() {
            "nbsp" => Some(' '),
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            "39" => Some('\''),
            other => other
                .strip_prefix("#x")
                .and_then(|h| u32::from_str_radix(h, 16).ok())
                .or_else(|| {
                    other
                        .strip_prefix('#')
                        .and_then(|d| d.parse::<u32>().ok())
                })
                .and_then(char::from_u32),
        };
        match replacement {
            Some(ch) => out.push(ch),
            None => out.push_str(&after[..semi + 1]),
        }
        rest = &after[semi + 1..];
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn excel_html_table_with_alignment() {
        let html = r#"
    <html xmlns:o="urn:schemas-microsoft-com:office:office">
    <head><style>td { mso-number-format:"\@"; }</style></head>
    <body>
      <table>
        <!--StartFragment-->
        <tr><td>姓名</td><td>部门</td><td style="text-align:right">得分</td></tr>
        <tr><td>张三</td><td>研发部</td><td align="right">98</td></tr>
        <tr><td>李四</td><td>产品 | 设计</td><td align="right">95</td></tr>
        <!--EndFragment-->
      </table>
    </body>
    </html>
  "#;
        let md = clipboard_table_to_markdown(
            Some(html),
            Some("姓名\t部门\t得分\n张三\t研发部\t98\n李四\t产品 | 设计\t95"),
        )
        .unwrap();
        assert_eq!(
            md,
            [
                "| 姓名 | 部门 | 得分 |",
                "| --- | --- | ---: |",
                "| 张三 | 研发部 | 98 |",
                "| 李四 | 产品 \\| 设计 | 95 |",
            ]
            .join("\n")
        );
    }

    #[test]
    fn multiline_cells_become_br() {
        let html = r#"
    <table>
      <tr><th>阶段</th><th>说明</th></tr>
      <tr><td>第一阶段</td><td>需求分析<br>原型设计</td></tr>
    </table>
  "#;
        assert_eq!(
            html_table_to_markdown(html).unwrap(),
            [
                "| 阶段 | 说明 |",
                "| --- | --- |",
                "| 第一阶段 | 需求分析<br>原型设计 |",
            ]
            .join("\n")
        );
    }

    #[test]
    fn tsv_fallback() {
        let tsv = "项目\t状态\t负责人\nSoloMD\t进行中\tAlex\nWebDAV\t已完成\tBot\n";
        assert_eq!(
            tsv_to_markdown(tsv).unwrap(),
            [
                "| 项目 | 状态 | 负责人 |",
                "| --- | --- | --- |",
                "| SoloMD | 进行中 | Alex |",
                "| WebDAV | 已完成 | Bot |",
            ]
            .join("\n")
        );
    }

    #[test]
    fn rejects_single_cell_and_indented_code() {
        assert_eq!(
            html_table_to_markdown("<table><tr><td>单格内容</td></tr></table>"),
            None
        );
        assert_eq!(tsv_to_markdown("function foo() {\n\treturn 1;\n\t}"), None);
        assert_eq!(clipboard_table_to_markdown(Some(""), Some("普通文本段落\n第二行")), None);
    }

    #[test]
    fn colspan_and_rowspan() {
        let html = r#"<table>
            <tr><td>a</td><td colspan="2">bc</td></tr>
            <tr><td rowspan="2">x</td><td>y</td><td>z</td></tr>
            <tr><td>y2</td><td>z2</td></tr>
        </table>"#;
        let md = html_table_to_markdown(html).unwrap();
        assert_eq!(
            md,
            [
                "| a | bc |  |",
                "| --- | --- | --- |",
                "| x | y | z |",
                "|  | y2 | z2 |",
            ]
            .join("\n")
        );
    }

    #[test]
    fn nested_layout_table_is_rejected() {
        let html = "<table><tr><td><table><tr><td>x</td><td>y</td></tr></table></td></tr></table>";
        assert_eq!(html_table_to_markdown(html), None);
    }

    #[test]
    fn single_row_of_cells_gets_empty_body_row() {
        let md = html_table_to_markdown("<table><tr><td>a</td><td>b</td></tr></table>").unwrap();
        assert_eq!(md, "| a | b |\n| --- | --- |\n|  |  |");
    }

    #[test]
    fn entities_decoded_in_cells() {
        let md = html_table_to_markdown(
            "<table><tr><td>a&amp;b</td><td>&lt;x&gt;</td></tr></table>",
        )
        .unwrap();
        assert!(md.contains("a&b"));
        assert!(md.contains("<x>"));
    }
}
