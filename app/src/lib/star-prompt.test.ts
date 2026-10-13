import assert from 'node:assert/strict';
import { test } from 'node:test';

import { recordDay, shouldPrompt, type StarPromptState } from './star-prompt.ts';

function useOn(days: string[]): StarPromptState {
  let s: StarPromptState | null = null;
  for (const d of days) s = recordDay(s, d);
  return s!;
}

test('not on the first days, however many launches', () => {
  const s = useOn(['2026-10-01', '2026-10-01', '2026-10-01']);
  assert.equal(s.days.length, 1);
  assert.equal(shouldPrompt(s, '2026-10-01'), false);
});

test('after five distinct days of use, at least three days in', () => {
  const s = useOn(['2026-10-01', '2026-10-02', '2026-10-03', '2026-10-05', '2026-10-06']);
  assert.equal(shouldPrompt(s, '2026-10-06'), true);
});

test('five days crammed into a short span is not enough on its own', () => {
  const s: StarPromptState = { first: '2026-10-01', days: ['a', 'b', 'c', 'd', 'e'], shown: false };
  assert.equal(shouldPrompt(s, '2026-10-02'), false);
});

test('once shown, never again', () => {
  const s = { ...useOn(['2026-10-01', '2026-10-02', '2026-10-03', '2026-10-04', '2026-10-05']), shown: true };
  assert.equal(shouldPrompt(recordDay(s, '2026-11-01'), '2026-11-01'), false);
});
