/**
 * Where the next new file should be saved.
 *
 * "New file" used to open the OS save dialog wherever that dialog happened to
 * remember last — unrelated to what the user was looking at, so a note meant
 * for `00 需求问题管理/` could land next to the vault root or in whatever
 * folder was saved to an hour earlier. The Explorer publishes its current
 * selection here instead (a folder → the folder itself, a file → the file's
 * folder) and the file layer uses it as the dialog's starting folder.
 *
 * Deliberately plain module state, not a store or a ref: nothing renders it,
 * the Explorer owns the visual side. Session-only by design — a destination
 * remembered from last week is a folder the user has forgotten about, and
 * there is already a sane fallback chain (workspace → Desktop → Documents →
 * home) for the "no selection yet" case.
 */

export interface TreeSelection {
  path: string;
  /** True when the row is a folder, i.e. the path is the destination itself. */
  isDir: boolean;
}

/** Folder containing `path`, or null when there is none to speak of. */
export function parentDirOf(path: string): string | null {
  const m = path.match(/^(.*)[\\/][^\\/]+$/);
  // An empty prefix means "the root of the volume" (a POSIX `/a.md`, a bare
  // `C:`); that is not a folder we can offer as a save destination, so report
  // it the same way as "no parent at all".
  return m && m[1] ? m[1] : null;
}

/**
 * The folder a new file belongs in.
 *
 * A selected folder wins, then the folder of the selected file, then the
 * folder of the open document (the common case: reading a note, pressing
 * Ctrl+N). Null when none of them is known — the caller falls back to its own
 * chain.
 */
export function newFileDirFor(
  selection: TreeSelection | null | undefined,
  activeFilePath: string | null | undefined,
): string | null {
  if (selection) {
    const dir = selection.isDir ? selection.path : parentDirOf(selection.path);
    if (dir) return dir;
  }
  return activeFilePath ? parentDirOf(activeFilePath) : null;
}

let selection: TreeSelection | null = null;

/** Called by the Explorer whenever the user's selection changes. */
export function setTreeSelection(sel: TreeSelection | null): void {
  selection = sel;
}

export function treeSelection(): TreeSelection | null {
  return selection;
}
