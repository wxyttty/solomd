/**
 * Where an image to insert comes from, and what it is (#349).
 *
 * On Android the image pickers — the system photo picker behind "Insert
 * image…" and the SAF document picker — never hand back a filesystem path.
 * They return a `content://` URI, which `copy_file` (std::fs::copy) cannot
 * open, and which carries no file extension. Before this, the copy failed,
 * the error only reached the console, and the image was never inserted. It
 * looked like the picker did nothing.
 *
 * Pure helpers only (no Tauri imports) so they can be unit-tested in node.
 */

/** A `content://` URI from an Android picker or share intent. */
export function isContentUri(p: string): boolean {
  return /^content:\/\//i.test(p);
}

/** A document inside a SAF vault (`saf:<documentId>`). Not a filesystem path:
 *  binary files can't be written there yet, only text notes. */
export function isSafPath(p: string | undefined | null): boolean {
  return !!p && p.startsWith('saf:');
}

/** Image type from its first bytes. `null` when it isn't a format we know. */
export function sniffImageExt(b: Uint8Array): string | null {
  const at = (i: number, ...v: number[]) => v.every((x, k) => b[i + k] === x);
  const ascii = (i: number, s: string) => at(i, ...[...s].map((c) => c.charCodeAt(0)));
  if (b.length < 4) return null;
  if (at(0, 0x89, 0x50, 0x4e, 0x47)) return 'png';
  if (at(0, 0xff, 0xd8, 0xff)) return 'jpg';
  if (ascii(0, 'GIF8')) return 'gif';
  if (ascii(0, 'RIFF') && ascii(8, 'WEBP')) return 'webp';
  if (ascii(0, 'BM')) return 'bmp';
  if (at(0, 0x49, 0x49, 0x2a, 0x00) || at(0, 0x4d, 0x4d, 0x00, 0x2a)) return 'tiff';
  if (ascii(4, 'ftyp')) {
    const brand = String.fromCharCode(...b.slice(8, 12));
    if (/^avi[fs]$/.test(brand)) return 'avif';
    if (/^(heic|heix|hevc|hevx|mif1|msf1)$/.test(brand)) return 'heic';
  }
  const head = new TextDecoder('utf-8', { fatal: false }).decode(b.slice(0, 256)).trimStart();
  if (/^(<\?xml[\s\S]*?)?<svg[\s>]/i.test(head)) return 'svg';
  return null;
}
