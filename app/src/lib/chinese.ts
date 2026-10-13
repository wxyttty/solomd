/**
 * Chinese power-user text utilities.
 *
 * Pure functions: no stores, no DOM, no I/O. Safe to import from anywhere.
 *
 * - CJK-aware word counting that handles mixed Chinese/Japanese/Korean +
 *   ASCII content sensibly.
 *
 * Deliberately dependency-free: this module is on the startup path (status
 * bar, pomodoro, writing goals). Simplified/Traditional conversion and pinyin
 * live in `chinese-convert.ts`, which is loaded on demand.
 */

/**
 * Count CJK + ASCII words in `text`.
 *
 * `cjk` counts individual ideographs / kana / hangul syllables (each
 * "character" is one word in CJK writing systems). `asciiWords` runs a
 * whitespace split on whatever is left after stripping CJK. `total` is a
 * sane mixed-content word count that usually matches what people expect
 * to see in a status-bar "word count" indicator.
 */
export function cjkWordCount(text: string): {
  cjk: number;
  asciiWords: number;
  total: number;
  chars: number;
  withSpaces: number;
} {
  if (!text) {
    return { cjk: 0, asciiWords: 0, total: 0, chars: 0, withSpaces: 0 };
  }

  // CJK Unified Ideographs (incl. Ext A) + Japanese kana + Korean Hangul syllables.
  const cjkRe = /[\u3400-\u9fff\u3040-\u30ff\uac00-\ud7af]/g;
  const cjkMatches = text.match(cjkRe);
  const cjk = cjkMatches ? cjkMatches.length : 0;

  // Strip the CJK characters, then whitespace-split the remainder for ASCII
  // "word" counting. Tokens that are nothing but punctuation/symbols are
  // filtered out — a real word needs at least one letter or digit.
  const nonCjk = text.replace(cjkRe, ' ');
  const asciiWords = nonCjk
    .split(/\s+/)
    .filter((w) => w.length > 0 && /[\p{L}\p{N}]/u.test(w)).length;

  const withSpaces = text.length;
  const chars = text.replace(/\s+/g, '').length;

  return {
    cjk,
    asciiWords,
    total: cjk + asciiWords,
    chars,
    withSpaces,
  };
}
