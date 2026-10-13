/**
 * focus-paragraph.ts — which lines 专注模式 keeps lit.
 *
 * The CodeMirror extension (`cm-focus-mode.ts`) dims every line except the
 * paragraph the cursor sits in, where a paragraph is a run of contiguous
 * non-blank lines. Windows without Vim runs the plain <textarea> editor and
 * never loads that extension (#316), so the same rule lives here as a pure
 * function both paths can share — and that the plain path can unit-test
 * without a DOM.
 */

export interface ParagraphRange {
  /** 1-based, inclusive. */
  first: number;
  /** 1-based, inclusive. */
  last: number;
}

/** 1-based line number containing offset `pos` in `text`. */
export function lineAt(text: string, pos: number): number {
  const clamped = Math.max(0, Math.min(pos, text.length));
  let line = 1;
  for (let i = 0; i < clamped; i++) if (text.charCodeAt(i) === 10) line++;
  return line;
}

/**
 * The paragraph (run of contiguous non-blank lines) around the selection
 * `from`..`to`. A selection spanning blank lines keeps everything it touches
 * lit, exactly as the CodeMirror build() does.
 */
export function activeParagraphLines(
  text: string,
  from: number,
  to: number = from,
): ParagraphRange {
  const lines = text.split('\n');
  const isBlank = (n: number) => (lines[n - 1] ?? '').trim().length === 0;
  const lo = Math.min(from, to);
  const hi = Math.max(from, to);
  let first = lineAt(text, lo);
  let last = lineAt(text, hi);
  while (first > 1 && !isBlank(first - 1)) first--;
  while (last < lines.length && !isBlank(last + 1)) last++;
  return { first, last };
}
