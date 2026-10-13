/**
 * Format a markdown document using Prettier (via the standalone build so we
 * don't pull in any Node-only dependencies).
 *
 * Front matter is preserved verbatim — Prettier's markdown parser doesn't
 * understand YAML front matter, and ad-hoc rewrites would corrupt it.
 */

const FRONT_MATTER_RE = /^(---\r?\n[\s\S]*?\r?\n---\r?\n)([\s\S]*)$/;

export async function formatMarkdown(
  source: string,
  opts: { printWidth?: number } = {},
): Promise<string> {
  const printWidth = opts.printWidth ?? 100;
  const m = FRONT_MATTER_RE.exec(source);
  const fm = m ? m[1] : '';
  const body = m ? m[2] : source;

  // Prettier's standalone build plus the markdown plugin is ~1 MB, and it is
  // only ever needed when the user asks to format.
  const [prettier, markdownPlugin] = await Promise.all([
    import('prettier/standalone'),
    import('prettier/plugins/markdown'),
  ]);

  const formatted = await prettier.format(body, {
    parser: 'markdown',
    plugins: [markdownPlugin],
    printWidth,
    proseWrap: 'preserve',
    tabWidth: 2,
  });

  // Prettier guarantees trailing newline. Strip an extra one if our concat
  // would produce a double-blank between FM and body.
  if (fm) {
    const bodyTrimmed = formatted.replace(/^\r?\n/, '');
    return fm + bodyTrimmed;
  }
  return formatted;
}
