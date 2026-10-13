/**
 * Dev harness for the HTML export's local images — open `/html-harness.html`
 * from the Vite dev server (`npm --prefix app run dev`).
 *
 * The reported bug: a note exported to HTML showed its figures as broken
 * images, because the export rewrote local paths into `http://asset.localhost/…`
 * — a URL only SoloMD's own webview can resolve. The fix embeds the bytes.
 *
 * A web page cannot read the note off disk, so the caller hands the note and
 * its images over as base64:
 *
 *     await window.__htmlHarnessCheck({ path, markdown, files })
 *
 * with `files` keyed by the absolute path the note uses. It then runs the *real*
 * pipeline (`renderMarkdown` → `inlineLocalImages` → `rewriteImageUrls`), renders
 * both the old and the new markup side by side, and checks that every embedded
 * image decodes back to the bytes it started from.
 */
import {
  inlineLocalImages,
  resolveImagePath,
  rewriteImageUrls,
  type BinaryReader,
} from '../lib/image-resolve';
import { renderMarkdown } from '../lib/markdown';

const statusEl = document.getElementById('status') as HTMLElement;
const beforeEl = document.getElementById('before') as HTMLElement;
const afterEl = document.getElementById('after') as HTMLElement;
const reportEl = document.getElementById('report') as HTMLElement;

export interface HtmlHarnessInput {
  /** Absolute path of the note, so relative image paths resolve like they do in the app. */
  path: string;
  /** Note source. */
  markdown: string;
  /** Absolute image path → base64 of its bytes. */
  files: Record<string, string>;
}

export interface HtmlHarnessReport {
  status: 'ok' | 'failed';
  beforeSrcs: string[];
  afterSrcs: string[];
  embedded: { path: string; mime: string; bytes: number; roundTrip: boolean }[];
  failed: string[];
}

function base64ToBytes(base64: string): Uint8Array {
  const binary = atob(base64);
  const out = new Uint8Array(binary.length);
  for (let i = 0; i < binary.length; i++) out[i] = binary.charCodeAt(i);
  return out;
}

/**
 * Attribute value. The leading whitespace matters: `data-solomd-local-src` must
 * not be able to answer for `src`.
 */
const attr = (tag: string, name: string): string | null =>
  new RegExp(`(?:^|\\s)${name}=["']([^"']*)["']`, 'i').exec(tag)?.[1] ?? null;

const imgTags = (html: string): string[] => html.match(/<img\b[^>]*>/gi) ?? [];

/**
 * What the app's `convertFileSrc()` produces inside the webview — spelled out,
 * because the harness runs outside Tauri where that call has nothing to talk to.
 */
function assetUrlFor(path: string): string {
  return `http://asset.localhost/${encodeURIComponent(path)}`;
}

async function check(input: HtmlHarnessInput): Promise<HtmlHarnessReport> {
  const readBytes: BinaryReader = async (path) => {
    const base64 = input.files[path];
    return base64 === undefined ? null : base64ToBytes(base64);
  };

  const raw = renderMarkdown(input.markdown);
  const beforeTags = imgTags(raw);
  const afterTags = imgTags(
    rewriteImageUrls(await inlineLocalImages(raw, null, input.path, readBytes), null, input.path),
  );

  const embedded: HtmlHarnessReport['embedded'] = [];
  const failed: string[] = [];
  const beforeSrcs: string[] = [];
  const afterSrcs: string[] = [];
  beforeEl.replaceChildren();
  afterEl.replaceChildren();

  beforeTags.forEach((tag, i) => {
    const src = attr(tag, 'src') ?? '';
    const alt = attr(tag, 'alt') ?? '';
    const remote = /^(https?|data|blob|asset|tauri):/i.test(src);
    const path = remote ? null : resolveImagePath(src, null, input.path);

    // Left pane: the src the old export wrote (an asset URL the file can't
    // reach); right pane: the bytes, embedded.
    const beforeSrc = path ? assetUrlFor(path) : src;
    const afterSrc = attr(afterTags[i] ?? '', 'src') ?? '';
    beforeSrcs.push(beforeSrc);
    afterSrcs.push(afterSrc.slice(0, 60));
    beforeEl.insertAdjacentHTML('beforeend', `<img src="${beforeSrc}" alt="${alt}">`);
    afterEl.insertAdjacentHTML('beforeend', `<img src="${afterSrc}" alt="${alt}">`);

    const match = /^data:([^;,]+);base64,(.*)$/s.exec(afterSrc);
    if (!match) {
      failed.push(beforeSrc.slice(0, 80));
      return;
    }
    const bytes = base64ToBytes(match[2]);
    const original = path ? input.files[path] : undefined;
    const expected = original === undefined ? null : base64ToBytes(original);
    embedded.push({
      path: path ?? src,
      mime: match[1],
      bytes: bytes.length,
      roundTrip:
        expected !== null &&
        expected.length === bytes.length &&
        bytes.every((b, index) => b === expected[index]),
    });
  });

  const report: HtmlHarnessReport = {
    status: failed.length || embedded.some((e) => !e.roundTrip) ? 'failed' : 'ok',
    beforeSrcs,
    afterSrcs,
    embedded,
    failed,
  };

  statusEl.textContent =
    report.status === 'ok'
      ? `PASS — ${embedded.length} image(s) embedded from ${Object.keys(input.files).length} file(s), all byte-identical`
      : `FAIL — ${failed.length} image(s) still unreachable, ${embedded.filter((e) => !e.roundTrip).length} corrupted`;
  statusEl.style.borderLeftColor = report.status === 'ok' ? '#1a7f37' : '#c02b2b';
  reportEl.innerHTML =
    '<table><thead><tr><th>image</th><th>mime</th><th>bytes</th><th>round-trips?</th></tr></thead><tbody>' +
    embedded
      .map(
        (e) =>
          `<tr><td>${e.path.split('/').pop()}</td><td>${e.mime}</td><td>${e.bytes}</td>` +
          `<td class="${e.roundTrip ? 'ok' : 'bad'}">${e.roundTrip ? 'yes' : 'NO'}</td></tr>`,
      )
      .join('') +
    '</tbody></table>';
  return report;
}

declare global {
  interface Window {
    __htmlHarness?: HtmlHarnessReport | { status: 'failed'; error: string };
    __htmlHarnessCheck?: (input: HtmlHarnessInput) => Promise<HtmlHarnessReport>;
  }
}

window.__htmlHarnessCheck = check;
statusEl.textContent = 'ready — call window.__htmlHarnessCheck({ path, markdown, files })';

// ── #332 / #256: the whole standalone export, and the Word export ──────────
//
//     await window.__htmlHarnessExport({ content, title })  → the .html string
//     await window.__docxHarness(markdown)                  → .docx as base64
//
// Both run the app's own builders (`buildStandaloneHtml`, `markdownToDocxBlob`)
// so what is checked is what the export writes.
import { buildStandaloneHtml, type StandaloneHtmlInput } from '../lib/html-export';

async function exportWhole(input: StandaloneHtmlInput): Promise<string> {
  const html = await buildStandaloneHtml(input);
  statusEl.textContent = `export built — ${html.length} chars`;
  return html;
}

async function docx(markdown: string): Promise<string> {
  const { markdownToDocxBlob } = await import('../lib/docx-export');
  const blob = await markdownToDocxBlob(markdown, 'harness');
  const buf = new Uint8Array(await blob.arrayBuffer());
  let bin = '';
  for (let i = 0; i < buf.length; i += 0x8000) bin += String.fromCharCode(...buf.subarray(i, i + 0x8000));
  return btoa(bin);
}

declare global {
  interface Window {
    __htmlHarnessExport?: (input: StandaloneHtmlInput) => Promise<string>;
    __docxHarness?: (markdown: string) => Promise<string>;
  }
}
window.__htmlHarnessExport = exportWhole;
window.__docxHarness = docx;
