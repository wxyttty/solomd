//! file_ops.rs — 文件树操作：新建/删除 + 移动/重命名 + 全库链接重写 + 目录/扩展名枚举。
//!
//! 从上游 commands.rs（4.12.0/5.0.0）去 Tauri 壳移植：`#[tauri::command]` 包装删除、
//! 内部逻辑逐字节保留（含 `<stem>.assets/` 随迁、子树防自移、围栏感知的
//! 行级链接重写、隐藏文件/扩展名枚举）。供 FileTreePanel 调用。

use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

pub fn fs_dir_exists(path: String) -> bool {
    Path::new(&path).is_dir()
}

/// 新建文件（上游 v3.0 file-tree edit ops）。拒绝覆盖已存在文件，
/// 父目录缺失时自动创建。空文件由调用方决定内容（上游传 ""）。
pub fn fs_create_file(path: String, content: Option<String>) -> Result<(), String> {
    let p = Path::new(&path);
    if p.exists() {
        return Err(format!("already exists: {path}"));
    }
    if let Some(parent) = p.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent).map_err(|e| format!("mkdir failed: {e}"))?;
        }
    }
    fs::write(p, content.unwrap_or_default().as_bytes())
        .map_err(|e| format!("create failed: {e}"))
}

/// 新建目录。拒绝覆盖已存在路径。
pub fn fs_create_dir(path: String) -> Result<(), String> {
    let p = Path::new(&path);
    if p.exists() {
        return Err(format!("already exists: {path}"));
    }
    fs::create_dir_all(p).map_err(|e| format!("mkdir failed: {e}"))
}

/// 删除文件/目录（上游 #112）。桌面端优先进系统废纸篓（Finder 废纸篓 /
/// Windows 回收站 / XDG trash），误删可恢复；trash 服务失败（如文件系统
/// 不支持）才回退永久删除，保证任何环境都能删。幂等：路径已不存在
/// 直接成功。
pub fn fs_delete(path: String) -> Result<(), String> {
    let p = Path::new(&path);
    if !p.exists() {
        return Ok(()); // idempotent — already gone is fine
    }
    // 桌面端才进废纸篓（移动端无用户可见 trash）。
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    {
        if trash::delete(p).is_ok() {
            return Ok(());
        }
    }
    if p.is_dir() {
        fs::remove_dir_all(p).map_err(|e| format!("delete dir failed: {e}"))
    } else {
        fs::remove_file(p).map_err(|e| format!("delete file failed: {e}"))
    }
}

pub fn fs_rename(from: String, to: String) -> Result<(), String> {
    let from_p = Path::new(&from);
    let to_p = Path::new(&to);
    if !from_p.exists() {
        return Err(format!("source missing: {from}"));
    }
    if to_p.exists() {
        return Err(format!("target already exists: {to}"));
    }

    // v4.3.5 — per-file `.assets/` follow-along. If a sibling directory
    // named `<from-stem>.assets/` exists next to the file we're renaming,
    // move it alongside so the per-file attachment layout survives the
    // rename. `_assets/` (the shared mode) is deliberately untouched —
    // it's shared across every md in that directory.
    //
    // We compute the assets-rename target up front, attempt the primary
    // rename first, then the assets rename, then the link rewrite.
    // Any step beyond the primary that fails is logged but non-fatal:
    // the user's file rename succeeded; we don't want to revert it just
    // because the assets folder rename hit a permission edge case.
    let from_assets = sibling_assets_dir(from_p);
    let to_assets = sibling_assets_dir(to_p);
    let stems_differ = from_assets
        .as_ref()
        .zip(to_assets.as_ref())
        .map(|(a, b)| {
            a.file_name() != b.file_name()
        })
        .unwrap_or(false);

    fs::rename(from_p, to_p).map_err(|e| format!("rename failed: {e}"))?;

    if let (Some(fa), Some(ta)) = (from_assets, to_assets) {
        if fa.is_dir() && !ta.exists() {
            if let Err(e) = fs::rename(&fa, &ta) {
                eprintln!("[fs_rename] assets folder rename failed: {e}");
            } else if stems_differ && is_markdown_path(to_p) {
                // Stem changed → relative refs inside the body now point at
                // a folder that no longer exists. Rewrite them in place.
                if let Err(e) = rewrite_assets_refs(to_p, &fa, &ta) {
                    eprintln!("[fs_rename] body rewrite failed: {e}");
                }
            }
        }
    }
    Ok(())
}

/// Given a path like `/a/b/foo.md`, return `/a/b/foo.assets`. Returns None
/// if the path has no stem (e.g. `/`, an empty string).
fn sibling_assets_dir(p: &Path) -> Option<std::path::PathBuf> {
    let stem = p.file_stem()?.to_string_lossy().to_string();
    if stem.is_empty() {
        return None;
    }
    let parent = p.parent()?;
    Some(parent.join(format!("{stem}.assets")))
}

fn is_markdown_path(p: &Path) -> bool {
    matches!(
        p.extension().and_then(|e| e.to_str()).map(|s| s.to_ascii_lowercase()).as_deref(),
        Some("md") | Some("markdown") | Some("mdown") | Some("mkd")
    )
}

/// Rewrite `<old_stem>.assets/` references inside the file body to
/// `<new_stem>.assets/`. Reads as UTF-8 (which markdown effectively is for
/// our purposes — link paths are always ASCII-safe), does a literal
/// substring replace, and writes back. Best-effort: if the file isn't
/// readable as UTF-8 or has no matches, the original bytes are preserved.
fn rewrite_assets_refs(file: &Path, old_assets: &Path, new_assets: &Path) -> Result<(), String> {
    let old_name = old_assets
        .file_name()
        .and_then(|s| s.to_str())
        .ok_or_else(|| "old assets name missing".to_string())?;
    let new_name = new_assets
        .file_name()
        .and_then(|s| s.to_str())
        .ok_or_else(|| "new assets name missing".to_string())?;
    let bytes = fs::read(file).map_err(|e| format!("read for rewrite: {e}"))?;
    let body = match std::str::from_utf8(&bytes) {
        Ok(s) => s,
        Err(_) => return Ok(()), // non-UTF-8 body, skip rewrite
    };
    // Match the assets folder followed by `/` so we don't accidentally
    // rewrite occurrences inside prose (`foo.assets` mentioned in text).
    let old_pat = format!("{old_name}/");
    let new_pat = format!("{new_name}/");
    if !body.contains(&old_pat) {
        return Ok(());
    }
    let rewritten = body.replace(&old_pat, &new_pat);
    fs::write(file, rewritten).map_err(|e| format!("write back: {e}"))
}

// ---------------------------------------------------------------------------
// Moving files and folders inside the tree (#290 + #267).
//
// `fs_rename` above is already a filesystem move, but it only ever moves
// within ONE directory, so two things a real move breaks never came up:
//
//   1. **Cross-device.** A vault can span a mount point (a symlinked
//      subfolder on an external disk, a network share). `fs::rename` returns
//      EXDEV there and nothing happens. We fall back to copy-then-remove,
//      and only remove once the copy is complete, so a failure halfway
//      leaves the original intact.
//
//   2. **Relative links.** A note carries its body to the new folder, and
//      every relative `](…)` target in that body was written from the OLD
//      folder. `![](_assets/a.png)` resolves to nothing one level down —
//      "I moved a note and all its images broke" is the bug report this
//      exists to prevent. We re-express those targets from the new location.
//
// The link rewrite is deliberately **lexical**: it never hits the disk to
// resolve a target, it only adds or cancels leading `../` segments. So
// percent-encoding (`My%20file.png`), targets whose file doesn't exist,
// and case-insensitive filesystems all survive untouched — we never
// re-spell what the user typed, we only re-anchor it.
//
// Targets that point INSIDE the thing being moved are left alone: their
// relationship to the note didn't change. That set includes the per-file
// `<stem>.assets/` folder, which travels along with the note.
// ---------------------------------------------------------------------------

pub fn fs_move(from: String, to: String) -> Result<(), String> {
    fs_move_inner(from, to)
}

pub fn fs_move_inner(from: String, to: String) -> Result<(), String> {
    let from_p = Path::new(&from);
    let to_p = Path::new(&to);
    if !from_p.exists() {
        return Err(format!("source missing: {from}"));
    }
    let to_parent = to_p
        .parent()
        .ok_or_else(|| "destination has no parent folder".to_string())?;
    if !to_parent.is_dir() {
        return Err(format!("destination folder missing: {}", to_parent.display()));
    }
    if to_p.exists() {
        return Err(format!("target already exists: {to}"));
    }
    // Dropping a folder onto itself or onto one of its own descendants would
    // move a directory inside its own subtree. The OS refuses that with a
    // localized errno; catching it here lets the UI say something useful,
    // and lets the tree reject the drop before it ever starts.
    if from_p.is_dir() && is_self_or_descendant(from_p, to_parent) {
        return Err("a folder cannot be moved inside itself".to_string());
    }

    // Same per-file `.assets/` follow-along as `fs_rename`. On a move the
    // stem doesn't change, so the folder just travels to the new parent and
    // the `<stem>.assets/…` links inside the body keep resolving.
    let from_assets = sibling_assets_dir(from_p);
    let to_assets = sibling_assets_dir(to_p);

    move_tree(from_p, to_p)?;

    let mut moved_roots: Vec<Vec<String>> = vec![abs_segments(from_p)];
    if let (Some(fa), Some(ta)) = (from_assets, to_assets) {
        if fa.is_dir() && !ta.exists() {
            match move_tree(&fa, &ta) {
                // The assets folder moved too, so links into it must NOT be
                // rewritten — record it as part of the moved set.
                Ok(()) => moved_roots.push(abs_segments(&fa)),
                Err(e) => eprintln!("[fs_move] assets folder move failed: {e}"),
            }
        }
    }

    relink_moved_tree(to_p, from_p, &moved_roots);
    Ok(())
}

/// True when `inner` is `outer` itself or sits underneath it. `inner` is the
/// destination's *parent* (the destination itself doesn't exist yet), so it
/// canonicalises cleanly; symlinked vault subfolders compare by their real
/// location, which is what the OS will actually do.
fn is_self_or_descendant(outer: &Path, inner: &Path) -> bool {
    let c_outer = fs::canonicalize(outer).unwrap_or_else(|_| outer.to_path_buf());
    let c_inner = fs::canonicalize(inner).unwrap_or_else(|_| inner.to_path_buf());
    c_inner.starts_with(&c_outer)
}

/// `fs::rename`, falling back to copy-then-remove when source and destination
/// live on different filesystems (EXDEV = 18 on unix, ERROR_NOT_SAME_DEVICE =
/// 17 on Windows). Any other error is reported as-is rather than silently
/// retried as a copy — a permission failure should stay a permission failure.
fn move_tree(from: &Path, to: &Path) -> Result<(), String> {
    match fs::rename(from, to) {
        Ok(()) => Ok(()),
        Err(e) => {
            if !matches!(e.raw_os_error(), Some(18) | Some(17)) {
                return Err(format!("move failed: {e}"));
            }
            copy_tree(from, to).map_err(|e| format!("cross-device copy failed: {e}"))?;
            let removed = if from.is_dir() {
                fs::remove_dir_all(from)
            } else {
                fs::remove_file(from)
            };
            removed.map_err(|e| {
                format!("copied to the new location, but the original could not be removed: {e}")
            })
        }
    }
}

fn copy_tree(from: &Path, to: &Path) -> std::io::Result<()> {
    if from.is_dir() {
        fs::create_dir_all(to)?;
        for entry in fs::read_dir(from)? {
            let entry = entry?;
            copy_tree(&entry.path(), &to.join(entry.file_name()))?;
        }
    } else {
        if let Some(parent) = to.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::copy(from, to)?;
    }
    Ok(())
}

/// An absolute path reduced to its `Normal` components. Prefix and root
/// components are dropped: every comparison here is between two paths under
/// the same vault, so the shared root contributes nothing, and dropping it
/// makes Windows `C:\a\b` and unix `/a/b` behave identically.
pub fn abs_segments(p: &Path) -> Vec<String> {
    p.components()
        .filter_map(|c| match c {
            std::path::Component::Normal(s) => Some(s.to_string_lossy().to_string()),
            _ => None,
        })
        .collect()
}

/// Resolve a relative link target against a directory, purely lexically.
/// `None` when the target climbs above the filesystem root — we leave those
/// alone rather than guess.
pub fn lexical_resolve(base: &[String], target: &str) -> Option<Vec<String>> {
    let mut out = base.to_vec();
    for seg in target.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                out.pop()?;
            }
            s => out.push(s.to_string()),
        }
    }
    Some(out)
}

/// Express the absolute `target` relative to the absolute `base` directory.
pub fn lexical_relative(base: &[String], target: &[String]) -> String {
    let common = base
        .iter()
        .zip(target.iter())
        .take_while(|(a, b)| a == b)
        .count();
    let mut segs: Vec<String> = vec!["..".to_string(); base.len() - common];
    segs.extend(target[common..].iter().cloned());
    if segs.is_empty() {
        ".".to_string()
    } else {
        segs.join("/")
    }
}

/// A target we must not touch: an anchor, an absolute path, or anything with
/// a URL scheme. The scheme test also catches a Windows drive letter (`C:\…`).
fn is_external_target(t: &str) -> bool {
    if t.is_empty() || t.starts_with('#') || t.starts_with('/') || t.starts_with('\\') {
        return true;
    }
    match t.find(':') {
        None => false,
        Some(i) => {
            let scheme = &t[..i];
            !scheme.is_empty()
                && scheme.starts_with(|c: char| c.is_ascii_alphabetic())
                && scheme
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '+' || c == '-' || c == '.')
        }
    }
}

/// Re-anchor one link target. Returns `None` when nothing should change.
fn shift_target(
    t: &str,
    old_dir: &[String],
    new_dir: &[String],
    moved_roots: &[Vec<String>],
) -> Option<String> {
    if is_external_target(t) {
        return None;
    }
    // `file.md#heading` / `file.md?x=1` — only the path part is a location.
    let cut = t.find(['#', '?']).unwrap_or(t.len());
    let (path, suffix) = t.split_at(cut);
    if path.is_empty() {
        return None;
    }
    let resolved = lexical_resolve(old_dir, path)?;
    // Pointing into something that moved along with us: the relationship is
    // unchanged, so the written target is still correct.
    if moved_roots.iter().any(|r| resolved.starts_with(r)) {
        return None;
    }
    let shifted = format!("{}{}", lexical_relative(new_dir, &resolved), suffix);
    if shifted == t {
        None
    } else {
        Some(shifted)
    }
}

fn inline_link_re() -> &'static regex_lite::Regex {
    static RE: std::sync::OnceLock<regex_lite::Regex> = std::sync::OnceLock::new();
    RE.get_or_init(|| regex_lite::Regex::new(r"\]\(([^)\n]*)\)").unwrap())
}

fn ref_def_re() -> &'static regex_lite::Regex {
    static RE: std::sync::OnceLock<regex_lite::Regex> = std::sync::OnceLock::new();
    RE.get_or_init(|| regex_lite::Regex::new(r"^[ \t]*\[[^\]\n]+\]:[ \t]*(\S+)").unwrap())
}

fn html_attr_re() -> &'static regex_lite::Regex {
    static RE: std::sync::OnceLock<regex_lite::Regex> = std::sync::OnceLock::new();
    RE.get_or_init(|| regex_lite::Regex::new(r#"(?:src|href)[ \t]*=[ \t]*"([^"\n]*)""#).unwrap())
}

/// Inside a `](…)` payload, isolate the destination from an optional title
/// (`path "title"`) and an optional `<>` wrapper. Returns its byte range.
fn destination_range(payload: &str) -> Option<(usize, usize)> {
    let lead = payload.len() - payload.trim_start().len();
    let rest = &payload[lead..];
    if let Some(stripped) = rest.strip_prefix('<') {
        let close = stripped.find('>')?;
        return Some((lead + 1, lead + 1 + close));
    }
    let end = rest
        .find(|c: char| c.is_whitespace())
        .unwrap_or(rest.len());
    if end == 0 {
        None
    } else {
        Some((lead, lead + end))
    }
}

/// Rewrite every relative link target on ONE line. Matches inside an inline
/// code span are skipped — an odd number of backticks before the match means
/// we're inside one, and rewriting sample code in prose would be a bug.
fn rewrite_line(
    line: &str,
    old_dir: &[String],
    new_dir: &[String],
    moved_roots: &[Vec<String>],
) -> Option<String> {
    let mut edits: Vec<(usize, usize, String)> = Vec::new();

    let consider = |start: usize, end: usize, edits: &mut Vec<(usize, usize, String)>| {
        if line[..start].matches('`').count() % 2 == 1 {
            return;
        }
        if let Some(next) = shift_target(&line[start..end], old_dir, new_dir, moved_roots) {
            edits.push((start, end, next));
        }
    };

    for c in inline_link_re().captures_iter(line) {
        let payload = c.get(1).unwrap();
        if let Some((s, e)) = destination_range(payload.as_str()) {
            consider(payload.start() + s, payload.start() + e, &mut edits);
        }
    }
    for re in [ref_def_re(), html_attr_re()] {
        for c in re.captures_iter(line) {
            let m = c.get(1).unwrap();
            consider(m.start(), m.end(), &mut edits);
        }
    }
    if edits.is_empty() {
        return None;
    }

    edits.sort_by_key(|(s, _, _)| *s);
    let mut out = String::with_capacity(line.len());
    let mut cursor = 0usize;
    for (s, e, replacement) in edits {
        if s < cursor {
            continue; // overlapping match (an href inside a markdown link) — first wins
        }
        out.push_str(&line[cursor..s]);
        out.push_str(&replacement);
        cursor = e;
    }
    out.push_str(&line[cursor..]);
    Some(out)
}

/// Rewrite a whole note body. Fenced code blocks are passed through verbatim.
pub fn rewrite_links_after_move(
    body: &str,
    old_dir: &[String],
    new_dir: &[String],
    moved_roots: &[Vec<String>],
) -> Option<String> {
    let mut out = String::with_capacity(body.len());
    let mut in_fence = false;
    let mut changed = false;
    for line in body.split_inclusive('\n') {
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            in_fence = !in_fence;
            out.push_str(line);
            continue;
        }
        if in_fence {
            out.push_str(line);
            continue;
        }
        match rewrite_line(line, old_dir, new_dir, moved_roots) {
            Some(next) => {
                changed = true;
                out.push_str(&next);
            }
            None => out.push_str(line),
        }
    }
    if changed {
        Some(out)
    } else {
        None
    }
}

/// Walk everything that just moved and re-anchor the relative links in each
/// markdown file. Non-UTF-8 bodies are skipped rather than transcoded: a
/// GBK-encoded note round-trips through the editor, not through here.
fn relink_moved_tree(new_root: &Path, old_root: &Path, moved_roots: &[Vec<String>]) {
    let files: Vec<std::path::PathBuf> = if new_root.is_dir() {
        walkdir::WalkDir::new(new_root)
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().is_file() && is_markdown_path(e.path()))
            .map(|e| e.path().to_path_buf())
            .collect()
    } else if is_markdown_path(new_root) {
        vec![new_root.to_path_buf()]
    } else {
        Vec::new()
    };

    for file in files {
        // Where this exact file used to live. For a single-file move the
        // relative part is empty and the old path is just `old_root`.
        let old_file = match file.strip_prefix(new_root) {
            Ok(rel) if !rel.as_os_str().is_empty() => old_root.join(rel),
            _ => old_root.to_path_buf(),
        };
        let (Some(od), Some(nd)) = (old_file.parent(), file.parent()) else {
            continue;
        };
        let old_dir = abs_segments(od);
        let new_dir = abs_segments(nd);
        if old_dir == new_dir {
            continue;
        }
        let Ok(bytes) = fs::read(&file) else { continue };
        let Ok(body) = std::str::from_utf8(&bytes) else {
            continue;
        };
        if let Some(next) = rewrite_links_after_move(body, &old_dir, &new_dir, moved_roots) {
            if let Err(e) = fs::write(&file, next) {
                eprintln!("[fs_move] link rewrite write-back failed for {}: {e}", file.display());
            }
        }
    }
}

/// Every folder under `root`, as vault-relative slash-separated paths, for the
/// "Move to…" picker.
///
/// The tree itself only knows about folders the user has expanded, and a
/// picker limited to those is useless for the case it exists to serve: filing
/// a note into a folder you haven't visited today. Build junk and attachment
/// folders are pruned along with their subtrees — nobody files a note into
/// `_assets`. Hidden folders follow the Explorer's own "show hidden files"
/// setting, except `.git`, which is always pruned: walking an entire object
/// store to fill a picker is pure cost, and a note filed in there would be
/// invisible to the app anyway.

pub fn fs_list_dirs_inner(root: String, show_hidden: bool) -> Result<Vec<String>, String> {
    const CAP: usize = 20_000;
    const SKIP: &[&str] = &["node_modules", "target", "dist", "_assets", ".git"];
    let root_p = Path::new(&root);
    if !root_p.is_dir() {
        return Err(format!("not a folder: {root}"));
    }
    let mut out: Vec<String> = Vec::new();
    for entry in walkdir::WalkDir::new(root_p)
        .into_iter()
        .filter_entry(|e| {
            if e.depth() == 0 {
                return true;
            }
            let name = e.file_name().to_string_lossy().to_string();
            (show_hidden || !name.starts_with('.'))
                && !SKIP.contains(&name.as_str())
                && !name.ends_with(".assets")
        })
        .filter_map(|e| e.ok())
    {
        if entry.depth() == 0 || !entry.file_type().is_dir() {
            continue;
        }
        if let Ok(rel) = entry.path().strip_prefix(root_p) {
            out.push(rel.to_string_lossy().replace('\\', "/"));
        }
        if out.len() >= CAP {
            break;
        }
    }
    out.sort();
    Ok(out)
}

/// One extension present in the vault, with how many files carry it.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ExtensionCount {
    /// Lower-cased, no leading dot. Empty string = files with no extension.
    pub ext: String,
    pub count: usize,
}

/// Every file extension in the vault, most common first (#282 — "只想显示该
/// 文件夹下面的 txt 或者是 md 文档").
///
/// The picker offers what the vault actually contains rather than a canned
/// list: a vault of `.md` + `.canvas` and one of `.txt` + `.org` should not
/// both be shown the same twenty checkboxes.

pub fn fs_list_extensions_inner(
    root: String,
    show_hidden: bool,
) -> Result<Vec<ExtensionCount>, String> {
    let root_p = Path::new(&root);
    if !root_p.is_dir() {
        return Err(format!("not a folder: {root}"));
    }
    let mut counts: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    for entry in filtered_walk(root_p, show_hidden) {
        if !entry.file_type().is_file() {
            continue;
        }
        *counts.entry(entry_extension(entry.path())).or_insert(0) += 1;
    }
    let mut out: Vec<ExtensionCount> = counts
        .into_iter()
        .map(|(ext, count)| ExtensionCount { ext, count })
        .collect();
    // Most common first, alphabetical within a tie — a stable order matters
    // more than the exact one, because the list is a checkbox column.
    out.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| a.ext.cmp(&b.ext)));
    Ok(out)
}

/// Vault-relative paths of every directory whose SUBTREE holds at least one
/// file with one of `exts`.
///
/// This exists because the tree is lazy: a folder the user hasn't expanded has
/// no loaded children, so the frontend cannot tell whether hiding it would
/// hide a match. Filtering to `.txt` and still being shown every folder until
/// you open it is the difference between a filter and a suggestion. One walk
/// per filter change answers it for the whole vault.

pub fn fs_dirs_with_extensions_inner(
    root: String,
    exts: Vec<String>,
    show_hidden: bool,
) -> Result<Vec<String>, String> {
    let root_p = Path::new(&root);
    if !root_p.is_dir() {
        return Err(format!("not a folder: {root}"));
    }
    let wanted: std::collections::HashSet<String> =
        exts.into_iter().map(|e| e.trim_start_matches('.').to_ascii_lowercase()).collect();
    let mut hits: std::collections::HashSet<String> = std::collections::HashSet::new();
    for entry in filtered_walk(root_p, show_hidden) {
        if !entry.file_type().is_file() || !wanted.contains(&entry_extension(entry.path())) {
            continue;
        }
        // Every ancestor up to (but not including) the root now has a match.
        let mut dir = entry.path().parent();
        while let Some(d) = dir {
            if d == root_p {
                break;
            }
            match d.strip_prefix(root_p) {
                Ok(rel) => {
                    let key = rel.to_string_lossy().replace('\\', "/");
                    // Already recorded: so are all of its ancestors.
                    if !hits.insert(key) {
                        break;
                    }
                }
                Err(_) => break,
            }
            dir = d.parent();
        }
    }
    let mut out: Vec<String> = hits.into_iter().collect();
    out.sort();
    Ok(out)
}

fn filtered_walk(root: &Path, show_hidden: bool) -> impl Iterator<Item = walkdir::DirEntry> {
    const SKIP: &[&str] = &["node_modules", "target", "dist", ".git"];
    walkdir::WalkDir::new(root)
        .into_iter()
        .filter_entry(move |e| {
            if e.depth() == 0 {
                return true;
            }
            let name = e.file_name().to_string_lossy().to_string();
            (show_hidden || !name.starts_with('.')) && !SKIP.contains(&name.as_str())
        })
        .filter_map(|e| e.ok())
        .take(200_000)
}

fn entry_extension(p: &Path) -> String {
    p.extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .unwrap_or_default()
}

#[cfg(test)]
mod create_delete_tests {
    use super::*;

    #[test]
    fn create_file_and_dir_refuse_to_clobber() {
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("a.md");
        assert!(fs_create_file(f.to_string_lossy().to_string(), Some("x".into())).is_ok());
        // 已存在：拒绝覆盖而不是静默改写。
        assert!(fs_create_file(f.to_string_lossy().to_string(), Some("y".into())).is_err());
        assert_eq!(std::fs::read_to_string(&f).unwrap(), "x");

        let d = dir.path().join("sub");
        assert!(fs_create_dir(d.to_string_lossy().to_string()).is_ok());
        assert!(fs_create_dir(d.to_string_lossy().to_string()).is_err());
    }

    #[test]
    fn create_file_makes_missing_parents() {
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("a/b/c.md");
        assert!(fs_create_file(f.to_string_lossy().to_string(), None).is_ok());
        assert!(f.is_file());
    }

    #[test]
    fn delete_is_idempotent_and_removes_trees() {
        let dir = tempfile::tempdir().unwrap();
        let sub = dir.path().join("sub");
        std::fs::create_dir(&sub).unwrap();
        std::fs::write(sub.join("n.md"), "n").unwrap();

        // 桌面端 trash 生效时文件进了废纸篓（同样不可见）；两种路径下
        // 都应报告成功且路径消失。
        assert!(fs_delete(sub.to_string_lossy().to_string()).is_ok());
        assert!(!sub.exists());
        // 幂等：已不存在直接成功。
        assert!(fs_delete(sub.to_string_lossy().to_string()).is_ok());
    }
}
