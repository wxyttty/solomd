import assert from 'node:assert/strict';
import { test } from 'node:test';

import { computeListContinuation, insideFence } from './list-continuation.ts';

/** Run Enter at the `|` in `src`; returns the result with `|` at the caret. */
function enter(src: string): string | null {
  const caret = src.indexOf('|');
  const text = src.slice(0, caret) + src.slice(caret + 1);
  const r = computeListContinuation(text, caret, caret);
  if (!r) return null;
  return r.value.slice(0, r.caret) + '|' + r.value.slice(r.caret);
}

test('bullet items continue with the same bullet', () => {
  assert.equal(enter('- one|'), '- one\n- |');
  assert.equal(enter('* one|'), '* one\n* |');
  assert.equal(enter('+ one|'), '+ one\n+ |');
});

test('ordered items increment and keep their delimiter', () => {
  assert.equal(enter('1. one|'), '1. one\n2. |');
  assert.equal(enter('9) nine|'), '9) nine\n10) |');
});

test('nested items keep their indent', () => {
  assert.equal(enter('- a\n  - b|'), '- a\n  - b\n  - |');
  assert.equal(enter('1. a\n   1. b|'), '1. a\n   1. b\n   2. |');
});

test('task items continue unchecked', () => {
  assert.equal(enter('- [x] done|'), '- [x] done\n- [ ] |');
  assert.equal(enter('- [ ] todo|'), '- [ ] todo\n- [ ] |');
});

test('quotes continue, including nested quotes and lists in quotes', () => {
  assert.equal(enter('> q|'), '> q\n> |');
  assert.equal(enter('> > q|'), '> > q\n> > |');
  assert.equal(enter('>q|'), '>q\n> |');
  assert.equal(enter('> - item|'), '> - item\n> - |');
});

test('Enter on an empty item ends the list', () => {
  assert.equal(enter('- one\n- |'), '- one\n|');
  assert.equal(enter('1. one\n2. |'), '1. one\n|');
  assert.equal(enter('- [ ] |'), '|');
  assert.equal(enter('> |'), '|');
  assert.equal(enter('> - |'), '> |');
});

test('Enter in the middle of an item splits it into two items', () => {
  assert.equal(enter('- ab|cd'), '- ab\n- |cd');
});

test('text after the caret on following lines is untouched', () => {
  assert.equal(enter('- a|\n- b\n'), '- a\n- |\n- b\n');
});

test('plain newline cases', () => {
  assert.equal(enter('plain text|'), null);
  assert.equal(enter('|- item'), null, 'caret before the marker');
  assert.equal(enter('-| item'), null, 'caret inside the marker');
  assert.equal(enter('-not a list|'), null);
  assert.equal(enter('2026. a year|').startsWith('2026. a year\n2027. '), true);
  const t = 'x';
  assert.equal(computeListContinuation(t, 0, 1), null, 'a selection');
});

test('lines inside fenced code do not continue', () => {
  assert.equal(enter('```\n- a|'), null);
  assert.equal(enter('~~~md\n1. a|'), null);
  assert.equal(enter('```\ncode\n```\n- a|'), '```\ncode\n```\n- a\n- |');
});

test('insideFence honours fence length and char', () => {
  assert.equal(insideFence('````\n```\n', 9), true);
  assert.equal(insideFence('````\n````\n', 10), false);
  assert.equal(insideFence('```\n~~~\n', 8), true);
});
