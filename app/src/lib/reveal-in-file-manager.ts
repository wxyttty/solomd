import { openPath, revealItemInDir } from '@tauri-apps/plugin-opener';

/** The folder that holds `path`, or null for a bare name / a volume root. */
export function parentDirOf(path: string): string | null {
  const trimmed = path.replace(/[\\/]+$/, '');
  const i = Math.max(trimmed.lastIndexOf('/'), trimmed.lastIndexOf('\\'));
  if (i <= 0) return i === 0 ? trimmed.slice(0, 1) : null;
  const parent = trimmed.slice(0, i);
  // `C:` alone is a drive-relative path, not the root of the drive.
  return /^[A-Za-z]:$/.test(parent) ? parent + '\\' : parent;
}

/**
 * Show `path` in Finder / Explorer / the Linux file manager (#333).
 *
 * `revealItemInDir` selects the item, but it can fail where the shell does
 * not cooperate (Explorer's SHOpenFolderAndSelectItems, a Linux session
 * without org.freedesktop.FileManager1). Opening the parent folder is the
 * next best thing, so fall back to that before giving up. Throws the original
 * reveal error when both fail, so the caller can tell the user instead of
 * the menu item silently doing nothing.
 */
export async function revealInFileManager(path: string): Promise<void> {
  try {
    await revealItemInDir(path);
  } catch (revealErr) {
    const parent = parentDirOf(path);
    if (parent) {
      try {
        await openPath(parent);
        return;
      } catch {
        /* report the reveal error below — it is the one that explains why */
      }
    }
    throw revealErr;
  }
}
