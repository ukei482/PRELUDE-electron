// HTML/CSS の UI の試作: 部品の決まり(parts.js)が守られることと、つなぎ(glue.js)・サイドバー(sidebar.js)が決まりに合うこと。
// DOM は使わない(サイドバーは HTML の文字列に描く)。
const test = require('node:test');
const assert = require('node:assert');
const Parts = require('../parts.js');
const Glue = require('../glue.js');
const Sidebar = require('../sidebar.js');
const Fake = require('../fake.js');

const snap = () => Fake.create().snapshot();
const partsOf = (core, ui = {}, sent = []) => Parts.make(Glue.values(core, ui), (p, cb, a) => sent.push([p, cb, a]));

test('つなぎは偽のコアの様子から、決まりどおりの props を作る', () => {
  const core = snap();
  const p = partsOf(core, { expanded: { [core.folders[0].path]: true } });
  assert.strictEqual(p.Spaces.current, '仕事');
  assert.strictEqual(p.Tabs.items.length, 4);
  assert.strictEqual(p.Tabs.items[0].paneCount, 3);
  assert.strictEqual(p.Tabs.panes.filter((x) => x.tab === p.Tabs.items[0].id).length, 3); // 分割したタブのペイン
  assert.strictEqual(p.PaneBar.canSplit, true); // 段 4
  assert.ok(p.Bookmarks.items.find((b) => b.title === 'GitHub').open);
  assert.strictEqual(p.Bookmarks.folders[0].expanded, true);
  assert.strictEqual(p.NewTab.kinds[0].kind, 'web');
  // bookmark.list の folder(名前)は、部品ではフォルダのパスになる
  const inbox = p.Bookmarks.items.find((b) => b.title === '受信箱');
  assert.ok(p.Bookmarks.folders.some((f) => f.path === inbox.folder), inbox.folder);
});

test('段が 2 より下なら分割できない', () => {
  const c = Fake.create();
  c.setLevel(1);
  assert.strictEqual(partsOf(c.snapshot()).PaneBar.canSplit, false);
});

test('決まりに無い部品・props を読むと止まる', () => {
  const p = partsOf(snap());
  assert.throws(() => p.Tab, /決まりに無い部品/);
  assert.throws(() => p.Tabs.item, /決まりに無い/);
  assert.throws(() => p.Tabs.items[0].icn, /決まりに無い/);
  assert.throws(() => p.Tabs.items.map((t) => t.tittle), /決まりに無い/); // 配列のメソッドの中でも
  assert.throws(() => {
    p.Tabs.canReopen = true;
  }, /書き換えない/);
});

test('決まりに無い callback・違う引数で頼むと止まる', () => {
  const sent = [];
  const p = partsOf(snap(), {}, sent);
  p.Tabs.call('activate', 3);
  assert.deepStrictEqual(sent, [['Tabs', 'activate', [3]]]);
  assert.throws(() => p.Tabs.call('activat', 3), /決まりに無い callback/);
  assert.throws(() => p.Tabs.call('activate', '3'), /int ではない/);
  assert.throws(() => p.Tabs.call('activate'), /引数は 1 個/);
});

test('ホストの値が足りない・型が違うと止まる', () => {
  const v = Glue.values(snap(), {});
  assert.throws(() => Parts.make({ ...v, PaneBar: { ...v.PaneBar, url: undefined } }, () => {}), /PaneBar.url/);
  const items = [{ ...v.Tabs.items[0], extra: 1 }];
  assert.throws(() => Parts.make({ ...v, Tabs: { ...v.Tabs, items } }, () => {}), /TabItem に無い項目/);
  assert.throws(() => Parts.make({ Sidebar: {} }, () => {}), /決まりに無い部品/);
});

// 描いた HTML の data-call を全部、偽の引数ではなく描いたとおりの引数で頼んでみる(決まりに無い頼みごとがあれば止まる)
function callsIn(html) {
  const out = [];
  const re = /data-call="([^"]+)" data-args="([^"]*)"/g;
  let m;
  while ((m = re.exec(html))) out.push([m[1], JSON.parse(m[2].replace(/&quot;/g, '"').replace(/&amp;/g, '&').replace(/&#39;/g, "'"))]);
  return out;
}

test('サイドバーは決まりの中だけで描け、頼みごとも決まりに合う(メニュー・細い列も)', () => {
  const core = snap();
  const sent = [];
  const p = partsOf(core, { expanded: Object.fromEntries(core.folders.map((f) => [f.path, true])) }, sent);
  const t = p.Tabs.items[0];
  const b = p.Bookmarks.items.find((x) => x.folder);
  const views = [
    {},
    { menu: { kind: 'space', x: 0, y: 0 } },
    { menu: { kind: 'newtab', x: 0, y: 0 } },
    { menu: { kind: 'tab', target: String(t.id), x: 0, y: 0 } },
    { menu: { kind: 'bm', target: b.path, x: 0, y: 0 } },
    { menu: { kind: 'folder', target: b.folder, x: 0, y: 0 } },
    { edit: { key: `bm:${b.path}`, value: null } },
  ];
  let n = 0;
  for (const view of views) {
    const html = Sidebar.render(p, view);
    for (const [fn, args] of callsIn(html)) {
      const [part, cb] = fn.split('.');
      p[part].call(cb, ...args);
      n++;
    }
  }
  assert.ok(n > 30, `頼みごとの数 ${n}`);
  const compact = partsOf(core, { compact: true });
  assert.match(Sidebar.render(compact, {}), /class="sb compact"/);
  // 開閉はスペースより上
  const html = Sidebar.render(p, {});
  assert.ok(html.indexOf('SidebarFrame.toggleCompact') < html.indexOf('data-menu="space"'));
});

test('編集の欄の確定は、決まりの callback になる', () => {
  const sent = [];
  const core = snap();
  const p = partsOf(core, {}, sent);
  Sidebar.submitEdit(p, 'renumber:7', '5');
  Sidebar.submitEdit(p, 'renumber:7', 'x'); // 数でなければ頼まない
  Sidebar.submitEdit(p, 'space-new', '  新しい作業 ');
  Sidebar.submitEdit(p, 'folder-new', '');
  assert.deepStrictEqual(sent, [
    ['Tabs', 'renumber', [7, 5]],
    ['Spaces', 'create', ['新しい作業']],
  ]);
});

test('部品の callback → コアの命令(全部の callback につなぎがある)', () => {
  const core = snap();
  const sample = { string: 'x', int: 1, bool: true };
  for (const [part, def] of Object.entries(Parts.PARTS)) {
    for (const [cb, sig] of Object.entries(def.callbacks)) {
      const st = Glue.steps(part, cb, sig.map((t) => sample[t]), core);
      assert.ok(Array.isArray(st), `${part}.${cb}`);
    }
  }
  const tabs = core.state.tabs;
  assert.deepStrictEqual(Glue.steps('Tabs', 'closeBelow', [tabs[1].id], core), tabs.slice(2).map((t) => ({ cmd: 'tab.close', args: { tab: { id: t.id } } })));
  assert.deepStrictEqual(Glue.steps('NewTab', 'open', ['web'], core), [{ launcher: {} }]);
  assert.deepStrictEqual(Glue.steps('Bookmarks', 'moveTo', ['/a.prelude', ''], core), [{ cmd: 'bookmark.move', args: { path: '/a.prelude', folder: '' } }]);
});

test('偽のコアは命令をそれらしく当てはめる(命令の名前と引数は本物と同じ)', () => {
  const c = Fake.create();
  const s0 = c.snapshot();
  const [t1, t2] = s0.state.tabs;
  c.run('tab.move', { tab: { id: t1.id }, index: 1 });
  assert.strictEqual(c.snapshot().state.tabs[1].id, t1.id);
  c.run('tab.close', { tab: { id: t2.id } });
  assert.strictEqual(c.snapshot().state.closed, 2);
  c.run('tab.reopen');
  const mdn = s0.bookmarks.find((b) => b.title === 'MDN');
  c.run('bookmark.move', { path: mdn.path, folder: '' });
  assert.strictEqual(c.snapshot().bookmarks.find((b) => b.title === 'MDN').folder, '');
  assert.strictEqual(c.snapshot().bookmarks.find((b) => b.title === '受信箱').folder, '毎日'); // 本物と同じく名前
  assert.throws(() => c.run('tab.fly', {}), /偽のコアには無い命令/);
});

test('分割のボタン: 自動の向きはペインの形から、向きはランチャーの引数になる', () => {
  // 左右に分けたタブの右のペイン(幅 8・高さ 9 で縦長)は下へ、分けていないペイン(16:9 で横長)は右へ
  const split = { k: 'split', dir: 'row', ratio: 0.5, a: { id: 1, type: 'web' }, b: { id: 2, type: 'files' } };
  assert.deepStrictEqual(Glue.paneRect(split, 2), { w: 8, h: 9 });
  assert.strictEqual(Glue.autoSplit(Glue.paneRect(split, 2)), 'down');
  assert.strictEqual(Glue.autoSplit(Glue.paneRect({ id: 3 }, 3)), 'right');
  assert.strictEqual(Glue.autoSplit(Glue.paneRect({ k: 'split', dir: 'col', ratio: 0.5, a: { id: 1 }, b: { id: 2 } }, 1)), 'right');

  const core = snap();
  const pane = Glue.values(core, {}).PaneBar.pane;
  assert.deepStrictEqual(Glue.steps('PaneBar', 'split', ['left'], core), [{ launcher: { split: 'row', before: true, pane } }]);
  assert.deepStrictEqual(Glue.steps('PaneBar', 'split', ['down'], core), [{ launcher: { split: 'col', before: false, pane } }]);
  assert.deepStrictEqual(Glue.steps('PaneBar', 'split', ['row'], core), [{ launcher: { split: 'row', before: false, pane } }]);
  assert.strictEqual(Glue.values(core, { splitMode: 'pick' }).PaneBar.splitMode, 'pick');
  assert.strictEqual(Glue.values(core, { splitMode: 'what' }).PaneBar.splitMode, 'auto');
});

test('今のペインは閉じる・分割だけ(Web の戻る・進む・消音は持たない)', () => {
  const core = snap();
  const pb = Glue.values(core, {}).PaneBar;
  assert.deepStrictEqual(Glue.steps('PaneBar', 'close', [], core), [{ cmd: 'pane.close', args: { pane: pb.pane } }]);
  assert.ok(!('canGoBack' in pb) && !('muted' in pb) && !('url' in pb));
  assert.throws(() => Glue.steps('PaneBar', 'back', [], core), /つなぎが無い/);
  // サイドバーは上に 閉じる、下に分割のボタンを描く(細い列では 閉じる を出さない)
  const html = Sidebar.render(partsOf(core));
  assert.ok(!html.includes('PaneBar.back') && html.includes('data-call="PaneBar.close"') && html.includes('class="splitbtn"'));
  const thin = Sidebar.render(partsOf(core, { compact: true }));
  assert.ok(!thin.includes('PaneBar.close') && thin.includes('class="splitbtn"'));
  // 向きを選ぶ動きなら、押すと十字が出る
  assert.ok(Sidebar.render(partsOf(core, { splitMode: 'pick' })).includes('data-menu="split"'));
  assert.ok(Sidebar.render(partsOf(core, { splitMode: 'pick' }), { menu: { kind: 'split', x: 0, y: 0 } }).includes('class="splitpad"'));
});

test('橋はランチャーの引数を作る(左・上はまだ右・下になる)', () => {
  const { launcherArgs } = require('../bridge.js');
  assert.deepStrictEqual(launcherArgs({ split: 'row', before: true, pane: 2 }), ['--split', 'row']);
  assert.deepStrictEqual(launcherArgs({ split: 'col' }), ['--split', 'col']);
  assert.deepStrictEqual(launcherArgs({ pane: 3, url: 'https://example.com' }), ['--pane', '3', '--url', 'https://example.com']);
  assert.deepStrictEqual(launcherArgs({ split: 'x; rm -rf /' }), []);
  assert.deepStrictEqual(launcherArgs({}), []);
});
