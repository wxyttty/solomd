import assert from 'node:assert/strict';
import { test } from 'node:test';

import { parentDirOf } from './reveal-in-file-manager.ts';

test('unix file → its folder', () => {
  assert.equal(parentDirOf('/Users/a/notes/x.md'), '/Users/a/notes');
});

test('windows file → its folder', () => {
  assert.equal(parentDirOf('D:\\TYYSTUDY\\SoloMD笔记库\\00 需求问题管理\\untitled.md'), 'D:\\TYYSTUDY\\SoloMD笔记库\\00 需求问题管理');
});

test('file at a drive root → the drive root, not the drive-relative "D:"', () => {
  assert.equal(parentDirOf('D:\\x.md'), 'D:\\');
  assert.equal(parentDirOf('D:/x.md'), 'D:\\');
});

test('file at the unix root → /', () => {
  assert.equal(parentDirOf('/x.md'), '/');
});

test('trailing separators are ignored', () => {
  assert.equal(parentDirOf('/a/b/'), '/a');
});

test('bare name has no parent', () => {
  assert.equal(parentDirOf('x.md'), null);
});
