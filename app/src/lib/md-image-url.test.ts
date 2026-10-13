import assert from 'node:assert/strict';
import { test } from 'node:test';
import MarkdownIt from 'markdown-it';

import { encodeImageDestination, markdownImage } from './md-image-url.ts';

const md = new MarkdownIt();
const srcOf = (markdown: string): string | null => {
  const m = /<img src="([^"]*)"/.exec(md.render(markdown));
  return m ? m[1] : null;
};
// What image-resolve.ts does with a rendered src: decode exactly once.
const decodeOnce = (s: string): string => {
  try {
    return decodeURIComponent(s);
  } catch {
    return s;
  }
};

test('the #345 path renders as an image once encoded', () => {
  const path = 'D:/Program Filejs/SoloMD/Image/image-20260924-221953-2pgx69.png';
  assert.equal(srcOf(`![](${path})`), null, 'raw form is literal text — the bug');
  const link = markdownImage(path);
  assert.equal(link, '![](D:/Program%20Filejs/SoloMD/Image/image-20260924-221953-2pgx69.png)');
  const src = srcOf(link);
  assert.ok(src, 'encoded form renders an <img>');
  assert.equal(decodeOnce(src!), path, 'decoding the rendered src gives back the real file');
});

test('paths without problem characters are left readable', () => {
  assert.equal(encodeImageDestination('_assets/image-1.png'), '_assets/image-1.png');
  assert.equal(encodeImageDestination('图片/截图.png'), '图片/截图.png');
  assert.equal(srcOf(markdownImage('图片/截图.png')) !== null, true);
});

test('a note name with spaces (per-file assets folder) round-trips', () => {
  const path = 'My Note.assets/image-2.png';
  const src = srcOf(markdownImage(path));
  assert.ok(src);
  assert.equal(decodeOnce(src!), path);
});

test('parentheses and a literal percent survive the single decode', () => {
  for (const path of ['C:/Users/A B/Pictures (old)/x.png', 'shots/100%.png', 'a/b%20c.png']) {
    const src = srcOf(markdownImage(path));
    assert.ok(src, `renders: ${path}`);
    assert.equal(decodeOnce(src!), path, `round-trips: ${path}`);
  }
});

test('remote URLs keep their encoding; only whitespace is touched', () => {
  assert.equal(encodeImageDestination('https://cdn.example.com/a%2Fb.png'), 'https://cdn.example.com/a%2Fb.png');
  assert.equal(encodeImageDestination('https://cdn.example.com/my shot.png'), 'https://cdn.example.com/my%20shot.png');
  assert.equal(encodeImageDestination('data:image/png;base64,AAAA'), 'data:image/png;base64,AAAA');
});

test('alt text is kept', () => {
  assert.equal(markdownImage('a b.png', 'logo'), '![logo](a%20b.png)');
});
