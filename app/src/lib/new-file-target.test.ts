import assert from 'node:assert/strict';
import { test } from 'node:test';

// Explicit .ts: `node --test` loads this module graph directly and Node's ESM
// resolver will not guess an extension (tsconfig has allowImportingTsExtensions).
import { newFileDirFor, parentDirOf, setTreeSelection, treeSelection } from './new-file-target.ts';

// ---- parentDirOf --------------------------------------------------------

test('parentDirOf takes the folder off both separator styles', () => {
  assert.equal(parentDirOf('C:\\vault\\00 需求\\note.md'), 'C:\\vault\\00 需求');
  assert.equal(parentDirOf('/home/me/vault/note.md'), '/home/me/vault');
});

test('parentDirOf has nothing to offer for a bare file name', () => {
  assert.equal(parentDirOf('note.md'), null);
  assert.equal(parentDirOf(''), null);
});

test('parentDirOf does not mistake a volume root for a folder', () => {
  // "" would be handed to the save dialog as a start folder, which means
  // nothing anywhere; the caller must fall back instead.
  assert.equal(parentDirOf('/note.md'), null);
  assert.equal(parentDirOf('C:\\note.md'), 'C:');
});

// ---- newFileDirFor ------------------------------------------------------

test('a selected folder is the destination itself', () => {
  assert.equal(newFileDirFor({ path: 'C:\\vault\\01 测试文件夹', isDir: true }, null), 'C:\\vault\\01 测试文件夹');
});

test('a selected file sends the new file to its own folder', () => {
  assert.equal(
    newFileDirFor({ path: 'C:\\vault\\00 需求\\note.md', isDir: false }, null),
    'C:\\vault\\00 需求',
  );
});

test('with no selection the open document decides', () => {
  assert.equal(newFileDirFor(null, '/vault/sub/note.md'), '/vault/sub');
  // The document wins over nothing at all, and an untitled tab (no path) has
  // no opinion — the caller falls back to its own chain.
  assert.equal(newFileDirFor(null, null), null);
  assert.equal(newFileDirFor(null, undefined), null);
});

test('a selection the tree cannot resolve still beats the open document', () => {
  // Home folder / root paths have no usable parent: fall through to the
  // document rather than handing the dialog an empty string.
  assert.equal(newFileDirFor({ path: '/note.md', isDir: false }, '/vault/sub/other.md'), '/vault/sub');
});

// ---- the shared slot ----------------------------------------------------

test('the tree selection slot round-trips and clears', () => {
  setTreeSelection({ path: 'C:\\vault\\01 测试文件夹', isDir: true });
  assert.deepEqual(treeSelection(), { path: 'C:\\vault\\01 测试文件夹', isDir: true });
  setTreeSelection(null);
  assert.equal(treeSelection(), null);
});
