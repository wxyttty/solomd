import assert from 'node:assert/strict';
import { test } from 'node:test';

import { sortEntries, reorderedNames, renameInOrder, removeFromOrder, sortNeedsTimes } from './tree-sort.ts';

const e = (name: string, is_dir = false, created?: number, modified?: number) => ({ name, is_dir, created, modified });
const names = (xs: { name: string }[]) => xs.map((x) => x.name);

const sample = [
  e('b.md', false, 300, 900),
  e('note 10.md', false, 100, 100),
  e('Z-folder', true, 50, 50),
  e('a.md', false, 200, 950),
  e('note 2.md', false, 400, 400),
  e('A-folder', true, 60, 60),
];

test('name ascending: folders first, natural and case-insensitive', () => {
  assert.deepEqual(names(sortEntries(sample, 'name-asc')), [
    'A-folder', 'Z-folder', 'a.md', 'b.md', 'note 2.md', 'note 10.md',
  ]);
});

test('name descending keeps folders first', () => {
  assert.deepEqual(names(sortEntries(sample, 'name-desc')), [
    'Z-folder', 'A-folder', 'note 10.md', 'note 2.md', 'b.md', 'a.md',
  ]);
});

test('created ascending puts the newest file last (#342)', () => {
  assert.deepEqual(names(sortEntries(sample, 'created-asc')), [
    'Z-folder', 'A-folder', 'note 10.md', 'a.md', 'b.md', 'note 2.md',
  ]);
});

test('created descending and modified descending', () => {
  assert.deepEqual(names(sortEntries(sample, 'created-desc')).slice(2), ['note 2.md', 'b.md', 'a.md', 'note 10.md']);
  assert.deepEqual(names(sortEntries(sample, 'modified-desc')).slice(2), ['a.md', 'b.md', 'note 2.md', 'note 10.md']);
});

test('missing times sort after known ones, by name', () => {
  const xs = [e('c.md'), e('b.md', false, 5), e('a.md')];
  assert.deepEqual(names(sortEntries(xs, 'created-desc')), ['b.md', 'a.md', 'c.md']);
  assert.deepEqual(names(sortEntries(xs, 'created-asc')), ['b.md', 'a.md', 'c.md']);
});

test('manual: listed names keep their order, new files follow by creation', () => {
  const order = ['note 2.md', 'a.md'];
  assert.deepEqual(names(sortEntries(sample, 'manual', order)).slice(2), ['note 2.md', 'a.md', 'note 10.md', 'b.md']);
});

test('reorderedNames moves within a group and refuses cross-group or no-op moves', () => {
  const cur = sortEntries(sample, 'name-asc');
  assert.deepEqual(reorderedNames(cur, 'note 10.md', 'a.md', 'before'), [
    'A-folder', 'Z-folder', 'note 10.md', 'a.md', 'b.md', 'note 2.md',
  ]);
  assert.equal(reorderedNames(cur, 'a.md', 'Z-folder', 'after'), null);
  assert.equal(reorderedNames(cur, 'a.md', 'b.md', 'before'), null);
  assert.equal(reorderedNames(cur, 'a.md', 'a.md', 'after'), null);
});

test('rename / remove bookkeeping and which modes need times', () => {
  assert.deepEqual(renameInOrder(['x', 'y'], 'y', 'z'), ['x', 'z']);
  assert.deepEqual(removeFromOrder(['x', 'y'], 'x'), ['y']);
  assert.equal(sortNeedsTimes('name-asc'), false);
  assert.equal(sortNeedsTimes('created-asc'), true);
  assert.equal(sortNeedsTimes('manual'), true);
});
