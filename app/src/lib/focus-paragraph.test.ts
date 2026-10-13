import { test } from 'node:test';
import assert from 'node:assert/strict';
import { activeParagraphLines, lineAt } from './focus-paragraph.ts';

const doc = 'one\ntwo\n\nthree\nfour\nfive\n\nsix';
//           1    2     3  4      5     6     7   8

test('lineAt counts newlines before the offset', () => {
  assert.equal(lineAt(doc, 0), 1);
  assert.equal(lineAt(doc, 3), 1); // end of "one"
  assert.equal(lineAt(doc, 4), 2); // start of "two"
  assert.equal(lineAt(doc, doc.length), 8);
  assert.equal(lineAt(doc, 9999), 8);
  assert.equal(lineAt(doc, -5), 1);
});

test('a caret expands to its whole paragraph', () => {
  const r = activeParagraphLines(doc, doc.indexOf('four'));
  assert.deepEqual(r, { first: 4, last: 6 });
});

test('a caret on a blank line lights the paragraphs on both sides', () => {
  // Faithful to the CodeMirror extension: it walks up and down from the
  // cursor line while the neighbour is non-blank, so a caret parked in the
  // gap keeps both paragraphs lit rather than dimming the whole document.
  const r = activeParagraphLines(doc, doc.indexOf('\n\n') + 1);
  assert.deepEqual(r, { first: 1, last: 6 });
});

test('a selection keeps every line it touches, blank ones included', () => {
  const r = activeParagraphLines(doc, doc.indexOf('two'), doc.indexOf('three'));
  assert.deepEqual(r, { first: 1, last: 6 });
});

test('the first and last paragraphs do not run off the document', () => {
  assert.deepEqual(activeParagraphLines(doc, 0), { first: 1, last: 2 });
  assert.deepEqual(activeParagraphLines(doc, doc.length), { first: 8, last: 8 });
});

test('an empty document is one lit line', () => {
  assert.deepEqual(activeParagraphLines('', 0), { first: 1, last: 1 });
});
