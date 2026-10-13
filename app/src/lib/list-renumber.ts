/**
 * Keep ordered-list numbers consecutive in the source after an edit — delete
 * `2. second` from `1. 2. 3.` and the rest becomes `1. 2.`, as in Typora and
 * MarkText. Rendering never needed this (Markdown numbers from the first
 * item); the source did, and every editor path shares this one rule:
 * CodeMirror through a transaction filter, the Windows textarea editors at
 * their three commit points.
 *
 * Only edits that add or remove a line break trigger it (`structuralEditRange`).
 * Typing inside a marker never does, so changing `10.` to `11.` digit by digit
 * isn't fought over halfway through.
 *
 * Lists written lazily (`1. 1. 1.`) are left alone, and the first item's
 * number is kept as the start.
 */

export interface TextChange {
  from: number;
  to: number;
  insert: string;
}

/**
 * The span of `next` touched by the edit that turned `prev` into it, or null
 * when that edit neither added nor removed a newline.
 */
export function structuralEditRange(prev: string, next: string): { from: number; to: number } | null {
  if (prev === next) return null;
  const max = Math.min(prev.length, next.length);
  let p = 0;
  while (p < max && prev.charCodeAt(p) === next.charCodeAt(p)) p++;
  let s = 0;
  while (
    s < max - p &&
    prev.charCodeAt(prev.length - 1 - s) === next.charCodeAt(next.length - 1 - s)
  ) {
    s++;
  }
  const removed = prev.slice(p, prev.length - s);
  const inserted = next.slice(p, next.length - s);
  if (!removed.includes('\n') && !inserted.includes('\n')) return null;
  return { from: p, to: next.length - s };
}

interface Line {
  start: number;
  end: number;
  /** Number of `>` quote markers in front. */
  quote: number;
  /** Indent width after the quote markers (tab = 4). */
  indent: number;
  blank: boolean;
  /** Any list marker (bullet or ordered) at this line's indent. */
  listMarker: boolean;
  item: { num: number; delim: string; numFrom: number; numTo: number } | null;
}

const QUOTE = /^((?:[ \t]*>[ \t]?)*)/;
const ORDERED = /^([ \t]*)(\d{1,9})([.)])(?=[ \t]|$)/;
const BULLET = /^[ \t]*[-*+](?=[ \t]|$)/;
const FENCE = /^ {0,3}(`{3,}|~{3,})/;

function indentWidth(ws: string): number {
  let w = 0;
  for (const ch of ws) w += ch === '\t' ? 4 - (w % 4) : 1;
  return w;
}

function parseLines(text: string): Line[] {
  const out: Line[] = [];
  let fence: string | null = null;
  let start = 0;
  while (start <= text.length) {
    const nl = text.indexOf('\n', start);
    const end = nl < 0 ? text.length : nl;
    const raw = text.slice(start, end);
    const quoteStr = raw.match(QUOTE)?.[1] ?? '';
    const rest = raw.slice(quoteStr.length);
    const quote = (quoteStr.match(/>/g) ?? []).length;
    const ws = rest.match(/^[ \t]*/)?.[0] ?? '';
    const line: Line = {
      start,
      end,
      quote,
      indent: indentWidth(ws),
      blank: rest.trim() === '',
      listMarker: false,
      item: null,
    };
    const f = rest.match(FENCE);
    if (fence) {
      // Inside a code block nothing is a list item.
      if (f && f[1][0] === fence[0] && f[1].length >= fence.length) fence = null;
    } else if (f) {
      fence = f[1];
    } else {
      const m = rest.match(ORDERED);
      if (m) {
        const numFrom = start + quoteStr.length + m[1].length;
        line.item = { num: Number(m[2]), delim: m[3], numFrom, numTo: numFrom + m[2].length };
        line.listMarker = true;
      } else if (BULLET.test(rest)) {
        line.listMarker = true;
      }
    }
    out.push(line);
    if (nl < 0) break;
    start = nl + 1;
  }
  return out;
}

/**
 * Renumber every ordered list overlapping `[from, to]` (offsets in `text`,
 * widened by a line on each side — deleting an item joins its neighbours).
 * Returns the changes in ascending order, or an empty array.
 */
export function renumberChanges(text: string, from: number, to: number): TextChange[] {
  const lines = parseLines(text);
  const lineAt = (pos: number) => {
    let lo = 0;
    let hi = lines.length - 1;
    while (lo < hi) {
      const mid = (lo + hi + 1) >> 1;
      if (lines[mid].start <= pos) lo = mid;
      else hi = mid - 1;
    }
    return lo;
  };
  const firstLine = Math.max(0, lineAt(from) - 1);
  const lastLine = Math.min(lines.length - 1, lineAt(to) + 1);

  const assigned = new Set<number>();
  const changes: TextChange[] = [];
  for (let i = 0; i < lines.length; i++) {
    const head = lines[i];
    if (!head.item || assigned.has(i)) continue;
    const items: number[] = [i];
    let last = i;
    let prevBlank = false;
    for (let j = i + 1; j < lines.length; j++) {
      const l = lines[j];
      if (l.quote !== head.quote) break;
      if (l.item && l.indent === head.indent) {
        if (l.item.delim !== head.item.delim) break;
        items.push(j);
        last = j;
        prevBlank = false;
        continue;
      }
      if (l.blank) {
        prevBlank = true;
        continue;
      }
      // Child content (nested lists, continuation paragraphs, code).
      if (l.indent > head.indent) {
        last = j;
        prevBlank = false;
        continue;
      }
      // A lazy continuation line right under item text keeps the list going.
      if (!prevBlank && !l.listMarker) {
        last = j;
        continue;
      }
      break;
    }
    for (const k of items) assigned.add(k);
    if (last < firstLine || i > lastLine || items.length < 2) continue;
    const nums = items.map((k) => lines[k].item!.num);
    if (nums.every((n) => n === nums[0])) continue;
    nums.forEach((n, idx) => {
      const want = nums[0] + idx;
      if (n === want) return;
      const it = lines[items[idx]].item!;
      changes.push({ from: it.numFrom, to: it.numTo, insert: String(want) });
    });
  }
  return changes.sort((a, b) => a.from - b.from);
}

/** Apply ascending, non-overlapping changes to `text`. */
export function applyChanges(text: string, changes: TextChange[]): string {
  let out = '';
  let pos = 0;
  for (const c of changes) {
    out += text.slice(pos, c.from) + c.insert;
    pos = c.to;
  }
  return out + text.slice(pos);
}

/** Where `pos` lands after `changes` are applied. */
export function mapPos(pos: number, changes: TextChange[]): number {
  let delta = 0;
  for (const c of changes) {
    if (c.to <= pos) delta += c.insert.length - (c.to - c.from);
    else if (c.from < pos) return c.from + c.insert.length + delta;
  }
  return pos + delta;
}

/**
 * The textarea editors' entry point: given the document before and after an
 * edit, the document to store instead (or null when nothing needs renumbering).
 */
export function renumberAfterEdit(
  prev: string,
  next: string,
): { value: string; changes: TextChange[] } | null {
  const range = structuralEditRange(prev, next);
  if (!range) return null;
  const changes = renumberChanges(next, range.from, range.to);
  if (!changes.length) return null;
  return { value: applyChanges(next, changes), changes };
}
