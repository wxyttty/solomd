import assert from 'node:assert/strict';
import { test } from 'node:test';

import { blockMatches, englishFallbackNeedles, flatten, needleOf, normalize } from './settings-search.ts';

const en = flatten({
  settings: {
    windowsEditorEngine: 'Editor engine',
    windowsEditorEngineHint: 'CodeMirror adds syntax colouring; some IMEs may drop characters.',
    vimMode: 'Vim mode',
    pdfToc: 'Start the PDF with a table of contents',
    count: 'Showing {n} items',
  },
});
const zh = flatten({
  settings: {
    windowsEditorEngine: '编辑器引擎',
    windowsEditorEngineHint: 'CodeMirror 带语法着色；部分输入法可能丢字。',
    vimMode: 'Vim 模式',
    pdfToc: '在 PDF 开头插入目录页',
    count: '共 {n} 项',
  },
});

test('normalize folds case and whitespace', () => {
  assert.equal(normalize('  Vim\n  MODE '), 'vim mode');
});

test('flatten keys nested strings by dotted path', () => {
  assert.equal(en.get('settings.vimMode'), 'Vim mode');
  assert.equal(en.size, 5);
});

test('needleOf cuts at the first placeholder and drops tiny snippets', () => {
  assert.equal(needleOf('Showing {n} items'), 'showing');
  assert.equal(needleOf('共 {n} 项'), null);
  assert.equal(needleOf('PDF'), null);
});

test('an English keyword finds the translated setting', () => {
  const needles = englishFallbackNeedles('IME', en, zh);
  assert.deepEqual(needles, ['codemirror 带语法着色；部分输入法可能丢字。']);
  assert.ok(blockMatches('编辑器引擎 CodeMirror 带语法着色；部分输入法可能丢字。', 'IME', needles));
  assert.ok(!blockMatches('Vim 模式', 'IME', needles));
});

test('an English keyword only matches at the start of a word', () => {
  const dict = flatten({ a: { t: 'Autosave time', i: 'Input method (IME) fixes' } });
  const zhDict = flatten({ a: { t: '自动保存时间', i: '输入法（IME）修复' } });
  assert.deepEqual(englishFallbackNeedles('ime', dict, zhDict), ['输入法（ime）修复']);
});

test('the rendered text still matches directly, in any language', () => {
  assert.ok(blockMatches('在 PDF 开头插入目录页', '目录', []));
  assert.ok(blockMatches('Vim 模式', 'vim', []));
});

test('an English UI gets no fallback needles', () => {
  assert.deepEqual(englishFallbackNeedles('ime', en, en), []);
});

test('Latin queries in rendered text also start at a word', () => {
  assert.ok(!blockMatches('Font: Times New Roman', 'ime', []));
  assert.ok(blockMatches('Font: Times New Roman', 'time', []));
  assert.ok(blockMatches('字体 Times New Roman', 'roman', []));
  assert.ok(blockMatches('默认字体（预览样式表）', '样式', []));
});

test('an empty query matches everything', () => {
  assert.ok(blockMatches('anything', '  ', []));
});
