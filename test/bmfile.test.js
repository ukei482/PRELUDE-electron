'use strict';
// ブックマークのファイル(src/bmfile.js と bmcodec.js)のテスト。npm test で動かす
const test = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const { bmFileRoot, bmFileData, bmFileParse, bmFilePaths, bmFileFillPaths, bmFileCommands, bmFileStripCommands, bmFileToBookmark, bmFileName } = require('../src/bmfile.js');
const { bmDecode, bmEncode } = require('../bmcodec.js');

const HOME = '/home/u';
const FIXTURES = path.join(__dirname, 'fixtures', 'format');
const fixture = (name) => fs.readFileSync(path.join(FIXTURES, name), 'utf8');
// ファイルに書く文字列と、文字列から読んだもの(メインプロセスと画面を通したのと同じ)
const write = (...a) => bmEncode(bmFileData(...a));
const read = (text, home = HOME) => bmFileParse(bmDecode(text), home);

const layout = {
  k: 'split', dir: 'row', ratio: 0.4,
  a: { k: 'leaf', type: 'web', url: 'https://example.com/' },
  b: { k: 'split', dir: 'col', ratio: 0.5,
    a: { k: 'leaf', type: 'files', path: '/home/u/src', sort: { key: 'date', desc: true } },
    b: { k: 'leaf', type: 'app', cmd: 'kate', cls: 'kate', name: 'Kate' } },
};

// 空の文字列の項目を落とす(Rust 版は空の項目を持たないので、比べる前にそろえる)
const dropEmpty = (o) => (o && typeof o === 'object' ? Object.fromEntries(Object.entries(o).filter(([, v]) => v !== '').map(([k, v]) => [k, dropEmpty(v)])) : o);

test('共通のファイル(Rust 版のテストも読む)を読む', () => {
  for (const name of ['full', 'legacy']) {
    const { title, paths, root } = read(fixture(`${name}.prelude`));
    assert.deepEqual(dropEmpty({ title, paths, root }), JSON.parse(fixture(`${name}.expected.json`)), name);
  }
});

test('TOML で書いて読むと同じ配置に戻る', () => {
  const text = write('作業', layout, HOME);
  assert.match(text, /^version = 2\ntitle = "作業"\n/);
  assert.match(text, /\[layout\.b\.a\]/);
  const back = read(text);
  assert.equal(back.title, '作業');
  assert.deepEqual(back.root, {
    ...layout,
    b: { ...layout.b, b: { k: 'leaf', type: 'app', cmd: 'kate', cls: 'kate', name: 'Kate', webUrl: '' } },
  });
});

test('0.11 までの JSON を読み、書き直すと TOML になる', () => {
  const old = read(fixture('legacy.prelude'));
  assert.equal(old.root.a.path, '/home/u/文書');
  assert.deepEqual(old.rest, {});
  const back = read(write(old.title, old.root, HOME, old.paths, old.rest));
  assert.deepEqual(back, old);
});

test('ホームの下のパスは ~ で書き、読む PC のホームへ戻す', () => {
  const d = bmFileData('', { k: 'leaf', type: 'files', path: '/home/u/src' }, HOME);
  assert.equal(d.layout.path, '~/src');
  assert.equal(read(bmEncode(d), '/home/other').root.path, '/home/other/src');
  // ホームと名前の始まりが同じだけの別のフォルダは変えない
  assert.equal(bmFileData('', { k: 'leaf', type: 'files', path: '/home/user2' }, HOME).layout.path, '/home/user2');
});

test('サイドバーのブックマークをペイン1つの配置にする', () => {
  assert.deepEqual(bmFileRoot({ kind: 'web', target: 'https://a/' }), { k: 'leaf', type: 'web', url: 'https://a/' });
  assert.deepEqual(bmFileRoot({ kind: 'files', target: '/x' }), { k: 'leaf', type: 'files', path: '/x' });
  assert.equal(bmFileRoot({ kind: 'tab', root: layout }), layout);
  assert.equal(bmFileRoot({ kind: 'folder' }), null);
});

test('ブックマークでないファイル・壊れた配置は読まない', () => {
  assert.throws(() => read('not toml = = 1'), /Not a bookmark/);
  assert.throws(() => read('title = "no layout"'), /Not a bookmark/);
  assert.throws(() => read('{"a":1}'), /Not a bookmark/);
  assert.throws(() => read('version = "x"\n[layout]\nk = "leaf"'), /Not a bookmark/);
  assert.throws(() => read('version = 3\n[layout]\nk = "leaf"'), /newer version/);
  assert.throws(() => read('{"prelude":"bookmark","version":2,"root":{"k":"leaf","type":"web"}}'), /newer version/);
  let deep = { k: 'leaf', type: 'web' };
  for (let i = 0; i < 20; i++) deep = { k: 'split', a: deep, b: { k: 'leaf', type: 'web' } };
  assert.throws(() => read(JSON.stringify({ prelude: 'bookmark', root: deep })));
  assert.throws(() => read(bmEncode({ layout: deep })));
});

test('配置の知らない項目は捨て、知らない種類はフォルダのペインにする', () => {
  const r = read(JSON.stringify({ prelude: 'bookmark', root: { k: 'leaf', type: 'evil', path: '~/a', onload: 'x' } })).root;
  assert.deepEqual(r, { k: 'leaf', type: 'files', path: '/home/u/a' });
});

test('配置・title・paths 以外の項目([ui.<部品名>] など)は残す', () => {
  const d = read(fixture('full.prelude'));
  assert.deepEqual(d.rest, { ui: { field: { color: 'blue' } } });
  const text = write(d.title, d.root, HOME, d.paths, { ...d.rest, note: 'メモ', version: 9, layout: 'x' });
  const back = read(text);
  assert.deepEqual(back.rest, { ui: { field: { color: 'blue' } }, note: 'メモ' }); // 決まった項目と同じ名前のものは rest にしない
  assert.match(text, /\[ui\.field\]\ncolor = "blue"/);
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
  assert.equal(bmFileName('a-/b'), 'a--b.prelude');
  assert.equal(bmFileName('../..'), 'ブックマーク.prelude');
  assert.equal(bmFileName(''), 'ブックマーク.prelude');
});

test('paths は書いたままの形で読み書きし、知らない項目も残す', () => {
  const text = write('作業', { k: 'leaf', type: 'files', path: '' }, HOME, { files: '/home/u/研究', save: './出力', terminal: '~/src', camera: '~/写真', bad: 3 });
  const want = { files: '~/研究', save: './出力', terminal: '~/src', camera: '~/写真' };
  assert.deepEqual(bmDecode(text).paths, want);
  assert.deepEqual(read(text).paths, want);
  // paths が無いファイルは paths を書かない・空で読む
  assert.equal(bmFileData('', { k: 'leaf', type: 'web', url: 'https://a/' }, HOME).paths, undefined);
  assert.deepEqual(read('{"prelude":"bookmark","root":{"k":"leaf","type":"web"}}').paths, {});
  assert.deepEqual(read('{"prelude":"bookmark","paths":"x","root":{"k":"leaf","type":"web"}}').paths, {});
});

test('使うフォルダは ブックマーク → スペース → 代わりの項目 の順(Rust 版と同じ)', () => {
  const bm = read(fixture('full.prelude'));
  const space = { paths: bmDecode(fixture('space.toml')).paths, dir: '/home/u/スペース/論文執筆' };
  const dir = '/home/u/スペース/論文執筆/執筆';
  assert.deepEqual(bmFilePaths(bm.paths, HOME, dir, space), {
    files: '/home/u/研究/論文',
    open: '/home/u/スペース/論文執筆/執筆/文献', // ブックマークの相対パスは .prelude のフォルダから
    save: '/home/u/研究/論文/原稿',
    download: '/home/u/研究/論文/原稿', // download → save
    terminal: '/home/u/スペース/論文執筆/src', // スペースの相対パスはスペースのフォルダから
  });
  // スペースが無く、ブックマークにも無ければ files。files も無ければ入れない(使う側の既定になる)
  assert.deepEqual(bmFilePaths({ files: '../src' }, HOME, dir), Object.fromEntries(['files', 'open', 'save', 'download', 'terminal'].map((k) => [k, '/home/u/スペース/論文執筆/src'])));
  assert.deepEqual(bmFilePaths({}, HOME, dir), {});
  assert.deepEqual(bmFilePaths(undefined, HOME, ''), {});
  assert.deepEqual(bmFilePaths({ save: '/s' }, HOME, dir), { save: '/s', download: '/s' });
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
