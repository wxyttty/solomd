/**
 * iOS moves an app's data container to a new UUID path on every update
 * (…/Data/Application/<UUID>/…), and installing a TestFlight build over the
 * App Store one counts. Every absolute path we persisted — the workspace
 * folder, recent folders and files, the per-folder tab sessions (whose
 * localStorage KEYS embed the folder) — then points at a container that no
 * longer exists, and the file tree reports "this folder is no longer where it
 * was" for the app's own Documents.
 *
 * openPath already re-anchors single file paths; this does the same for all
 * of our stored state, once, before the stores load. Paths outside the app's
 * container (iCloud Drive, other apps' folders) do not contain the pattern and
 * are left alone.
 */
import { documentDir } from '@tauri-apps/api/path';
import { isIOS } from './platform';

const CONTAINER = /\/Data\/Application\/([0-9A-Fa-f-]{36})(?=\/|$)/g;

export function reanchorStorage(storage: Storage, currentUuid: string): number {
  const fix = (s: string) =>
    s.replace(CONTAINER, (m, uuid: string) =>
      uuid.toUpperCase() === currentUuid.toUpperCase() ? m : `/Data/Application/${currentUuid}`,
    );
  const keys: string[] = [];
  for (let i = 0; i < storage.length; i++) {
    const k = storage.key(i);
    if (k && k.startsWith('solomd.')) keys.push(k);
  }
  let changed = 0;
  for (const key of keys) {
    const value = storage.getItem(key);
    if (value == null) continue;
    const newKey = fix(key);
    const newValue = fix(value);
    if (newKey === key && newValue === value) continue;
    if (newKey !== key) {
      storage.removeItem(key);
      // A session already saved under the current container wins over a
      // stale one being carried forward.
      if (storage.getItem(newKey) != null) continue;
    }
    storage.setItem(newKey, newValue);
    changed++;
  }
  if (changed) dedupeRecents(storage);
  return changed;
}

/** The old and current container paths now coincide — drop the duplicates
 *  from the recent lists (the folder switcher showed "Documents" twice). */
function dedupeRecents(storage: Storage) {
  const raw = storage.getItem('solomd.workspace.v1');
  if (!raw) return;
  try {
    const ws = JSON.parse(raw);
    for (const k of ['recentFolders', 'recentFiles']) {
      if (Array.isArray(ws[k])) ws[k] = [...new Set(ws[k])];
    }
    storage.setItem('solomd.workspace.v1', JSON.stringify(ws));
  } catch {
    /* unparseable — the store falls back to defaults on its own */
  }
}

export async function reanchorIosContainerPaths(): Promise<void> {
  if (!isIOS()) return;
  const docs = await documentDir();
  const m = /\/Data\/Application\/([0-9A-Fa-f-]{36})(?=\/|$)/.exec(docs);
  if (!m) return;
  reanchorStorage(localStorage, m[1]);
}
