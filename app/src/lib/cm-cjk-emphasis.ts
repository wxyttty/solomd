/**
 * CJK-friendly emphasis for the CodeMirror live editor (#262 / Gitee IKA1A0).
 *
 * The preview gets this from `markdown-it-cjk-friendly` (see markdown.ts).
 * Live edit does not go through markdown-it at all — it decorates the
 * @lezer/markdown syntax tree — so without this the same document renders
 * bold in the preview pane and stays literal `**…**` while editing.
 *
 * `**限制：**硬链接` is the shape that breaks: a bold run ending in a
 * full-width colon, immediately followed by a Han character. CommonMark says
 * the closing run isn't right-flanking (preceded by punctuation, followed by
 * a letter), so it can't close. The CommonMark CJK amendment
 * (commonmark/commonmark-spec#650) reads those clauses as *non-CJK*
 * punctuation, which lets it close. ASCII is unaffected.
 *
 * @lezer/markdown has no CJK extension, but a `parseInline` entry whose name
 * matches a built-in replaces it. The delimiter object identity matters
 * downstream — `resolveMarkers` compares against its own EmphasisAsterisk /
 * EmphasisUnderscore singletons to decide whether `**` becomes
 * StrongEmphasis, and those aren't exported — so this delegates to the
 * built-in parser to append the delimiter and then overwrites the open/close
 * flags it computed. `side` is a plain field on the appended part.
 *
 * Known deviation from the amendment: variation-selector sequences (IVS,
 * U+FE00–U+FE0F) are classified by their base character only. The preview
 * handles them exactly; here they'd only affect emphasis directly adjacent
 * to a variant-selected glyph.
 */
import { markdownLanguage } from '@codemirror/lang-markdown';
import type { MarkdownExtension } from '@lezer/markdown';
import { eastAsianWidthType } from 'get-east-asian-width';

const ASTERISK = 42;
const UNDERSCORE = 95;

// CommonMark counts both Unicode punctuation and symbols as "punctuation";
// @lezer/markdown uses the same class.
const PUNCT_RE = /[\p{S}\p{P}]/u;
const EMOJI_PRESENTATION_RE = /^\p{Emoji_Presentation}/u;
const HANGUL_RE = /^\p{sc=Hangul}/u;

/**
 * A "CJK character" per the amendment: East Asian Width W/F/H and not a
 * default-emoji-presentation character, or Hangul script. Ambiguous-width
 * characters are not CJK here (the amendment only treats them as such
 * through variation sequences, which this path doesn't track).
 */
function isCjk(cp: number): boolean {
  if (cp < 0x1100) return false;
  switch (eastAsianWidthType(cp)) {
    case 'fullwidth':
    case 'halfwidth':
      return true;
    case 'wide':
      return !EMOJI_PRESENTATION_RE.test(String.fromCodePoint(cp));
    case 'neutral':
      return HANGUL_RE.test(String.fromCodePoint(cp));
    default:
      return false;
  }
}

function isPunct(cp: number): boolean {
  return cp >= 0 && PUNCT_RE.test(String.fromCodePoint(cp));
}

function isNonCjkPunct(cp: number): boolean {
  return isPunct(cp) && !isCjk(cp);
}

function isSpace(cp: number): boolean {
  // -1 marks the edge of the inline section, which counts as whitespace.
  return cp < 0 || /\s/.test(String.fromCodePoint(cp));
}

/** Code point ending at `pos`, walking back over a surrogate pair. */
function codePointBefore(text: string, pos: number): number {
  if (pos <= 0) return -1;
  const low = text.charCodeAt(pos - 1);
  if (low >= 0xdc00 && low <= 0xdfff && pos >= 2) {
    const high = text.charCodeAt(pos - 2);
    if (high >= 0xd800 && high <= 0xdbff) return (high - 0xd800) * 0x400 + low - 0xdc00 + 0x10000;
  }
  return low;
}

/** Code point starting at `pos`. */
function codePointFrom(text: string, pos: number): number {
  if (pos >= text.length) return -1;
  return text.codePointAt(pos) ?? -1;
}

/** The stock Emphasis inline parser, captured before we shadow it. */
function builtinEmphasisParser(): ((cx: any, next: number, start: number) => number) | null {
  const parser = markdownLanguage.parser as any;
  const idx: number = parser.inlineNames?.indexOf('Emphasis') ?? -1;
  if (idx < 0) return null;
  const fn = parser.inlineParsers?.[idx];
  return typeof fn === 'function' ? fn : null;
}

export const cjkFriendlyEmphasis: MarkdownExtension = {
  parseInline: [
    {
      // Same name as the built-in → replaces it rather than stacking.
      name: 'Emphasis',
      parse(cx: any, next: number, start: number): number {
        if (next !== UNDERSCORE && next !== ASTERISK) return -1;
        const builtin = builtinEmphasisParser();
        if (!builtin) return -1;

        const before = cx.parts.length;
        const result = builtin(cx, next, start);
        if (result < 0) return result;
        // The built-in appends exactly one delimiter part; if that ever
        // stops being true, leave its own flags alone rather than guess.
        if (cx.parts.length !== before + 1) return result;
        const part = cx.parts[before];
        if (!part || typeof part.side !== 'number') return result;

        let pos = start + 1;
        while (cx.char(pos) === next) pos++;

        const text: string = cx.text;
        const offset: number = cx.offset;
        const lastCp = codePointBefore(text, start - offset);
        const nextCp = codePointFrom(text, pos - offset);

        const lastSpace = isSpace(lastCp);
        const nextSpace = isSpace(nextCp);
        const lastNonCjkPunct = isNonCjkPunct(lastCp);
        const nextNonCjkPunct = isNonCjkPunct(nextCp);

        const leftFlanking =
          !nextSpace && (!nextNonCjkPunct || lastSpace || lastNonCjkPunct || isCjk(lastCp));
        const rightFlanking =
          !lastSpace && (!lastNonCjkPunct || nextSpace || nextNonCjkPunct || isCjk(nextCp));

        // `_` still refuses intra-word emphasis; `*` (next === 42) doesn't.
        const canOpen = leftFlanking && (next === ASTERISK || !rightFlanking || isPunct(lastCp));
        const canClose = rightFlanking && (next === ASTERISK || !leftFlanking || isPunct(nextCp));

        part.side = (canOpen ? 1 : 0) | (canClose ? 2 : 0);
        return result;
      },
    },
  ],
};
