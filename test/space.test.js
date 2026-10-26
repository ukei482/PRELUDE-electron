'use strict';
// スペース(src/space.js)のテスト。npm test で動かす
const test = require('node:test');
const assert = require('node:assert/strict');
const { spaceSort, spaceMigrate, spaceWsLoad, spaceWsSave, spacePick } = require('../src/space.js');
const { bmFileRoot } = require('../src/bmfile.js');

const ids = (list) => list.map((b) => b.id);

test('開いているブックマークはタブの順、残りは名前順。フォルダは後ろ', () => {
  const marks = [
    { id: 'c', kind: 'web', title: 'C', folder: '' },
    { id: 'a', kind: 'web', title: 'A', folder: '' },
    { id: 'b', kind: 'web', title: 'B', folder: '' },
    { id: 'f2', kind: 'folder', title: 'Z' },
    { id: 'f1', kind: 'folder', title: 'Y' },
    { id: 'y', kind: 'web', title: 'y', folder: 'f1' },
    { id: 'x', kind: 'web', title: 'x', folder: 'f1' },
    { id: 'z', kind: 'web', title: 'z', folder: 'f2' },
  ];
  assert.deepEqual(ids(spaceSort(marks, [])), ['a', 'b', 'c', 'f1', 'x', 'y', 'f2', 'z']);
  assert.deepEqual(ids(spaceSort(marks, ['c', null, 'y', 'b'])), ['c', 'b', 'a', 'f1', 'y', 'x', 'f2', 'z']);
});

test('名前の数字は数の順に並ぶ', () => {
  const marks = [{ id: '10', kind: 'web', title: 'item 10' }, { id: '2', kind: 'web', title: 'item 2' }];
  assert.deepEqual(ids(spaceSort(marks, [])), ['2', '10']);
});

test('旧ブックマークを書き出すものに分ける', () => {
  const old = [
    { id: 'f', kind: 'folder', title: '仕事' },
    { id: 'w', kind: 'web', num: 1, title: 'Ex', url: 'https://example.com/', folder: 'f' },
    { id: 'd', kind: 'files', num: 2, title: 'src', path: '/home/u/src' },
    { id: 'p', kind: 'app', num: 3, title: 'Kate', cmd: 'kate', cls: 'kate', webUrl: '' },
    { id: 't', kind: 'tab', num: 4, title: '分割', root: { k: 'split', dir: 'row', ratio: 0.5, a: { k: 'leaf', type: 'home' }, b: { k: 'leaf', type: 'web', url: 'https://a/' } } },
    { id: 'r', kind: 'rust-app', title: '?' },
    { id: 'e', kind: 'web', title: 'empty', url: '' },
  ];
  const m = spaceMigrate(old, bmFileRoot);
  assert.deepEqual(m.folders, ['仕事']);
  assert.deepEqual(m.items.map((i) => [i.id, i.folder]), [['w', '仕事'], ['d', ''], ['p', ''], ['t', '']]);
  assert.deepEqual(m.items[0].root, { k: 'leaf', type: 'web', url: 'https://example.com/' });
  assert.deepEqual(m.items[1].root, { k: 'leaf', type: 'files', path: '/home/u/src' });
  assert.equal(m.items[2].root.cmd, 'kate');
  assert.equal(m.items[3].root.k, 'split');
  assert.deepEqual(m.rest.map((b) => b.id), ['r', 'e']);
});

test('古い形の保存は既定のスペースのタブになる', () => {
  const tabs = [{ num: 1, root: { k: 'leaf', type: 'home' } }];
  assert.deepEqual(spaceWsLoad({ active: 0, tabs }), { space: 'Main', spaces: { Main: { active: 0, tabs } } });
  assert.deepEqual(spaceWsLoad(null), { space: 'Main', spaces: {} });
});

test('保存して読むと同じスペースに戻る', () => {
  const a = { active: 1, tabs: [{ num: 1 }, { num: 2 }] };
  const b = { active: 0, tabs: [{ num: 1 }] };
  const saved = spaceWsSave('作業', a, { 作業: { active: 0, tabs: [] }, Main: b });
  assert.equal(saved.spaces['作業'], undefined);
  assert.deepEqual(spaceWsLoad(JSON.parse(JSON.stringify(saved))), { space: '作業', spaces: { Main: b, 作業: a } });
});

test('前のスペースが無ければ Main、それも無ければ最初のもの', () => {
  assert.equal(spacePick('作業', ['Main', '作業']), '作業');
  assert.equal(spacePick('消した', ['A', 'Main']), 'Main');
  assert.equal(spacePick('消した', ['A', 'B']), 'A');
  assert.equal(spacePick('x', []), 'Main');
});
