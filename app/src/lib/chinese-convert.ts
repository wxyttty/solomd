/**
 * Simplified/Traditional conversion and pinyin — the parts of the Chinese
 * toolkit that carry dictionaries.
 *
 * Split out of `chinese.ts` because opencc-js and pinyin-pro together are
 * megabytes of dictionary data, and `chinese.ts` is imported by the status
 * bar, the pomodoro store and the writing-goals composable — i.e. by the
 * startup path. Word counting needs no dictionary; these three do, and they
 * only ever run from a command the user invokes.
 */
// opencc-js exports are synchronous: `Converter({ from, to })` returns a
// plain `(s: string) => string`. The library does a bit of dictionary
// parsing on construction though, so we cache instances per direction.
// @ts-ignore — opencc-js ships no TypeScript declarations
import * as OpenCC from 'opencc-js';
import { pinyin as pinyinFn } from 'pinyin-pro';

type OpenCCConverter = (s: string) => string;

let s2tConverter: OpenCCConverter | null = null;
let t2sConverter: OpenCCConverter | null = null;

function getS2T(): OpenCCConverter {
  if (!s2tConverter) {
    // cn -> tw gives a fuller Traditional conversion (incl. phrase-level
    // substitutions) than hk or plain `t`.
    s2tConverter = (OpenCC as any).Converter({ from: 'cn', to: 'tw' }) as OpenCCConverter;
  }
  return s2tConverter;
}

function getT2S(): OpenCCConverter {
  if (!t2sConverter) {
    t2sConverter = (OpenCC as any).Converter({ from: 'tw', to: 'cn' }) as OpenCCConverter;
  }
  return t2sConverter;
}

/** Convert Simplified Chinese text to Traditional. Non-Chinese passes through. */
export function simplifiedToTraditional(text: string): string {
  if (!text) return '';
  return getS2T()(text);
}

/** Convert Traditional Chinese text to Simplified. Non-Chinese passes through. */
export function traditionalToSimplified(text: string): string {
  if (!text) return '';
  return getT2S()(text);
}

/**
 * Convert Hanzi in `text` to pinyin. Non-Chinese characters pass through.
 *
 * Defaults: no tone marks, lowercase, space-separated.
 */
export function pinyin(
  text: string,
  opts: { tone?: boolean; separator?: string } = {}
): string {
  if (!text) return '';
  const { tone = false, separator = ' ' } = opts;
  const result = pinyinFn(text, {
    toneType: tone ? 'symbol' : 'none',
    type: 'string',
    separator,
  });
  return typeof result === 'string' ? result.toLowerCase() : String(result).toLowerCase();
}

