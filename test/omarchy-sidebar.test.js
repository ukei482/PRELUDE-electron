// Omarchy のサイドバーの、コアの様子 → 行(omarchy/plugin/SidebarModel.js)。QML の JS(.pragma library)なので、その行を外して読む
const test = require('node:test');
const assert = require('node:assert');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');

const src = fs.readFileSync(path.join(__dirname, '..', 'omarchy', 'plugin', 'SidebarModel.js'), 'utf8').replace(/^\.pragma library$/m, '');
const ctx = {};
vm.runInNewContext(src, ctx);
// 別の realm の配列は deepStrictEqual で一致しないので、JSON で持ち出す
const M = Object.fromEntries(
  ['tabRows', 'bookmarkRows', 'newKinds', 'width', 'parse', 'hyprStyle'].map((k) => [k, (...a) => JSON.parse(JSON.stringify(ctx[k](...a)))]),
);

const state = {
  active: 2,
  closed: 1,
  space: 'Main',
  space_dir: '/h/スペース/Main',
  tabs: [
    { id: 1, num: 1, title: 'メモ', bookmark: '/h/スペース/Main/メモ.prelude', active_pane: 10, layout: { id: 10, type: 'files', path: '/h/メモ' } },
    {
      id: 2,
      num: 2,
      title: '',
      bookmark: '',
      active_pane: 21,
      layout: { k: 'split', dir: 'row', a: { id: 20, type: 'web', url: 'https://example.com/a' }, b: { id: 21, type: 'app', name: 'Kate', window_title: 'x.txt - Kate' } },
    },
  ],
};

test('タブの行: 分割したタブはペインの行が続き、題名が無ければ今のペインの名前', () => {
  const rows = M.tabRows(state);
  assert.deepStrictEqual(
    rows.map((r) => [r.row, r.id, r.title, r.mark, r.active]),
    [
      ['tab', 1, 'メモ', 'F', false],
      ['tab', 2, 'x.txt - Kate', 'A', true],
      ['pane', 20, 'example.com', 'W', false],
      ['pane', 21, 'x.txt - Kate', 'A', true],
    ],
  );
});

test('タブの行: 状態が無くても空', () => {
  assert.deepStrictEqual(M.tabRows(null).length, 0);
  assert.deepStrictEqual(M.tabRows({}).length, 0);
});

test('ブックマークの行: フォルダの外が先、閉じたフォルダは中身を出さず、開いているものに印', () => {
  const bms = [
    { path: '/h/スペース/Main/メモ.prelude', title: 'メモ', num: 1, kind: 'files', folder: '' },
    { path: '/h/スペース/Main/研究/論文.prelude', title: '論文', num: 2, kind: 'web', folder: '研究' },
  ];
  const folders = [{ path: '/h/スペース/Main/研究', title: '研究' }];
  const closed = M.bookmarkRows(state, bms, folders, {});
  assert.deepStrictEqual(
    closed.map((r) => [r.row, r.title, r.open ?? r.hasOpen, r.count ?? r.depth]),
    [
      ['bookmark', 'メモ', true, 0],
      ['folder', '研究', false, 1],
    ],
  );
  const open = M.bookmarkRows(state, bms, folders, { '/h/スペース/Main/研究': true });
  assert.deepStrictEqual(
    open.map((r) => [r.row, r.title, r.depth ?? null]),
    [
      ['bookmark', 'メモ', 0],
      ['folder', '研究', null],
      ['bookmark', '論文', 1],
    ],
  );
});

test('新しいタブの種類: Web が先、設定は出さない', () => {
  const k = M.newKinds([{ name: 'Files', type: 'files' }, { name: 'Settings', type: 'settings' }]);
  assert.deepStrictEqual(k.map((x) => x.kind), ['web', 'files']);
});

test('幅の設定: 文字でも読み、範囲に丸め、読めなければ既定', () => {
  assert.strictEqual(M.width('300', 260, 160, 800), 300);
  assert.strictEqual(M.width(40, 260, 160, 800), 160);
  assert.strictEqual(M.width('abc', 260, 160, 800), 260);
  assert.strictEqual(M.width(9999, 260, 160, 800), 800);
});

test('返事を読む: 壊れていれば fallback', () => {
  assert.deepStrictEqual(M.parse('["Main"]\n', []), ['Main']);
  assert.deepStrictEqual(M.parse('null\n', []), []);
  assert.deepStrictEqual(M.parse('', []), []);
});

test('Hyprland の窓の決まり: 間(上・右・下・左)・枠・角丸・選ばれていない窓の枠の色', () => {
  const text = [
    '{"option":"general:gaps_out","css":"20 20 20 20","set":true}',
    '{"option":"general:gaps_in","css":"5 5 5 5","set":true}',
    '{"option":"general:border_size","int":2,"set":true}',
    '{"option":"decoration:rounding","int":0,"set":true}',
    '{"option":"general:col.inactive_border","gradient":"ff1e1e1e0deg","set":true}',
  ].join('\n');
  assert.deepStrictEqual(M.hyprStyle(text), {
    gapsOut: { top: 20, right: 20, bottom: 20, left: 20 },
    gapsIn: 5,
    borderSize: 2,
    rounding: 0,
    borderColor: '#ff1e1e1e',
  });
  // 数だけの間、2 つの間(上下・左右)、読めないときは 0
  assert.deepStrictEqual(M.hyprStyle('{"int":8}').gapsOut, { top: 8, right: 8, bottom: 8, left: 8 });
  assert.deepStrictEqual(M.hyprStyle('{"css":"10 4"}').gapsOut, { top: 10, right: 4, bottom: 10, left: 4 });
  assert.deepStrictEqual(M.hyprStyle(''), { gapsOut: { top: 0, right: 0, bottom: 0, left: 0 }, gapsIn: 0, borderSize: 0, rounding: 0, borderColor: '' });
});
