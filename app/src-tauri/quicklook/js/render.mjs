// Markdown -> HTML for the Quick Look extension. build.sh bundles this into one
// IIFE that JavaScriptCore evaluates; it exposes globalThis.soloRender(md).
// JavaScriptCore has no DOM, so this is markdown-it with the DOM-free plugins
// the app also uses. Raw HTML in the note is escaped (html: false): a preview
// must not run or fetch anything.
import MarkdownIt from 'markdown-it';
import footnote from 'markdown-it-footnote';
import frontMatter from 'markdown-it-front-matter';
import mark from 'markdown-it-mark';
import cjkFriendly from 'markdown-it-cjk-friendly';
import hljs from 'highlight.js/lib/common';

const md = new MarkdownIt({
  html: false,
  linkify: true,
  typographer: false,
  highlight(code, lang) {
    if (lang && hljs.getLanguage(lang)) {
      try {
        return `<pre class="hljs"><code>${hljs.highlight(code, { language: lang, ignoreIllegals: true }).value}</code></pre>`;
      } catch {
        /* fall through to the default fence */
      }
    }
    return '';
  },
})
  .use(footnote)
  .use(frontMatter, () => {})
  .use(mark)
  .use(cjkFriendly);

// GitHub-style task lists: "- [ ] x" / "- [x] x" become disabled checkboxes.
md.core.ruler.after('inline', 'task-lists', (state) => {
  const t = state.tokens;
  for (let i = 2; i < t.length; i++) {
    if (t[i].type !== 'inline' || t[i - 1].type !== 'paragraph_open' || t[i - 2].type !== 'list_item_open') continue;
    const m = /^\[([ xX])\]\s+/.exec(t[i].content);
    if (!m || !t[i].children || !t[i].children.length) continue;
    const first = t[i].children[0];
    if (first.type !== 'text' || !first.content.startsWith(m[0])) continue;
    first.content = first.content.slice(m[0].length);
    const box = new state.Token('html_inline', '', 0);
    box.content = `<input type="checkbox" disabled${m[1] === ' ' ? '' : ' checked'}> `;
    t[i].children.unshift(box);
    t[i - 2].attrJoin('class', 'task-item');
  }
});

globalThis.soloRender = (src) => md.render(String(src));
