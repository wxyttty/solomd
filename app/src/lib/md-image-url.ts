/**
 * Make a local image path safe to use as a Markdown link destination.
 *
 * CommonMark does not allow spaces in a bare destination, so
 * `![](D:/Program Files/img.png)` is not an image at all: markdown-it renders
 * the whole thing as literal text (#345). The same happens with an unbalanced
 * `(` or `)`. Every image SoloMD writes into a note — pasted, dropped,
 * uploaded-with-local-fallback, or inserted from a file picker — goes through
 * this function, so an image folder under "Program Files", a note called
 * "My Note" (→ `My Note.assets/`), or a Windows user name with a space all
 * produce links that render.
 *
 * Percent-encoding rather than `<...>`: the rendered `src` is identical either
 * way, but the live-edit image-line detector and other editors (Typora writes
 * `%20` too) only understand the encoded form. Every place that turns a
 * rendered `src` back into a file path decodes it exactly once
 * (`image-resolve.ts`), which is why `%` itself is encoded as well: a file
 * literally named `100%.png` must come back as `100%.png`, not be mangled by
 * that decode.
 *
 * Only characters that break the destination are touched; letters, CJK and
 * `/` stay readable. A remote URL is already URL-encoded, so only its
 * whitespace is touched (an image host can hand back a key with a space in it).
 */
export function encodeImageDestination(path: string): string {
  if (/^(https?|data|blob|asset|tauri):/i.test(path)) return path.replace(/\s/g, '%20');
  return path
    .replace(/%/g, '%25')
    .replace(/\s/g, (c) => (c === ' ' ? '%20' : encodeURIComponent(c)))
    .replace(/\(/g, '%28')
    .replace(/\)/g, '%29')
    .replace(/</g, '%3C')
    .replace(/>/g, '%3E');
}

/** `![alt](path)` with the path made safe by {@link encodeImageDestination}. */
export function markdownImage(path: string, alt = ''): string {
  return `![${alt}](${encodeImageDestination(path)})`;
}
