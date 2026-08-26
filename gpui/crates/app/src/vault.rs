//! vault.rs — 笔记库文件操作（P2 M1：同步 fs，够用为主）。

use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[derive(Clone)]
pub struct Vault {
    pub root: PathBuf,
}

/// 外部变更检测结果。
#[derive(Debug, Default)]
pub struct Changes {
    pub modified: Vec<PathBuf>,
    pub deleted: Vec<PathBuf>,
}

fn walk_notes_rec(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    for e in rd.flatten() {
        let path = e.path();
        let name = e.file_name().to_string_lossy().to_string();
        if name.starts_with('.') || name == "target" || name == "node_modules" {
            continue;
        }
        if path.is_dir() {
            walk_notes_rec(&path, out);
        } else if matches!(
            path.extension().and_then(|x| x.to_str()),
            Some("md") | Some("markdown")
        ) {
            out.push(path);
        }
    }
}

/// 递归扫描 vault 内所有笔记的 mtime（微秒）。
fn scan_mtimes(root: &Path, out: &mut HashMap<PathBuf, u64>) {
    let Ok(rd) = std::fs::read_dir(root) else {
        return;
    };
    for e in rd.flatten() {
        let path = e.path();
        let name = e.file_name().to_string_lossy().to_string();
        if name.starts_with('.') || name == "target" || name == "node_modules" {
            continue;
        }
        if path.is_dir() {
            scan_mtimes(&path, out);
        } else if matches!(
            path.extension().and_then(|x| x.to_str()),
            Some("md") | Some("markdown")
        ) {
            let mtime = e
                .metadata()
                .and_then(|m| m.modified())
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_micros() as u64)
                .unwrap_or(0);
            out.insert(path, mtime);
        }
    }
}

pub struct Entry {
    pub path: PathBuf,
    pub name: String,
    pub is_dir: bool,
}

impl Vault {
    pub fn open(root: PathBuf) -> std::io::Result<Self> {
        std::fs::create_dir_all(&root)?;
        // root 规范化为绝对路径：resolve 对绝对路径直通，重复 resolve 幂等。
        // （曾因相对 root 双重 join 产生 _vault/_vault/ 嵌套、⌘S 写错位置。）
        let root = root.canonicalize().or_else(|_| {
            if root.is_absolute() {
                Ok(root)
            } else {
                std::env::current_dir().map(|cwd| cwd.join(root))
            }
        })?;
        Ok(Self { root })
    }

    pub fn resolve(&self, rel: &Path) -> PathBuf {
        if rel.is_absolute() {
            rel.to_path_buf()
        } else {
            self.root.join(rel)
        }
    }

    /// 列一层目录：子目录在前，仅 .md/.markdown 文件，跳过隐藏与构建产物。
    pub fn list_dir(&self, rel: &Path) -> std::io::Result<Vec<Entry>> {
        let dir = self.resolve(rel);
        let mut entries: Vec<Entry> = std::fs::read_dir(dir)?
            .filter_map(Result::ok)
            .filter_map(|e| {
                let path = e.path();
                let name = e.file_name().to_string_lossy().to_string();
                if name.starts_with('.') || name == "target" || name == "node_modules" {
                    return None;
                }
                let is_dir = path.is_dir();
                let is_note = matches!(
                    path.extension().and_then(|x| x.to_str()),
                    Some("md") | Some("markdown")
                );
                if is_dir || is_note {
                    Some(Entry {
                        path,
                        name,
                        is_dir,
                    })
                } else {
                    None
                }
            })
            .collect();
        entries.sort_by_key(|e| (!e.is_dir, e.name.to_lowercase()));
        Ok(entries)
    }

    pub fn read_note(&self, path: &Path) -> std::io::Result<String> {
        std::fs::read_to_string(self.resolve(path))
    }

    pub fn write_note(&self, path: &Path, content: &str) -> std::io::Result<()> {
        let path = self.resolve(path);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, content)
    }

    /// 递归列出全部笔记（相对 root 的路径，排序稳定）。
    pub fn walk_notes(&self) -> Vec<PathBuf> {
        let mut out = Vec::new();
        walk_notes_rec(&self.root, &mut out);
        out.sort();
        out
    }

    /// 全库 mtime 快照（外部变更检测的基线）。
    pub fn mtimes(&self) -> HashMap<PathBuf, u64> {
        let mut out = HashMap::new();
        scan_mtimes(&self.root, &mut out);
        out
    }

    /// 对比快照：返回外部修改/删除的笔记（不含本次会话写过的文件由调用方过滤）。
    pub fn diff_changes(&self, baseline: &HashMap<PathBuf, u64>) -> Changes {
        let now = self.mtimes();
        let mut changes = Changes::default();
        for (path, m) in &now {
            match baseline.get(path) {
                None => changes.modified.push(path.clone()), // 新文件也算"外部变更"
                Some(old) if old != m => changes.modified.push(path.clone()),
                _ => {}
            }
        }
        for path in baseline.keys() {
            if !now.contains_key(path) {
                changes.deleted.push(path.clone());
            }
        }
        changes
    }

    /// 新建笔记：untitled.md 起步，重名追加序号。
    pub fn create_note(&self, rel_dir: &Path) -> std::io::Result<PathBuf> {
        let base = self.resolve(rel_dir);
        for i in 0.. {
            let name = if i == 0 {
                "untitled.md".to_string()
            } else {
                format!("untitled-{i}.md")
            };
            let path = base.join(name);
            if !path.exists() {
                std::fs::write(&path, "")?;
                return Ok(path);
            }
        }
        unreachable!()
    }
}
