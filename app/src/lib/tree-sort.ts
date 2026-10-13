/**
 * #342 — how the file tree orders a folder's children.
 *
 * Folders always come before files, whatever the mode; every mode below
 * applies within each group. Alphabetical was the only order until a user
 * pointed out that a file they had just created landed in the middle of the
 * list instead of at the end where they were looking for it.
 */

export type TreeSortMode =
  | 'name-asc'
  | 'name-desc'
  | 'created-asc'
  | 'created-desc'
  | 'modified-desc'
  | 'manual';

export const TREE_SORT_MODES: TreeSortMode[] = [
  'name-asc',
  'name-desc',
  'created-asc',
  'created-desc',
  'modified-desc',
  'manual',
];

export interface SortableEntry {
  name: string;
  is_dir: boolean;
  modified?: number;
  created?: number;
}

/** Whether the mode needs `list_dir` to stat each entry for its times. */
export function sortNeedsTimes(mode: TreeSortMode): boolean {
  return mode === 'created-asc' || mode === 'created-desc' || mode === 'modified-desc' || mode === 'manual';
}

export function isTreeSortMode(v: unknown): v is TreeSortMode {
  return typeof v === 'string' && (TREE_SORT_MODES as string[]).includes(v);
}

// Natural order: "note 2" before "note 10", case-insensitive, and Chinese
// names compared by the locale rather than by code point.
const collator = new Intl.Collator(undefined, { numeric: true, sensitivity: 'base' });
const byName = (a: SortableEntry, b: SortableEntry) => collator.compare(a.name, b.name);

/** Missing times (a SAF listing, a stat that failed) sort after known ones in
 *  both directions, then by name, so the order is still stable. */
function byTime(key: 'created' | 'modified', dir: 1 | -1) {
  return (a: SortableEntry, b: SortableEntry) => {
    const ta = a[key];
    const tb = b[key];
    if (ta == null && tb == null) return byName(a, b);
    if (ta == null) return 1;
    if (tb == null) return -1;
    return ta === tb ? byName(a, b) : (ta - tb) * dir;
  };
}

/**
 * Manual order: names listed in `order` keep that sequence; anything not in
 * it (created since, or never placed) follows in creation order, so a new
 * file appears at the end, which is what the reporter expected to happen.
 */
function byManual(order: string[]) {
  const rank = new Map(order.map((n, i) => [n, i]));
  const created = byTime('created', 1);
  return (a: SortableEntry, b: SortableEntry) => {
    const ra = rank.get(a.name);
    const rb = rank.get(b.name);
    if (ra != null && rb != null) return ra - rb;
    if (ra != null) return -1;
    if (rb != null) return 1;
    return created(a, b);
  };
}

export function sortEntries<T extends SortableEntry>(
  entries: T[],
  mode: TreeSortMode,
  manualOrder: string[] = [],
): T[] {
  const cmp =
    mode === 'name-desc'
      ? (a: T, b: T) => -byName(a, b)
      : mode === 'created-asc'
        ? byTime('created', 1)
        : mode === 'created-desc'
          ? byTime('created', -1)
          : mode === 'modified-desc'
            ? byTime('modified', -1)
            : mode === 'manual'
              ? byManual(manualOrder)
              : byName;
  const dirs = entries.filter((e) => e.is_dir).sort(cmp);
  const files = entries.filter((e) => !e.is_dir).sort(cmp);
  return [...dirs, ...files];
}

/**
 * Move `name` before or after `target` within a folder's current visible
 * order, and return the new full order for that folder. Folders and files
 * are reordered within their own group only, so the result keeps folders
 * first. Returns null when the move is a no-op.
 */
export function reorderedNames(
  current: SortableEntry[],
  name: string,
  target: string,
  pos: 'before' | 'after',
): string[] | null {
  if (name === target) return null;
  const moving = current.find((e) => e.name === name);
  const anchor = current.find((e) => e.name === target);
  if (!moving || !anchor || moving.is_dir !== anchor.is_dir) return null;
  const names = current.map((e) => e.name).filter((n) => n !== name);
  const at = names.indexOf(target);
  names.splice(pos === 'before' ? at : at + 1, 0, name);
  const before = current.map((e) => e.name);
  return names.join('\u0000') === before.join('\u0000') ? null : names;
}

/** Manual-order bookkeeping when a child is renamed or leaves the folder. */
export function renameInOrder(order: string[], from: string, to: string): string[] {
  return order.map((n) => (n === from ? to : n));
}
export function removeFromOrder(order: string[], name: string): string[] {
  return order.filter((n) => n !== name);
}
