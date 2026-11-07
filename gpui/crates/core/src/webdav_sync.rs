//! #458 — WebDAV 笔记库同步（坚果云 / Nextcloud / ownCloud / AList / OpenList /
//! Synology WebDAV 等）。上游 app/src-tauri/src/webdav_sync.rs 去 Tauri 壳移植。
//!
//! Uses pure HTTP/WebDAV (`PROPFIND`, `MKCOL`, `GET`, `PUT`) over `reqwest`
//! with Basic Auth so it works across macOS, Windows, Linux, iOS, and Android
//! without needing `libgit2`.

use reqwest::Method;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::Duration;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebdavSyncResult {
    pub uploaded: usize,
    pub downloaded: usize,
    pub message: String,
}

const VAULT_MANIFEST_NAME: &str = "solomd-vault.json";
const VAULT_PROBE_AAD: &str = "encryption.probe";
const VAULT_PROBE_PLAIN: &[u8] = b"SoloMD probe v1";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CloudVaultManifest {
    pub version: u8,
    pub salt: String,
    pub kdf: crate::crypto::KdfParams,
    pub probe: String,
}

fn create_vault_manifest(passphrase: &str) -> Result<(CloudVaultManifest, [u8; 32]), String> {
    let pass = passphrase.trim();
    if pass.is_empty() {
        return Err("请先设置加密口令 (Encryption passphrase is required)".into());
    }
    let salt = crate::crypto::random_bytes(16);
    let kdf = crate::crypto::KdfParams::default();
    let key = crate::crypto::derive_key(pass, &salt, &kdf)?;
    let probe_bytes = crate::crypto::encrypt_bytes(&key, VAULT_PROBE_AAD, VAULT_PROBE_PLAIN)?;
    let manifest = CloudVaultManifest {
        version: 1,
        salt: crate::crypto::hex_encode(&salt),
        kdf,
        probe: crate::crypto::hex_encode(&probe_bytes),
    };
    Ok((manifest, key))
}

fn unlock_vault_manifest(
    manifest: &CloudVaultManifest,
    passphrase: &str,
) -> Result<[u8; 32], String> {
    let pass = passphrase.trim();
    if pass.is_empty() {
        return Err("需要输入加密口令才能解密同步 (Encryption passphrase is required)".into());
    }
    let salt = crate::crypto::hex_decode(&manifest.salt)?;
    let key = crate::crypto::derive_key(pass, &salt, &manifest.kdf)?;
    let probe_bytes = crate::crypto::hex_decode(&manifest.probe)?;
    let decrypted = crate::crypto::decrypt_bytes(&key, VAULT_PROBE_AAD, &probe_bytes)
        .map_err(|_| "加密口令不正确，无法解密该网盘库 (Incorrect encryption passphrase)".to_string())?;
    if decrypted != VAULT_PROBE_PLAIN {
        return Err("加密口令校验失败 (Encryption probe mismatch)".into());
    }
    Ok(key)
}

fn build_client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(45))
        .user_agent("SoloMD-WebDAV/5.0")
        .build()
        .map_err(|e| format!("Failed to create HTTP client: {e}"))
}

fn normalize_base_url(url: &str) -> String {
    url.trim().trim_end_matches('/').to_string()
}

fn normalize_remote_path(remote_path: &str) -> String {
    let trimmed = remote_path.trim().replace('\\', "/");
    let stripped = trimmed.trim_matches('/');
    if stripped.is_empty() {
        String::new()
    } else {
        format!("/{stripped}")
    }
}

fn percent_encode_segment(seg: &str) -> String {
    let mut out = String::with_capacity(seg.len());
    for &b in seg.as_bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let (Some(hi), Some(lo)) = (
                (bytes[i + 1] as char).to_digit(16),
                (bytes[i + 2] as char).to_digit(16),
            ) {
                out.push(((hi << 4) | lo) as u8);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8(out).unwrap_or_else(|_| s.to_string())
}

fn encode_rel_segments(rel: &str) -> String {
    rel.split('/')
        .filter(|s| !s.is_empty() && *s != ".")
        .map(percent_encode_segment)
        .collect::<Vec<_>>()
        .join("/")
}

fn build_target_url(base_url: &str, remote_path: &str, rel_path: &str) -> String {
    let base = normalize_base_url(base_url);
    let root = normalize_remote_path(remote_path);
    let root_encoded = if root.is_empty() {
        String::new()
    } else {
        format!("/{}", encode_rel_segments(&root[1..]))
    };
    let rel_encoded = encode_rel_segments(rel_path);
    if rel_encoded.is_empty() {
        format!("{base}{root_encoded}/")
    } else {
        format!("{base}{root_encoded}/{rel_encoded}")
    }
}

async fn ensure_remote_dir(
    client: &reqwest::Client,
    base_url: &str,
    remote_path: &str,
    rel_dir: &str,
    username: &str,
    password: &str,
    created_cache: &mut HashSet<String>,
) -> Result<(), String> {
    let mut parts: Vec<String> = Vec::new();
    let root = normalize_remote_path(remote_path);
    if !root.is_empty() {
        for seg in root.trim_matches('/').split('/') {
            if !seg.is_empty() {
                parts.push(seg.to_string());
            }
        }
    }
    for seg in rel_dir.trim_matches('/').split('/') {
        if !seg.is_empty() && seg != "." {
            parts.push(seg.to_string());
        }
    }

    let base = normalize_base_url(base_url);
    let mut current = String::new();
    for seg in parts {
        if !current.is_empty() {
            current.push('/');
        }
        current.push_str(&seg);
        if created_cache.contains(&current) {
            continue;
        }
        let encoded = encode_rel_segments(&current);
        let dir_url = format!("{base}/{encoded}/");
        let mkcol = Method::from_bytes(b"MKCOL").map_err(|e| e.to_string())?;
        let resp = client
            .request(mkcol, &dir_url)
            .basic_auth(username, Some(password))
            .send()
            .await
            .map_err(|e| format!("MKCOL {dir_url} failed: {e}"))?;
        let code = resp.status().as_u16();
        // 201 Created, 405 Method Not Allowed (already exists), 301/200/409
        if code == 401 || code == 403 {
            return Err(format!("Authentication failed (HTTP {code})"));
        }
        created_cache.insert(current.clone());
    }
    Ok(())
}

fn should_skip_dir(name: &str) -> bool {
    name.starts_with('.')
        || name == "node_modules"
        || name == "target"
        || name == "dist"
}

fn should_sync_file(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
        return false;
    };
    if name.starts_with('.') || name == "Thumbs.db" || name == ".DS_Store" {
        return false;
    }
    let Some(ext) = path.extension().and_then(|e| e.to_str()) else {
        return false;
    };
    matches!(
        ext.to_ascii_lowercase().as_str(),
        "md" | "markdown" | "mdown" | "mkd" | "txt" | "png" | "jpg" | "jpeg" | "gif" | "webp" | "svg" | "pdf" | "bib" | "csl"
    )
}

fn collect_local_files(root: &Path, dir: &Path, out: &mut Vec<(String, PathBuf)>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if path.is_dir() {
            if should_skip_dir(name) {
                continue;
            }
            collect_local_files(root, &path, out);
        } else if path.is_file() && should_sync_file(&path) {
            if let Ok(rel) = path.strip_prefix(root) {
                let rel_str = rel
                    .components()
                    .map(|c| c.as_os_str().to_string_lossy().into_owned())
                    .collect::<Vec<_>>()
                    .join("/");
                if !rel_str.is_empty() {
                    out.push((rel_str, path));
                }
            }
        }
    }
}

/// 解析 WebDAV `PROPFIND` 响应：每个 `<response>` 的 href + 是否目录。
///
/// 命名空间前缀任意（`d:` / `D:` / 无前缀 / 自定义）——用「找 `<...response>`
/// 开标签 → 找对应 `</...response>` 闭标签」配对，而不是匹配第一个
/// `response>`（上游那种写法在 `<d:response>` 无属性时会只框住开标签本身，
/// 拿不到 href）。
fn parse_propfind_xml(xml: &str) -> Vec<(String, bool)> {
    let mut results = Vec::new();
    let lower = xml.to_ascii_lowercase();
    let mut pos = 0usize;

    while let Some((block, next)) = next_element_block(xml, &lower, "response", pos) {
        pos = next;
        let block_lower = block.to_ascii_lowercase();
        let Some(href) = first_element_text(block, &block_lower, "href") else {
            continue;
        };
        let href = href.trim();
        if href.is_empty() {
            continue;
        }
        let is_collection =
            block_lower.contains(":collection") || block_lower.contains("<collection");
        results.push((href.to_string(), is_collection));
    }
    results
}

/// 从 `from` 起找下一个 `<tag ...>…</tag>`，返回 (整块内容, 块后位置)。
/// 标签名大小写不敏感、命名空间前缀任意。
fn next_element_block<'a>(
    original: &'a str,
    lower: &str,
    tag: &str,
    from: usize,
) -> Option<(&'a str, usize)> {
    let search = from;
    loop {
        let open = find_open_tag(lower, tag, search)?;
        // open.0 = 开标签起始，open.1 = 开标签 `>` 之后
        let Some(close_start) = find_close_tag(lower, tag, open.1) else {
            return None;
        };
        let inner = &original[open.1..close_start];
        // 闭标签 `</…response>` 之后。
        let Some(gt) = lower[close_start..].find('>') else {
            return None;
        };
        let next = close_start + gt + 1;
        return Some((inner, next));
    }
}

/// 找 `<[prefix:]tag` 开标签，返回 (标签起始, `>` 之后的位置)。
fn find_open_tag(lower: &str, tag: &str, from: usize) -> Option<(usize, usize)> {
    let mut pos = from;
    while let Some(rel) = lower[pos..].find(&format!("<")) {
        let lt = pos + rel;
        let after_lt = lt + 1;
        // 跳过闭标签 `</` 和注释 `<!`/`<?`。
        if matches!(lower.as_bytes().get(after_lt), Some(b'/') | Some(b'!') | Some(b'?')) {
            pos = after_lt;
            continue;
        }
        // 读标签名（可含前缀）。
        let name_start = after_lt;
        let mut name_end = name_start;
        while let Some(&b) = lower.as_bytes().get(name_end) {
            if b.is_ascii_alphanumeric() || b == b':' || b == b'-' || b == b'_' || b == b'.' {
                name_end += 1;
            } else {
                break;
            }
        }
        let name = &lower[name_start..name_end];
        // 后缀匹配：`d:response` 末端是 `response`。
        let is_match = name == tag || name.ends_with(&format!(":{tag}"));
        if is_match {
            // 找到这个标签的 `>`（属性里可能有引号包住的 `>`，这里按
            // 真实 WebDAV 响应简化处理：属性不会含裸 `>`）。
            if let Some(gt) = lower[name_end..].find('>') {
                return Some((lt, name_end + gt + 1));
            }
            return None;
        }
        pos = name_end.max(after_lt);
    }
    None
}

/// 找 `</[prefix:]tag` 的起始位置。
fn find_close_tag(lower: &str, tag: &str, from: usize) -> Option<usize> {
    let mut pos = from;
    loop {
        let rel = lower[pos..].find("</")?;
        let start = pos + rel;
        let name_start = start + 2;
        let mut name_end = name_start;
        while let Some(&b) = lower.as_bytes().get(name_end) {
            if b.is_ascii_alphanumeric() || b == b':' || b == b'-' || b == b'_' || b == b'.' {
                name_end += 1;
            } else {
                break;
            }
        }
        let name = &lower[name_start..name_end];
        if name == tag || name.ends_with(&format!(":{tag}")) {
            return Some(start);
        }
        pos = name_end.max(name_start);
    }
}

/// 取块内首个 `[prefix:]tag` 的文字内容（去 CDATA / XML 实体）。
fn first_element_text<'a>(original: &'a str, lower: &str, tag: &str) -> Option<&'a str> {
    let (start, after_open) = find_open_tag(lower, tag, 0)?;
    let _ = start;
    let close_start = find_close_tag(lower, tag, after_open)?;
    let raw = &original[after_open..close_start];
    // `<d:href>` 里通常是纯文本，偶尔有 CDATA。
    let raw = raw
        .trim()
        .trim_start_matches("<![CDATA[")
        .trim_end_matches("]]>")
        .trim();
    Some(raw)
}

pub async fn webdav_test_connection(
    url: String,
    username: String,
    password: String,
    remote_path: String,
) -> Result<String, String> {
    if url.trim().is_empty() {
        return Err("WebDAV server URL is required".into());
    }
    let client = build_client()?;
    let target = build_target_url(&url, &remote_path, "");
    let propfind = Method::from_bytes(b"PROPFIND").map_err(|e| e.to_string())?;
    let resp = client
        .request(propfind.clone(), &target)
        .header("Depth", "0")
        .basic_auth(username.trim(), Some(password.as_str()))
        .send()
        .await
        .map_err(|e| format!("Connection failed: {e}"))?;

    let status = resp.status().as_u16();
    if status == 401 || status == 403 {
        return Err(format!("Authentication failed (HTTP {status})"));
    }
    if status == 404 {
        // Attempt to create the remote folder automatically
        let mut cache = HashSet::new();
        ensure_remote_dir(
            &client,
            &url,
            &remote_path,
            "",
            username.trim(),
            password.as_str(),
            &mut cache,
        )
        .await?;
        return Ok("Connected (created remote folder)".into());
    }
    if (200..300).contains(&status) || status == 207 {
        return Ok(format!("Connected (HTTP {status})"));
    }
    Err(format!("WebDAV server returned HTTP {status}"))
}

pub async fn webdav_push_workspace(
    workspace: String,
    url: String,
    username: String,
    password: String,
    remote_path: String,
    encrypted: Option<bool>,
    passphrase: Option<String>,
) -> Result<WebdavSyncResult, String> {
    let ws_path = PathBuf::from(workspace.trim());
    if !ws_path.is_dir() {
        return Err("Open a workspace folder before syncing".into());
    }
    if url.trim().is_empty() {
        return Err("Configure WebDAV server URL in Settings › Sync first".into());
    }

    let use_encryption = encrypted.unwrap_or(false);
    let pass_str = passphrase.unwrap_or_default();

    let mut files = Vec::new();
    collect_local_files(&ws_path, &ws_path, &mut files);

    let client = build_client()?;
    let mut created_dirs = HashSet::new();
    ensure_remote_dir(
        &client,
        &url,
        &remote_path,
        "",
        username.trim(),
        password.as_str(),
        &mut created_dirs,
    )
    .await?;

    let enc_key: Option<[u8; 32]> = if use_encryption {
        let manifest_url = build_target_url(&url, &remote_path, VAULT_MANIFEST_NAME);
        let existing_resp = client
            .get(&manifest_url)
            .basic_auth(username.trim(), Some(password.as_str()))
            .send()
            .await
            .map_err(|e| format!("Failed to check remote vault manifest: {e}"))?;
        let key = if existing_resp.status().is_success() {
            let text = existing_resp.text().await.unwrap_or_default();
            let manifest: CloudVaultManifest = serde_json::from_str(&text)
                .map_err(|e| format!("Invalid remote solomd-vault.json: {e}"))?;
            unlock_vault_manifest(&manifest, &pass_str)?
        } else {
            let (manifest, k) = create_vault_manifest(&pass_str)?;
            let manifest_json = serde_json::to_string_pretty(&manifest)
                .map_err(|e| e.to_string())?;
            let put_resp = client
                .put(&manifest_url)
                .basic_auth(username.trim(), Some(password.as_str()))
                .body(manifest_json.into_bytes())
                .send()
                .await
                .map_err(|e| format!("Failed to upload vault manifest: {e}"))?;
            if !put_resp.status().is_success() {
                return Err(format!(
                    "Failed to upload solomd-vault.json (HTTP {})",
                    put_resp.status()
                ));
            }
            k
        };
        Some(key)
    } else {
        None
    };

    let mut uploaded = 0usize;
    for (rel, full_path) in files {
        if let Some((parent, _)) = rel.rsplit_once('/') {
            ensure_remote_dir(
                &client,
                &url,
                &remote_path,
                parent,
                username.trim(),
                password.as_str(),
                &mut created_dirs,
            )
            .await?;
        }
        let plain_bytes = std::fs::read(&full_path)
            .map_err(|e| format!("Failed to read {rel}: {e}"))?;
        let (remote_rel, upload_bytes) = if let Some(ref key) = enc_key {
            let cipher = crate::crypto::encrypt_bytes(key, &rel, &plain_bytes)?;
            (format!("{rel}.enc"), cipher)
        } else {
            (rel.clone(), plain_bytes)
        };
        let file_url = build_target_url(&url, &remote_path, &remote_rel);
        let resp = client
            .put(&file_url)
            .basic_auth(username.trim(), Some(password.as_str()))
            .body(upload_bytes)
            .send()
            .await
            .map_err(|e| format!("Failed to upload {remote_rel}: {e}"))?;
        let code = resp.status().as_u16();
        if code == 401 || code == 403 {
            return Err(format!(
                "Authentication failed while uploading {remote_rel} (HTTP {code})"
            ));
        }
        if !(200..300).contains(&code) {
            return Err(format!("Upload failed for {remote_rel} (HTTP {code})"));
        }
        uploaded += 1;
    }

    let msg = if use_encryption {
        format!("已端到端加密上传 {uploaded} 个文件到 WebDAV (Encrypted & uploaded {uploaded} files)")
    } else {
        format!("Uploaded {uploaded} files to WebDAV")
    };

    Ok(WebdavSyncResult {
        uploaded,
        downloaded: 0,
        message: msg,
    })
}

pub async fn webdav_pull_workspace(
    workspace: String,
    url: String,
    username: String,
    password: String,
    remote_path: String,
    encrypted: Option<bool>,
    passphrase: Option<String>,
) -> Result<WebdavSyncResult, String> {
    let ws_path = PathBuf::from(workspace.trim());
    if !ws_path.is_dir() {
        return Err("Open a workspace folder before syncing".into());
    }
    if url.trim().is_empty() {
        return Err("Configure WebDAV server URL in Settings › Sync first".into());
    }

    let client = build_client()?;
    let pass_str = passphrase.unwrap_or_default();

    // Check if the remote folder has a `solomd-vault.json` encrypted manifest.
    let manifest_url = build_target_url(&url, &remote_path, VAULT_MANIFEST_NAME);
    let manifest_resp = client
        .get(&manifest_url)
        .basic_auth(username.trim(), Some(password.as_str()))
        .send()
        .await;

    let enc_key: Option<[u8; 32]> = match manifest_resp {
        Ok(r) if r.status().is_success() => {
            let text = r.text().await.unwrap_or_default();
            if let Ok(manifest) = serde_json::from_str::<CloudVaultManifest>(&text) {
                Some(unlock_vault_manifest(&manifest, &pass_str)?)
            } else if encrypted.unwrap_or(false) {
                return Err("远程网盘未找到有效的加密元数据 solomd-vault.json".into());
            } else {
                None
            }
        }
        _ => {
            if encrypted.unwrap_or(false) {
                return Err("远程网盘尚未初始化加密库 (Remote encrypted vault not found — push first)".into());
            }
            None
        }
    };

    let propfind = Method::from_bytes(b"PROPFIND").map_err(|e| e.to_string())?;

    // Breadth-first `Depth: 1` traversal (works on all WebDAV servers including
    // 坚果云 Nutstore and Nextcloud, which reject `Depth: infinity`).
    let mut queue: Vec<String> = vec![String::new()];
    let mut visited_dirs: HashSet<String> = HashSet::new();
    let mut downloaded = 0usize;

    while let Some(rel_dir) = queue.pop() {
        if !visited_dirs.insert(rel_dir.clone()) {
            continue;
        }
        let dir_url = build_target_url(&url, &remote_path, &rel_dir);
        let resp = client
            .request(propfind.clone(), &dir_url)
            .header("Depth", "1")
            .basic_auth(username.trim(), Some(password.as_str()))
            .send()
            .await
            .map_err(|e| format!("PROPFIND failed: {e}"))?;

        let status = resp.status().as_u16();
        if status == 404 && rel_dir.is_empty() {
            return Ok(WebdavSyncResult {
                uploaded: 0,
                downloaded: 0,
                message: "Remote WebDAV folder is empty".into(),
            });
        }
        if status == 401 || status == 403 {
            return Err(format!("Authentication failed (HTTP {status})"));
        }
        if !(200..300).contains(&status) && status != 207 {
            return Err(format!("WebDAV PROPFIND returned HTTP {status}"));
        }

        let xml = resp.text().await.unwrap_or_default();
        let entries = parse_propfind_xml(&xml);
        for (idx, (href, is_collection)) in entries.into_iter().enumerate() {
            // The first entry in Depth:1 response is typically the directory itself.
            let decoded = percent_decode(&href);
            let trimmed_href = decoded.trim_end_matches('/');
            let Some(leaf) = trimmed_href.rsplit('/').next() else {
                continue;
            };
            if leaf.is_empty() || leaf.starts_with('.') || leaf == VAULT_MANIFEST_NAME {
                continue;
            }
            if idx == 0 {
                continue;
            }
            let child_rel = if rel_dir.is_empty() {
                leaf.to_string()
            } else {
                format!("{rel_dir}/{leaf}")
            };
            if is_collection {
                if !should_skip_dir(leaf) && visited_dirs.len() < 200 {
                    queue.push(child_rel);
                }
            } else if let Some(plain_rel) = child_rel.strip_suffix(".enc") {
                let Some(ref key) = enc_key else {
                    continue;
                };
                let local_target = ws_path.join(plain_rel);
                if !should_sync_file(&local_target) {
                    continue;
                }
                let file_url = build_target_url(&url, &remote_path, &child_rel);
                let get_resp = client
                    .get(&file_url)
                    .basic_auth(username.trim(), Some(password.as_str()))
                    .send()
                    .await
                    .map_err(|e| format!("Failed to download {child_rel}: {e}"))?;
                if get_resp.status().is_success() {
                    let cipher_bytes = get_resp
                        .bytes()
                        .await
                        .map_err(|e| format!("Read error for {child_rel}: {e}"))?;
                    let plain_bytes = crate::crypto::decrypt_bytes(key, plain_rel, &cipher_bytes)?;
                    if let Some(parent) = local_target.parent() {
                        let _ = std::fs::create_dir_all(parent);
                    }
                    std::fs::write(&local_target, &plain_bytes)
                        .map_err(|e| format!("Write error for {plain_rel}: {e}"))?;
                    downloaded += 1;
                }
            } else {
                let local_target = ws_path.join(&child_rel);
                if !should_sync_file(&local_target) {
                    continue;
                }
                let file_url = build_target_url(&url, &remote_path, &child_rel);
                let get_resp = client
                    .get(&file_url)
                    .basic_auth(username.trim(), Some(password.as_str()))
                    .send()
                    .await
                    .map_err(|e| format!("Failed to download {child_rel}: {e}"))?;
                if get_resp.status().is_success() {
                    if let Some(parent) = local_target.parent() {
                        let _ = std::fs::create_dir_all(parent);
                    }
                    let bytes = get_resp
                        .bytes()
                        .await
                        .map_err(|e| format!("Read error for {child_rel}: {e}"))?;
                    std::fs::write(&local_target, &bytes)
                        .map_err(|e| format!("Write error for {child_rel}: {e}"))?;
                    downloaded += 1;
                }
            }
        }
    }

    let msg = if enc_key.is_some() {
        format!("已从 WebDAV 下载并解密 {downloaded} 个文件 (Downloaded & decrypted {downloaded} files)")
    } else {
        format!("Downloaded {downloaded} files from WebDAV")
    };

    Ok(WebdavSyncResult {
        uploaded: 0,
        downloaded,
        message: msg,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urls_build_with_percent_encoding() {
        let u = build_target_url("https://dav.example.com/remote.php/dav/files/u/", "/Notes", "");
        assert_eq!(u, "https://dav.example.com/remote.php/dav/files/u/Notes/");
        let u = build_target_url(
            "https://dav.example.com",
            "Notes/子目录",
            "a b/c&d.md",
        );
        assert_eq!(
            u,
            "https://dav.example.com/Notes/%E5%AD%90%E7%9B%AE%E5%BD%95/a%20b/c%26d.md"
        );
    }

    #[test]
    fn remote_path_normalizes() {
        assert_eq!(normalize_remote_path(""), "");
        assert_eq!(normalize_remote_path("/"), "");
        assert_eq!(normalize_remote_path("Notes/"), "/Notes");
        assert_eq!(normalize_remote_path("\\Notes\\sub\\"), "/Notes/sub");
    }

    #[test]
    fn percent_round_trip() {
        for s in ["a b", "子目录", "a&b", "x+y=z", "a%20b"] {
            let enc = percent_encode_segment(s);
            assert!(!enc.contains(' ') && !enc.contains('/'), "{enc}");
            // 中文/保留字符被编码，ASCII 原样。
            let dec = percent_decode(&enc);
            assert_eq!(dec, s, "round trip failed for {s} -> {enc}");
        }
        // 非法百分号原样保留（不能丢字符）。
        assert_eq!(percent_decode("100%zz"), "100%zz");
    }

    #[test]
    fn propfind_parses_nutstore_and_nextcloud_shapes() {
        let nutstore = r#"<?xml version="1.0"?>
<d:multistatus xmlns:d="DAV:">
  <d:response>
    <d:href>/dav/Notes/</d:href>
    <d:propstat><d:prop><d:resourcetype><d:collection/></d:resourcetype></d:prop></d:propstat>
  </d:response>
  <d:response>
    <d:href>/dav/Notes/a.md</d:href>
    <d:propstat><d:prop><d:resourcetype/></d:propstat>
  </d:response>
  <d:response>
    <d:href>/dav/Notes/sub/</d:href>
    <d:propstat><d:prop><d:resourcetype><d:collection/></d:resourcetype></d:prop></d:propstat>
  </d:response>
</d:multistatus>"#;
        let entries = parse_propfind_xml(nutstore);
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0].0, "/dav/Notes/");
        assert!(entries[0].1, "目录应识别为 collection");
        assert_eq!(entries[1].0, "/dav/Notes/a.md");
        assert!(!entries[1].1);
        assert!(entries[2].1);
    }

    #[test]
    fn file_and_dir_filters_match_upstream() {
        assert!(should_skip_dir(".git"));
        assert!(should_skip_dir("node_modules"));
        assert!(should_skip_dir("target"));
        assert!(!should_skip_dir("Notes"));
        assert!(!should_skip_dir("子目录"));

        assert!(should_sync_file(Path::new("/ws/a.md")));
        assert!(should_sync_file(Path::new("/ws/a.PNG")));
        assert!(should_sync_file(Path::new("/ws/refs.bib")));
        assert!(!should_sync_file(Path::new("/ws/.DS_Store")));
        assert!(!should_sync_file(Path::new("/ws/x.dmg")));
        assert!(!should_sync_file(Path::new("/ws/noext")));
    }

    #[test]
    fn collect_local_files_skips_build_dirs_and_dotfiles() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        std::fs::create_dir_all(root.join("Notes/sub")).unwrap();
        std::fs::create_dir_all(root.join("node_modules")).unwrap();
        std::fs::create_dir_all(root.join(".git")).unwrap();
        std::fs::write(root.join("a.md"), "a").unwrap();
        std::fs::write(root.join("Notes/sub/b.md"), "b").unwrap();
        std::fs::write(root.join("node_modules/x.md"), "x").unwrap();
        std::fs::write(root.join(".git/y.md"), "y").unwrap();
        std::fs::write(root.join(".hidden.md"), "h").unwrap();
        std::fs::write(root.join("skip.dmg"), "d").unwrap();

        let mut files = Vec::new();
        collect_local_files(root, root, &mut files);
        let mut rels: Vec<&str> = files.iter().map(|(r, _)| r.as_str()).collect();
        rels.sort();
        assert_eq!(rels, vec!["Notes/sub/b.md", "a.md"]);
    }

    #[test]
    fn encrypted_manifest_round_trip_rejects_wrong_passphrase() {
        let (manifest, key) = create_vault_manifest("correct horse").unwrap();
        let json = serde_json::to_string(&manifest).unwrap();
        let parsed: CloudVaultManifest = serde_json::from_str(&json).unwrap();
        // 正确口令解锁出同一把钥匙。
        let unlocked = unlock_vault_manifest(&parsed, "correct horse").unwrap();
        assert_eq!(unlocked, key);
        // 错误口令必须被 probe 拦住，而不是静默给出一把错钥匙。
        assert!(unlock_vault_manifest(&parsed, "wrong").is_err());
    }

    #[test]
    fn empty_passphrase_is_rejected() {
        assert!(create_vault_manifest("   ").is_err());
        let (manifest, _) = create_vault_manifest("x").unwrap();
        assert!(unlock_vault_manifest(&manifest, "").is_err());
    }
}
