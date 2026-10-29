'use strict';
// ブックマークのファイル(src/bmfile.js)のテスト。npm test で動かす
const test = require('node:test');
const assert = require('node:assert/strict');
const { bmFileRoot, bmFileText, bmFileParse, bmFilePaths, bmFileFillPaths, bmFileCommands, bmFileStripCommands, bmFileToBookmark, bmFileName } = require('../src/bmfile.js');

const HOME = '/home/u';
const layout = {
  k: 'split', dir: 'row', ratio: 0.4,
  a: { k: 'leaf', type: 'web', url: 'https://example.com/' },
  b: { k: 'split', dir: 'col', ratio: 0.5,
    a: { k: 'leaf', type: 'files', path: '/home/u/src', sort: { key: 'date', desc: true } },
    b: { k: 'leaf', type: 'app', cmd: 'kate', cls: 'kate', name: 'Kate' } },
};

test('書いて読むと同じ配置に戻る', () => {
  const back = bmFileParse(bmFileText('作業', layout, HOME), HOME);
  assert.equal(back.title, '作業');
  assert.deepEqual(back.root, {
    ...layout,
    b: { ...layout.b, b: { k: 'leaf', type: 'app', cmd: 'kate', cls: 'kate', name: 'Kate', webUrl: '' } },
  });
});

test('ホームの下のパスは ~ で書き、読む PC のホームへ戻す', () => {
  const text = bmFileText('', { k: 'leaf', type: 'files', path: '/home/u/src' }, HOME);
  assert.equal(JSON.parse(text).root.path, '~/src');
  assert.equal(bmFileParse(text, '/home/other').root.path, '/home/other/src');
  // ホームと名前の始まりが同じだけの別のフォルダは変えない
  assert.equal(JSON.parse(bmFileText('', { k: 'leaf', type: 'files', path: '/home/user2' }, HOME)).root.path, '/home/user2');
});

test('サイドバーのブックマークをペイン1つの配置にする', () => {
  assert.deepEqual(bmFileRoot({ kind: 'web', target: 'https://a/' }), { k: 'leaf', type: 'web', url: 'https://a/' });
  assert.deepEqual(bmFileRoot({ kind: 'files', target: '/x' }), { k: 'leaf', type: 'files', path: '/x' });
  assert.equal(bmFileRoot({ kind: 'tab', root: layout }), layout);
  assert.equal(bmFileRoot({ kind: 'folder' }), null);
});

test('ブックマークでないファイル・壊れた配置は読まない', () => {
  assert.throws(() => bmFileParse('not json', HOME));
  assert.throws(() => bmFileParse('{"a":1}', HOME));
  assert.throws(() => bmFileParse('{"prelude":"bookmark","version":2,"root":{"k":"leaf","type":"web"}}', HOME), /newer version/);
  let deep = { k: 'leaf', type: 'web' };
  for (let i = 0; i < 20; i++) deep = { k: 'split', a: deep, b: { k: 'leaf', type: 'web' } };
  assert.throws(() => bmFileParse(JSON.stringify({ prelude: 'bookmark', root: deep }), HOME));
});

test('知らない項目は捨て、知らない種類はフォルダのペインにする', () => {
  const r = bmFileParse(JSON.stringify({ prelude: 'bookmark', root: { k: 'leaf', type: 'evil', path: '~/a', onload: 'x' } }), HOME).root;
  assert.deepEqual(r, { k: 'leaf', type: 'files', path: '/home/u/a' });
});

test('起動されるコマンドを取り出す・外す', () => {
  const two = { k: 'split', a: layout, b: { k: 'split', a: { k: 'leaf', type: 'app', cmd: 'kate' }, b: { k: 'leaf', type: 'app', cmd: '', webUrl: 'https://b/' } } };
  assert.deepEqual(bmFileCommands(two), ['kate']);
  assert.deepEqual(bmFileCommands(bmFileStripCommands(two)), []);
  assert.equal(bmFileStripCommands(two).b.b.webUrl, 'https://b/');
  // only を渡すと、その中のコマンドだけを外す
  const mixed = { k: 'split', a: { k: 'leaf', type: 'app', cmd: 'kate' }, b: { k: 'leaf', type: 'app', cmd: 'sh -c evil' } };
  assert.deepEqual(bmFileCommands(bmFileStripCommands(mixed, ['sh -c evil'])), ['kate']);
});

test('配置をサイドバーのブックマークに戻す', () => {
  assert.deepEqual(bmFileToBookmark('A', { k: 'leaf', type: 'web', url: 'https://a/' }), { kind: 'web', title: 'A', target: 'https://a/' });
  assert.deepEqual(bmFileToBookmark('B', { k: 'leaf', type: 'files', path: '/x' }), { kind: 'files', title: 'B', target: '/x' });
  assert.equal(bmFileToBookmark('C', { k: 'leaf', type: 'app', cmd: 'kate', cls: 'kate' }).cmd, 'kate');
  assert.equal(bmFileToBookmark('D', layout).kind, 'tab');
  assert.equal(bmFileToBookmark('E', { k: 'leaf', type: 'home' }).kind, 'tab');
});

test('タイトルからファイル名を作る', () => {
  assert.equal(bmFileName('github.com | src'), 'github.com - src.prelude');
  assert.equal(bmFileName('a/b'), 'a-b.prelude');
  assert.equal(bmFileName('../..'), 'ブックマーク.prelude');
  assert.equal(bmFileName(''), 'ブックマーク.prelude');
});

test('paths は書いたままの形で読み書きし、知らないキーは捨てる', () => {
  const text = bmFileText('作業', { k: 'leaf', type: 'files', path: '' }, HOME, { files: '/home/u/研究', save: './出力', terminal: '~/src', evil: 'x' });
  const d = JSON.parse(text);
  assert.deepEqual(d.paths, { files: '~/研究', save: './出力', terminal: '~/src' });
  assert.deepEqual(bmFileParse(text, HOME).paths, { files: '~/研究', save: './出力', terminal: '~/src' });
  // paths が無いファイルは paths を書かない・空で読む(0.11 までの版のファイルと同じ形)
  assert.equal(JSON.parse(bmFileText('', { k: 'leaf', type: 'web', url: 'https://a/' }, HOME)).paths, undefined);
  assert.deepEqual(bmFileParse('{"prelude":"bookmark","root":{"k":"leaf","type":"web"}}', HOME).paths, {});
  assert.deepEqual(bmFileParse('{"prelude":"bookmark","paths":"x","root":{"k":"leaf","type":"web"}}', HOME).paths, {});
});

test('paths を使うフォルダに決める(~ はホーム、相対は .prelude のフォルダから)', () => {
  const dir = '/home/u/ブックマーク/研究';
  assert.deepEqual(bmFilePaths({ files: '~/研究', save: './出力', terminal: '../src' }, HOME, dir),
    { files: '/home/u/研究', save: '/home/u/ブックマーク/研究/出力', terminal: '/home/u/ブックマーク/src' });
  // 無いものは files → .prelude のフォルダ → ホーム
  assert.deepEqual(bmFilePaths({ files: '/data' }, HOME, dir), { files: '/data', save: '/data', terminal: '/data' });
  assert.deepEqual(bmFilePaths({}, HOME, dir), { files: dir, save: dir, terminal: dir });
  assert.deepEqual(bmFilePaths(undefined, HOME, ''), { files: HOME, save: HOME, terminal: HOME });
  assert.equal(bmFilePaths({ files: '.' }, HOME, dir).files, dir);
});

test('パスの無いフォルダ・フィールドのペインは files のフォルダで開く', () => {
  const l = { k: 'split', dir: 'row', ratio: 0.5,
    a: { k: 'leaf', type: 'files', path: '' },
    b: { k: 'split', dir: 'col', ratio: 0.5, a: { k: 'leaf', type: 'field', path: '/x' }, b: { k: 'leaf', type: 'web', url: '' } } };
  const r = bmFileFillPaths(l, '/w');
  assert.equal(r.a.path, '/w');
  assert.equal(r.b.a.path, '/x');
  assert.equal(r.b.b.path, undefined);
});
