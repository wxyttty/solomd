/**
 * Settings search (#352).
 *
 * The settings panel is filtered by what is actually rendered: each
 * top-level block of the panel is matched against its own text, so there is
 * no second list of settings to keep in step with the template.
 *
 * The one thing the rendered text can't answer is an English keyword typed
 * into a translated UI ("IME", "vim", "PDF" while the app is in Chinese).
 * For that, the query is also looked up in the English dictionary: every
 * English string that contains it contributes the *translated* string under
 * the same key as an extra needle. A block matches if it contains the query
 * or any of those needles.
 */

export function normalize(s: string): string {
  return s.toLowerCase().replace(/\s+/g, ' ').trim();
}

/** Every string leaf of a nested dictionary, keyed by its dotted path. */
export function flatten(dict: unknown, prefix = '', out = new Map<string, string>()): Map<string, string> {
  if (dict && typeof dict === 'object') {
    for (const [k, v] of Object.entries(dict as Record<string, unknown>)) {
      const key = prefix ? `${prefix}.${k}` : k;
      if (typeof v === 'string') out.set(key, v);
      else flatten(v, key, out);
    }
  }
  return out;
}

/**
 * The part of a translated string that will appear verbatim on screen.
 * Strings with `{param}` placeholders are cut at the first placeholder,
 * since the rendered text has the value substituted in. Too-short snippets
 * (a lone "PDF", "AI") would match nearly everything, so they are dropped.
 */
export function needleOf(translated: string): string | null {
  const head = normalize(translated.split('{')[0]);
  const snippet = head.slice(0, 40).trim();
  return snippet.length >= 4 ? snippet : null;
}

/**
 * Needles contributed by the English dictionary for `query`.
 * `en` and `current` are flattened dictionaries; when the UI is English the
 * result is empty (the rendered text already covers it).
 */
export function englishFallbackNeedles(
  query: string,
  en: Map<string, string>,
  current: Map<string, string>,
  limit = 200,
): string[] {
  const q = normalize(query);
  if (!q || en === current) return [];
  // An English keyword should start a word: "ime" must find "IMEs", not
  // every string containing "time". (The rendered text is still matched as a
  // plain substring by blockMatches — this only governs the fallback.)
  const esc = q.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
  const word = new RegExp(`(^|[^a-z0-9])${esc}`);
  const needles = new Set<string>();
  for (const [key, enText] of en) {
    if (!word.test(normalize(enText))) continue;
    const tr = current.get(key);
    if (!tr || tr === enText) continue;
    const n = needleOf(tr);
    if (n) needles.add(n);
    if (needles.size >= limit) break;
  }
  return [...needles];
}

/**
 * Latin queries match at the start of a word ("ime" must not hit
 * "Times New Roman"); anything else (Chinese, Japanese…) has no word breaks
 * to lean on and matches as a plain substring.
 */
export function queryMatcher(query: string): (text: string) => boolean {
  const q = normalize(query);
  if (/^[a-z0-9 ._+#-]+$/.test(q)) {
    const esc = q.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
    const re = new RegExp(`(^|[^a-z0-9])${esc}`);
    return (text) => re.test(normalize(text));
  }
  return (text) => normalize(text).includes(q);
}

/** Does a block whose text is `text` match the query (or a fallback needle)? */
export function blockMatches(text: string, query: string, needles: string[]): boolean {
  const q = normalize(query);
  if (!q) return true;
  if (queryMatcher(q)(text)) return true;
  const t = normalize(text);
  return needles.some((n) => t.includes(n));
}
