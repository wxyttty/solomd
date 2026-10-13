/**
 * Shared image URL resolution for SoloMD.
 * Used by Preview.vue, pdf-export.ts, image-export.ts, and docx-export.ts
 * to convert local image paths into URLs the webview can load or bytes for embedding.
 */

import { convertFileSrc, invoke } from '@tauri-apps/api/core';

/**
 * Normalize a filesystem path so `convertFileSrc` produces a URL the
 * webview will actually load on every platform.
 *   1. Mixed `\` / `/` separators are unified.
 *   2. `./` and `../` segments are resolved.
 *   3. Windows drive prefixes (`C:`) survive normalization.
 */
export function normalizePath(p: string): string {
  if (!p) return p;
  let s = p.replace(/\\/g, '/');
  const driveMatch = s.match(/^([a-zA-Z]):\/(.*)$/);
  let prefix = '';
  let body = s;
  if (driveMatch) {
    prefix = driveMatch[1].toUpperCase() + ':/';
    body = driveMatch[2];
  } else if (s.startsWith('//')) {
    prefix = '//';
    body = s.slice(2);
  } else if (s.startsWith('/')) {
    prefix = '/';
    body = s.slice(1);
  }
  const out: string[] = [];
  for (const seg of body.split('/')) {
    if (seg === '' || seg === '.') continue;
    if (seg === '..') {
      if (out.length > 0) out.pop();
      continue;
    }
    out.push(seg);
  }
  return prefix + out.join('/');
}

const LOCAL_IMAGE_WITH_URL_SUFFIX =
  /^(.*?\.(?:png|jpe?g|gif|webp|svg|bmp|tiff?|avif|heic|heif|ico))([?#].*)$/i;

function decodeHtmlAttr(value: string): string {
  return value
    .replace(/&amp;/g, '&')
    .replace(/&quot;/g, '"')
    .replace(/&#39;/g, "'")
    .replace(/&lt;/g, '<')
    .replace(/&gt;/g, '>');
}

function encodeHtmlAttr(value: string): string {
  return value
    .replace(/&/g, '&amp;')
    .replace(/"/g, '&quot;')
    .replace(/'/g, '&#39;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;');
}

function decodeUriOnce(value: string): string {
  try {
    return decodeURIComponent(value);
  } catch {
    try {
      return decodeURI(value);
    } catch {
      return value;
    }
  }
}

function stripLocalImageUrlSuffix(value: string): string {
  const match = LOCAL_IMAGE_WITH_URL_SUFFIX.exec(value);
  return match ? match[1] : value;
}

function fileUrlToPath(value: string): string {
  if (!/^file:\/\//i.test(value)) return value;
  const rest = value.slice('file://'.length);
  if (/^[a-zA-Z]:[\\/]/.test(rest)) return rest;
  if (rest.startsWith('/') && /^\/[a-zA-Z]:[\\/]/.test(rest)) return rest.slice(1);
  if (/^localhost\//i.test(rest)) return rest.slice('localhost/'.length);
  if (!rest.startsWith('/')) return `//${rest}`;
  return rest;
}

function cleanLocalImageSrc(src: string): string {
  const attr = decodeHtmlAttr(src.trim());
  const withoutUrlSuffix = stripLocalImageUrlSuffix(attr);
  return fileUrlToPath(decodeUriOnce(withoutUrlSuffix));
}

export function isLocalSvgPath(value: string): boolean {
  return /\.svg$/i.test(stripLocalImageUrlSuffix(value));
}

export function svgTextToDataUrl(svg: string): string {
  return `data:image/svg+xml;charset=utf-8,${encodeURIComponent(svg)}`;
}

/**
 * Resolve a local image src to an absolute filesystem path.
 * Returns the original src unchanged for remote/data/blob/asset URLs.
 */
export function resolveImagePath(
  src: string,
  imageRoot: string | null,
  filePath?: string,
): string {
  if (!src) return src;
  if (/^(https?|data|blob|asset|tauri):/i.test(src)) return src;

  let p = cleanLocalImageSrc(src);

  const isAbsolute = p.startsWith('/') || /^[a-zA-Z]:[\\/]/.test(p);
  if (!isAbsolute) {
    let base: string | null = null;
    if (imageRoot) {
      const rootAbsolute = imageRoot.startsWith('/') || /^[a-zA-Z]:[\\/]/.test(imageRoot);
      if (rootAbsolute) {
        base = imageRoot;
      } else if (filePath) {
        const dir = filePath.replace(/[\\/][^\\/]*$/, '');
        base = dir + '/' + imageRoot;
      }
    }
    if (!base && filePath) {
      base = filePath.replace(/[\\/][^\\/]*$/, '');
    }
    if (base) {
      p = base + '/' + p;
    }
  }

  return normalizePath(p);
}

/**
 * Resolve a single image src into something the webview can load.
 * Converts local paths to `asset://` URLs via Tauri's convertFileSrc().
 */
export function resolveImageSrc(
  src: string,
  imageRoot: string | null,
  filePath?: string,
): string {
  const resolved = resolveImagePath(src, imageRoot, filePath);
  // If it's still a remote/data/blob URL, return as-is
  if (/^(https?|data|blob|asset|tauri):/i.test(resolved)) return resolved;
  try {
    return convertFileSrc(resolved);
  } catch {
    return src;
  }
}

export function resolveImageSrcWithMeta(
  src: string,
  imageRoot: string | null,
  filePath?: string,
): { src: string; localPath: string | null } {
  if (!src || /^(https?|data|blob|asset|tauri):/i.test(src)) {
    return { src, localPath: null };
  }
  const localPath = resolveImagePath(src, imageRoot, filePath);
  if (/^(https?|data|blob|asset|tauri):/i.test(localPath)) {
    return { src: localPath, localPath: null };
  }
  try {
    return { src: convertFileSrc(localPath), localPath };
  } catch {
    return { src, localPath };
  }
}

/** Rewrite all `<img src=…>` URLs in the rendered markdown HTML. */
export function rewriteImageUrls(
  rawHtml: string,
  imageRoot: string | null,
  filePath?: string,
): string {
  return rawHtml.replace(
    /(<img[^>]*\bsrc=)(["'])([^"']*)\2/gi,
    (_match, prefix: string, q: string, src: string) => {
      // markdown-it percent-encodes non-ASCII in image URLs (`感` → `%E6%84%9F`).
      // Passing that straight to convertFileSrc encodes the `%` again, yielding
      // `%25E6…` — a double-encoded path that 404s, so images under a Chinese
      // (or space-containing) folder never load (顾河 report, Typora `./images/
      // <笔记名>/` paths). Decode the local path first so it's encoded exactly
      // once — mirroring rewriteLinkUrls. Remote/data/asset URLs are left alone.
      if (!src || /^(https?|data|blob|asset|tauri):/i.test(src)) {
        return `${prefix}${q}${src}${q}`;
      }
      const resolved = resolveImageSrcWithMeta(src, imageRoot, filePath);
      const nextPrefix = resolved.localPath && isLocalSvgPath(resolved.localPath)
        ? prefix.replace(/^<img\b/i, `<img data-solomd-local-src="${encodeHtmlAttr(resolved.localPath)}"`)
        : prefix;
      return `${nextPrefix}${q}${resolved.src}${q}`;
    },
  );
}

async function loadLocalSvgDataUrl(path: string): Promise<string | null> {
  try {
    const result = await invoke<{ content: string }>('read_file', { path });
    const svg = result.content.trimStart();
    if (!svg.startsWith('<svg') && !svg.startsWith('<?xml')) return null;
    return svgTextToDataUrl(result.content);
  } catch {
    return null;
  }
}

/** MIME type for an image path, or `null` when the extension is not an image. */
function imageMimeForPath(path: string): string | null {
  const ext = stripLocalImageUrlSuffix(path).split('.').pop()?.toLowerCase() ?? '';
  switch (ext) {
    case 'png':
    case 'apng':
      return 'image/png';
    case 'jpg':
    case 'jpeg':
      return 'image/jpeg';
    case 'gif':
      return 'image/gif';
    case 'webp':
      return 'image/webp';
    case 'svg':
      return 'image/svg+xml';
    case 'bmp':
      return 'image/bmp';
    case 'tif':
    case 'tiff':
      return 'image/tiff';
    case 'avif':
      return 'image/avif';
    case 'heic':
    case 'heif':
      return 'image/heic';
    case 'ico':
      return 'image/x-icon';
    default:
      return null;
  }
}

function bytesToBase64(bytes: Uint8Array): string {
  let binary = '';
  // Chunked: spreading a multi-megabyte array into fromCharCode overflows the
  // argument limit.
  const CHUNK = 0x8000;
  for (let i = 0; i < bytes.length; i += CHUNK) {
    binary += String.fromCharCode(...bytes.subarray(i, i + CHUNK));
  }
  return btoa(binary);
}

/** Reads a file's bytes — injectable so the inlining can be unit tested. */
export type BinaryReader = (path: string) => Promise<Uint8Array | null>;

const readBinaryViaTauri: BinaryReader = async (path) => {
  try {
    return new Uint8Array(await invoke<number[]>('read_binary_file', { path }));
  } catch {
    return null;
  }
};

/**
 * Embed every local image in rendered markdown HTML as a `data:` URL.
 *
 * Exported HTML is a standalone file, so the `asset.localhost` URLs the app
 * rewrites local paths to — which is what makes images show up in the webview
 * — are unreachable the moment that file leaves the app: a shared .html opens
 * with broken images (issue reported with a note whose figures are local SVGs).
 * Inlining the bytes keeps the export self-contained and offline, matching what
 * the rest of the export path promises.
 *
 * Remote / `data:` / blob sources are left alone, and so is anything that can't
 * be read (a missing file stays a broken link rather than vanishing).
 */
export async function inlineLocalImages(
  rawHtml: string,
  imageRoot: string | null,
  filePath?: string,
  readBytes: BinaryReader = readBinaryViaTauri,
): Promise<string> {
  const matches = [...rawHtml.matchAll(/(<img[^>]*\bsrc=)(["'])([^"']*)\2/gi)];
  if (!matches.length) return rawHtml;

  const cache = new Map<string, string | null>();
  const dataUrlFor = async (src: string): Promise<string | null> => {
    if (!src || /^(https?|data|blob|asset|tauri):/i.test(src)) return null;
    const path = resolveImagePath(src, imageRoot, filePath);
    if (/^(https?|data|blob|asset|tauri):/i.test(path)) return null;
    const cached = cache.get(path);
    if (cached !== undefined) return cached;

    const mime = imageMimeForPath(path);
    const bytes = mime ? await readBytes(path) : null;
    const url = mime && bytes && bytes.length ? `data:${mime};base64,${bytesToBase64(bytes)}` : null;
    cache.set(path, url);
    return url;
  };

  let out = '';
  let cursor = 0;
  for (const match of matches) {
    const [full, prefix, quote, src] = match;
    const index = match.index ?? 0;
    out += rawHtml.slice(cursor, index);
    cursor = index + full.length;
    const dataUrl = await dataUrlFor(src);
    out += dataUrl ? `${prefix}${quote}${dataUrl}${quote}` : full;
  }
  return out + rawHtml.slice(cursor);
}

export function installSvgImageFallbacks(root: ParentNode): void {
  const images = root.querySelectorAll<HTMLImageElement>('img[data-solomd-local-src]');
  for (const img of Array.from(images)) {
    if (img.dataset.solomdSvgFallbackBound === '1') continue;
    const path = img.dataset.solomdLocalSrc || '';
    if (!isLocalSvgPath(path)) continue;
    img.dataset.solomdSvgFallbackBound = '1';
    const fallback = async () => {
      if (img.src.startsWith('data:image/svg+xml')) return;
      const dataUrl = await loadLocalSvgDataUrl(path);
      if (dataUrl) img.src = dataUrl;
    };
    img.addEventListener('error', fallback, { once: true });
    if (img.complete && img.naturalWidth === 0) {
      void fallback();
    }
  }
}

/**
 * v4.3.0 issue #77 — local-file `<a href=…>` URLs in rendered markdown
 * resolve against the webview's base URL (`http://tauri.localhost/`),
 * which then bakes into exported PDFs / DOCX / images as a useless
 * `http://tauri.localhost/foo.factoryio` link.
 *
 * This rewrites local-file hrefs to absolute `file://` URLs so the link
 * (a) still works on the original machine and (b) shows a meaningful
 * file-system path when the PDF is shared. Remote schemes (http/https/
 * mailto/tel/data/etc.) and in-page anchors (`#section`) are left alone.
 */
export function rewriteLinkUrls(
  rawHtml: string,
  imageRoot: string | null,
  filePath?: string,
): string {
  return rawHtml.replace(
    /(<a\b[^>]*\bhref=)(["'])([^"']*)\2/gi,
    (_match, prefix: string, q: string, href: string) => {
      // Leave anchors / remote / data-style URLs as-is.
      if (!href) return `${prefix}${q}${q}`;
      if (href.startsWith('#')) return `${prefix}${q}${href}${q}`;
      if (/^(https?|mailto|tel|sms|data|blob|asset|tauri|ftp|file):/i.test(href)) {
        return `${prefix}${q}${href}${q}`;
      }
      // Decode the path so `%20` etc. don't go through resolution twice.
      let decoded: string;
      try { decoded = decodeURI(href); } catch { decoded = href; }
      const resolved = resolveImagePath(decoded, imageRoot, filePath);
      const isAbs = resolved.startsWith('/') || /^[a-zA-Z]:[\\/]/.test(resolved);
      if (!isAbs) return `${prefix}${q}${href}${q}`;
      // Build a `file://` URL — encodeURI keeps the path readable while
      // escaping spaces / unicode. Windows drives need an extra `/`.
      const fileUrl = /^[a-zA-Z]:/.test(resolved)
        ? `file:///${encodeURI(resolved.replace(/\\/g, '/'))}`
        : `file://${encodeURI(resolved)}`;
      return `${prefix}${q}${fileUrl}${q}`;
    },
  );
}
