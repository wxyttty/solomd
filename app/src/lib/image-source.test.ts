import assert from 'node:assert/strict';
import { test } from 'node:test';

import { isContentUri, isSafPath, sniffImageExt } from './image-source.ts';

const bytes = (...v: number[]) => new Uint8Array([...v, ...new Array(16).fill(0)]);
const ascii = (s: string) => new TextEncoder().encode(s);

test('the Android photo picker URI from #349 is recognised as content://', () => {
  assert.equal(
    isContentUri('content://media/picker/0/com.android.providers.media.photopicker/media/1000012345'),
    true,
  );
  assert.equal(isContentUri('content://com.android.providers.media.documents/document/image%3A42'), true);
  assert.equal(isContentUri('/storage/emulated/0/Pictures/a.png'), false);
  assert.equal(isContentUri('C:/Users/me/a.png'), false);
});

test('SAF vault documents are told apart from filesystem paths', () => {
  assert.equal(isSafPath('saf:primary:Notes/Do cel.md'), true);
  assert.equal(isSafPath('/storage/emulated/0/Notes/a.md'), false);
  assert.equal(isSafPath(undefined), false);
});

test('common image formats are identified from their bytes', () => {
  assert.equal(sniffImageExt(bytes(0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a)), 'png');
  assert.equal(sniffImageExt(bytes(0xff, 0xd8, 0xff, 0xe0)), 'jpg');
  assert.equal(sniffImageExt(ascii('GIF89a......')), 'gif');
  assert.equal(sniffImageExt(ascii('RIFF\u0000\u0000\u0000\u0000WEBPVP8 ')), 'webp');
  assert.equal(sniffImageExt(ascii('BM..............')), 'bmp');
  assert.equal(sniffImageExt(ascii('\u0000\u0000\u0000\u001cftypheic....')), 'heic');
  assert.equal(sniffImageExt(ascii('\u0000\u0000\u0000\u001cftypavif....')), 'avif');
  assert.equal(sniffImageExt(ascii('<?xml version="1.0"?>\n<svg xmlns="x">')), 'svg');
});

test('unknown or tiny payloads give null so the caller picks a default', () => {
  assert.equal(sniffImageExt(ascii('hello world, not an image')), null);
  assert.equal(sniffImageExt(new Uint8Array([0x89, 0x50])), null);
});
