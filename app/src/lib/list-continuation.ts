/**
 * Markdown list / quote continuation on Enter for the plain-textarea editors
 * (Windows, #341). CodeMirror gets the same behaviour from
 * `insertNewlineContinueMarkup`; this is the textarea equivalent, kept pure so
 * both plain paths (flat textarea and live-edit blocks) share one rule.
 *
 *   - `- item|`        → `- item\n- |`
 *   - `3. item|`       → `3. item\n4. |`   (`)` delimiters kept)
 *   - `- [x] done|`    → `- [x] done\n- [ ] |`
 *   - `> > quote|`     → `> > quote\n> > |`
 *   - `> - item|`      → `> - item\n> - |`
 *   - Enter on an empty item removes that item's marker (ends the list /
 *     quote), leaving the line empty — the second Enter of "Enter, Enter".
 *
 * Returns null whenever a plain newline is the right answer: a selection, the
 * caret inside the marker itself, a line that is not a list/quote, or a line
 * inside a fenced code block.
 */

export interface ContinuationEdit {
  value: string;
  caret: number;
}

// Leading indent + any number of `>` quote markers (each with optional space).
const QUOTE_PREFIX = /^([ \t]*(?:>[ \t]?)*)/;
// A list marker right after the quote prefix: bullet or ordered, then an
// optional task box.
const LIST_MARKER = /^([ \t]*)(?:([-*+])|(\d{1,9})([.)]))([ \t]+)(\[[ xX]\][ \t]+)?/;

/** True when `lineStart` sits inside an open ``` / ~~~ fence of `text`. */
export function insideFence(text: string, lineStart: number): boolean {
  let open: { ch: string; len: number } | null = null;
  const before = text.slice(0, lineStart);
  for (const line of before.split('\n')) {
    const m = line.match(/^ {0,3}(`{3,}|~{3,})(.*)$/);
    if (!m) continue;
    const ch = m[1][0];
    const len = m[1].length;
    if (!open) {
      // A backtick opener's info string may not contain backticks.
      if (ch === '`' && m[2].includes('`')) continue;
      open = { ch, len };
    } else if (ch === open.ch && len >= open.len && m[2].trim() === '') {
      open = null;
    }
  }
  return open !== null;
}

export function computeListContinuation(
  text: string,
  selStart: number,
  selEnd: number,
): ContinuationEdit | null {
  if (selStart !== selEnd) return null;
  const caret = selStart;
  const lineStart = text.lastIndexOf('\n', caret - 1) + 1;
  const nl = text.indexOf('\n', caret);
  const lineEnd = nl < 0 ? text.length : nl;
  const line = text.slice(lineStart, lineEnd);

  if (insideFence(text, lineStart)) return null;

  const quote = line.match(QUOTE_PREFIX)?.[1] ?? '';
  const hasQuote = quote.includes('>');
  const rest = line.slice(quote.length);
  const list = rest.match(LIST_MARKER);

  let markerLen: number; // length of the whole marker on this line
  let nextMarker: string; // marker for the new line
  if (list) {
    const [whole, indent, bullet, num, delim, gap, task] = list;
    markerLen = quote.length + whole.length;
    // Normalise the gap after the marker to one space unless it was a
    // deliberate multi-space alignment (keep it then — it is the item's
    // content column).
    const sep = gap.includes('\t') ? gap : gap.length > 4 ? ' ' : gap;
    const head = bullet ?? `${Number(num) + 1}${delim}`;
    nextMarker = `${quote}${indent}${head}${sep}${task ? '[ ] ' : ''}`;
  } else if (hasQuote) {
    markerLen = quote.length;
    // `>` without a trailing space still continues as `> `.
    nextMarker = /[ \t]$/.test(quote) ? quote : `${quote} `;
  } else {
    return null;
  }

  // Caret inside (or before) the marker: splitting there would duplicate it.
  if (caret - lineStart < markerLen) return null;

  const content = line.slice(markerLen);
  if (content.trim() === '' && caret === lineEnd) {
    // Empty item → drop the marker and end the list here. For a list inside a
    // quote, keep the quote (`> - ` → `> `); a bare quote line ends the quote.
    const keep = list && hasQuote ? (/[ \t]$/.test(quote) ? quote : `${quote} `) : '';
    const value = text.slice(0, lineStart) + keep + text.slice(lineEnd);
    return { value, caret: lineStart + keep.length };
  }

  const insert = `\n${nextMarker}`;
  return { value: text.slice(0, caret) + insert + text.slice(caret), caret: caret + insert.length };
}
