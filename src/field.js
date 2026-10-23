'use strict';
// フィールド表示のファイラ。開いたフォルダを1枚の「カード」(中身の一覧)にして、親子のカードを線でつなぎ、左→右の木としてフィールドに並べる。
// カードの中は ↑↓ で選び、→ で子フォルダのカードを右に開き、← で親のカードへ戻る(Shift+← で今のカードを閉じる)。
// Ctrl+↑↓ で、今のカードを兄弟フォルダ(親のカードの次・前のフォルダ)の中身に切り替える。
// 操作中のカードへカメラがなめらかに移動して、画面の中央に置く。開いたカードは残る。
// カードごと(フォルダのパスごと)に表示の設定を持つ: リスト/アイコン、並べ替え、サイズ・更新日時の列。見出しの ︙(またはメニューキー)で切り替える。
// 読み込みは開いたフォルダ1つずつ(fileops.js の fs:list = readdir だけ)。サイズ・日付は表示や並べ替えに要る時だけ fs:stats で読む。
// 読んだ結果はパスごとに覚えておき、カーソルがフォルダに止まったら中身を先読みしておく。
// 開いた履歴の点数(history.js)が高い項目には、カードの中で印を付け(並び順は変えない)、そのフォルダを先読みする。
// app.js より先に読み込まれる。h / ico / btn / api / C / baseName / clamp / popup / fmtSize / commonButtons などは app.js、
// decorate(種類の判定)/ collator / fmtDate は files.js のものを呼び出し時に使う。

const FIELD = {
  ...FIELD_GAPS, CARDW: 320, ROWH: 28, HEAD: 34, VIEWH: 15 * 28, TAU: 90, PREFETCH: 120, CACHE: 400,
  SIZEW: 84, DATEW: 128, TILEW: 100, TILEH: 104, ICONCOLS: 4, THUMBS: 400,
  HOT: 3, HOTMIN: 0.3, // 印を付けるのは、点数が HOTMIN 以上の上位 HOT 件
};
// 種類ごとの色はフォルダペインのアイコン(style.css の .ico.k-*)と同じ
const FIELD_COLORS = { image: '#4f8a5b', video: '#8a4f7d', audio: '#4f6f8a', archive: '#a0712c', code: '#3f7f86', doc: '#a04a3c' };
const fieldSortName = (a, b) => (b.isDir - a.isDir) || a.name.localeCompare(b.name, undefined, { numeric: true, sensitivity: 'base' });

// ------------------------------------------------------------------ 読み込み(キャッシュ・先読み)
const fieldCache = new Map(); // key → entries。古いものから捨てる
const fieldPending = new Map(); // key → 読み込み中の Promise(同じフォルダを二重に読まない)
const fieldKey = (full) => (C.behavior.showHidden ? '1' : '0') + full;

function fieldChildren(full) {
  const key = fieldKey(full);
  if (fieldCache.has(key)) return Promise.resolve({ ok: true, entries: fieldCache.get(key) });
  if (fieldPending.has(key)) return fieldPending.get(key);
  const p = api.fs.list(full, C.behavior.showHidden).then((r) => {
    fieldPending.delete(key);
    if (r.ok) {
      r.entries = r.entries.map((e) => decorate(r.path, e)).sort(fieldSortName);
      fieldCache.set(key, r.entries);
      if (fieldCache.size > FIELD.CACHE) fieldCache.delete(fieldCache.keys().next().value);
    }
    return r;
  });
  fieldPending.set(key, p);
  return p;
}
const fieldForget = (full) => fieldCache.delete(fieldKey(full));

// サイズ・日付(entries に書き込む。読んだものは _st を立てて二重に読まない)
async function fieldStats(fv, c) {
  const list = c.entries;
  const todo = list?.filter((e) => !e._st);
  if (!todo?.length || c.statsBusy) return;
  c.statsBusy = true;
  for (let i = 0; i < todo.length; i += 400) {
    const chunk = todo.slice(i, i + 400);
    const st = await api.fs.stats(c.full, chunk.map((e) => e.name));
    chunk.forEach((e, k) => {
      e._st = true;
      const s = st[k];
      if (!s) return;
      e.size = s[0] < 0 ? undefined : s[0];
      e.mtime = s[1];
      e.dateStr = fmtDate(s[1]);
    });
  }
  c.statsBusy = false;
  if (c.entries === list) { fieldApply(c); fieldSettle(fv, c); }
}

// ------------------------------------------------------------------ 表示の設定(フォルダのパスごと)
const FIELD_VIEW = { mode: 'list', sort: 'name', desc: false, size: false, date: false };
let fieldViews = null; // パス → 設定(変えたフォルダだけ)。PRELUDE の窓どうしで共有する
function fieldViewStore() {
  if (!fieldViews) { try { fieldViews = JSON.parse(localStorage.getItem('fieldViews') || '{}') || {}; } catch { fieldViews = {}; } }
  return fieldViews;
}
function fieldViewSave() { try { localStorage.setItem('fieldViews', JSON.stringify(fieldViews)); } catch {} }
// 設定していないフォルダは、画像が多ければアイコン、それ以外はリスト
function fieldViewOf(c) {
  const v = fieldViewStore()[c.full];
  if (v) return { ...FIELD_VIEW, ...v };
  const files = c.entries?.filter((e) => !e.isDir) || [];
  const img = files.filter((e) => e.kind === 'image').length;
  return { ...FIELD_VIEW, mode: img >= 4 && img >= files.length * 0.6 ? 'icon' : 'list' };
}
const fieldNeedStats = (v) => v.size || v.date || v.sort === 'size' || v.sort === 'date';

function fieldSetView(leaf, c, patch) {
  const st = fieldViewStore();
  if (patch) st[c.full] = { ...fieldViewOf(c), ...patch }; else delete st[c.full];
  fieldViewSave();
  // 同じフォルダのカード(ほかのペイン・窓のものも)に反映する
  for (const t of S.tabs) for (const l of leaves(t.root)) {
    if (l.type !== 'field' || !l.fv?.cards) continue;
    let hit = false;
    for (const k of l.fv.cards) if (k.full === c.full) { fieldApply(k, l.fv); fieldSettle(l.fv, k); hit = true; }
    if (hit && l.fv.cur) fieldCursor(l, l.fv.cur.i);
  }
  leaf.fv.canvas?.focus();
}

// 並べ替えと、カードの形(幅・アイコンの列数・中身の高さ)を決め直す。カーソルと子のカードは同じ項目に残す
function fieldApply(c, fv) {
  const v = c.view = fieldViewOf(c);
  const at = c.list?.[c.i]?.full;
  const d = v.desc ? -1 : 1;
  const byName = (a, b) => collator.compare(a.name, b.name);
  const cmp = v.sort === 'size' ? (a, b) => ((a.size || 0) - (b.size || 0)) * d || byName(a, b)
    : v.sort === 'date' ? (a, b) => ((a.mtime || 0) - (b.mtime || 0)) * d || byName(a, b)
    : v.sort === 'type' ? (a, b) => (collator.compare(a.kind, b.kind) || collator.compare(a.ext, b.ext)) * d || byName(a, b)
    : (a, b) => byName(a, b) * d;
  c.list = (c.entries || []).slice().sort((a, b) => (b.isDir - a.isDir) || (a.isDir && v.sort === 'size' ? byName(a, b) : cmp(a, b)));
  if (at) { const j = c.list.findIndex((e) => e.full === at); if (j >= 0) c.i = j; }
  c.kids = c.kids.filter((k) => (k.row = c.list.findIndex((e) => e.isDir && e.full === k.full)) >= 0);
  // 行き先になりそうな項目(履歴の点数の上位)。読み込み・表示の変更の時だけ決め直し、操作中に印が動かないようにする
  const now = Date.now();
  const hot = c.list.map((e, i) => [i, freqScore(e.full, now)]).filter(([, s]) => s >= FIELD.HOTMIN).sort((a, b) => b[1] - a[1]).slice(0, FIELD.HOT);
  c.hot = new Map(hot.map(([i], r) => [i, r]));
  const icon = v.mode === 'icon';
  c.w = icon ? 12 + FIELD.ICONCOLS * FIELD.TILEW : FIELD.CARDW + (v.size ? FIELD.SIZEW : 0) + (v.date ? FIELD.DATEW : 0);
  c.cols = icon ? FIELD.ICONCOLS : 1;
  const n = c.list.length;
  const nd = c.nd = icon ? (c.list.findIndex((e) => !e.isDir) + 1 || n + 1) - 1 : n;
  c.contentH = nd * FIELD.ROWH + (n > nd ? Math.ceil((n - nd) / c.cols) * FIELD.TILEH + 12 : 0);
  c.viewH = Math.max(FIELD.ROWH, Math.min(c.contentH, FIELD.VIEWH));
  c.h = FIELD.HEAD + c.viewH + 8;
  c.i = clamp(c.i, 0, Math.max(0, n - 1));
  fieldScrollTo(c);
  if (fieldNeedStats(v) && fv) fieldStats(fv, c);
}

// idx の項目の位置(カードの中身の座標)。アイコン表示では、フォルダは上に行で、ファイルは下にタイルで並べる
function fieldItem(c, idx) {
  if (idx < c.nd) return { x: 0, y: idx * FIELD.ROWH, w: c.w, h: FIELD.ROWH, tile: false };
  const k = idx - c.nd, col = k % c.cols, row = Math.floor(k / c.cols);
  return { x: 6 + col * FIELD.TILEW, y: c.nd * FIELD.ROWH + 6 + row * FIELD.TILEH, w: FIELD.TILEW, h: FIELD.TILEH, tile: true, col, row };
}

// ------------------------------------------------------------------ カード
// row: 親のカードの何行目のフォルダか。kids: 開いている子のカード。sy: カードの中のスクロール(px)
function fieldCard(full, parent, row) {
  const c = { full, name: baseName(full) || '/', parent, row, kids: [], entries: null, list: [], err: '', loading: false, i: 0, sy: 0, x: 0, y: 0, dx: null, dy: null, alpha: 1, placed: false };
  fieldApply(c);
  return c;
}
const fieldParentPath = (p) => (p === '/' ? null : p.replace(/\/+[^/]+\/*$/, '') || '/');
// row の行の中心(カードの上端から)。スクロールで見えていない行は、見えている端にそろえる
const fieldRowY = (c, row) => FIELD.HEAD + clamp(fieldItem(c, row).y - c.sy + FIELD.ROWH / 2, FIELD.ROWH / 2, c.viewH - FIELD.ROWH / 2);

function fieldScrollTo(c) {
  if (!c.list.length) { c.sy = 0; return; }
  const it = fieldItem(c, c.i);
  if (it.y < c.sy) c.sy = it.y;
  if (it.y + it.h > c.sy + c.viewH) c.sy = it.y + it.h - c.viewH;
  c.sy = clamp(c.sy, 0, Math.max(0, c.contentH - c.viewH));
}

async function fieldLoad(fv, c, force) {
  if (force) fieldForget(c.full);
  const full = c.full;
  c.loading = true;
  fieldKick(fv);
  const cached = !force && fieldCache.has(fieldKey(full));
  const r = await fieldChildren(full);
  if (c.full !== full) return false; // 読んでいる間に Ctrl+↑↓ で別のフォルダに替わった
  c.loading = false;
  c.entries = r.ok ? r.entries : [];
  c.err = r.ok ? '' : r.error;
  fieldApply(c, fv);
  for (const [i] of c.hot) if (c.list[i].isDir) fieldChildren(c.list[i].full); // よく使うフォルダは先に読んでおく
  // 覚えていた中身(先読み・前に開いた時のもの)は古いかもしれない。カードを開いた時は読み直し、違っていれば差し替える
  // (開いている間の変更は fieldWatch の監視で知る)
  if (cached && r.ok) fieldRevalidate(fv, c, r.entries);
  return true;
}
async function fieldRevalidate(fv, c, old) {
  const full = c.full;
  fieldForget(full);
  const r = await fieldChildren(full);
  if (c.full !== full || !r.ok || !fv.cards.includes(c)) return;
  const same = r.entries.length === old.length && r.entries.every((e, i) => e.name === old[i].name && e.isDir === old[i].isDir);
  if (same) { fieldCache.set(fieldKey(full), old); return; } // 読んであったサイズ・日付を残す
  c.entries = r.entries;
  fieldApply(c, fv);
  fieldSettle(fv, c);
}

// ------------------------------------------------------------------ 配置(一度置いたカードは動かさない)
// 決め方と計算は fieldlayout.js(fieldFreeY / fieldOrderY / fieldPlaceKid / fieldSettleLayout など)

// 木をたどって、描く順のカードの一覧を作り直す
function fieldCollect(fv) {
  const out = [];
  const walk = (c) => { out.push(c); c.kids.forEach(walk); };
  walk(fv.root);
  fv.cards = out;
  fieldWatch(fv);
  fieldKick(fv);
}
// 開いているカードのフォルダを監視して、外で増えた・消えたファイル(画面録画・ダウンロードなど)を反映する。
// 監視の id は「field:ペインの id:フォルダ」。知らせは fieldOnChanged(files.js の initFiles から呼ぶ)
const fieldWatchId = (fv, dir) => `field:${fv.id}:${dir}`;
function fieldWatch(fv) {
  if (!fv.id) return;
  const want = new Set(fv.cards.map((k) => k.full));
  const had = fv.watching || new Set();
  for (const d of had) if (!want.has(d)) api.fs.unwatch(fieldWatchId(fv, d));
  for (const d of want) if (!had.has(d)) api.fs.watch(fieldWatchId(fv, d), d);
  fv.watching = want;
}
function fieldUnwatchAll(fv) {
  for (const d of fv?.watching || []) api.fs.unwatch(fieldWatchId(fv, d));
  if (fv) fv.watching = new Set();
}
function fieldOnChanged(id, dir) {
  let open = false;
  fieldEachLeaf((l) => { if (fieldWatchId(l.fv, dir) === id && l.fv.watching?.has(dir)) open = true; });
  if (!open) { api.fs.unwatch(id); return; }
  if (!fieldEachLeafBusy()) fieldRefresh([dir]);
}
// 名前の変更中は読み直さない(入力欄の位置がずれる)。終わった時の読み直しで反映される
function fieldEachLeafBusy() {
  let busy = false;
  fieldEachLeaf((l) => { if (l.fv.renaming) busy = true; });
  return busy;
}

// 親のカードの行に見出しをそろえた時の c の y
const fieldWantY = (c) => c.parent.y + fieldRowY(c.parent, c.row) - FIELD.HEAD / 2;

// 読み込んで大きさの決まった新しいカードを置く。親のカードのその行から伸びてくる
function fieldPlace(fv, c) {
  const p = c.parent;
  fieldCollect(fv);
  if (p) {
    fieldPlaceKid(fv.cards, c, fieldWantY(c));
    Object.assign(c, { dx: p.dx + p.w - 60, dy: p.dy + fieldRowY(p, c.row) - FIELD.HEAD / 2, alpha: 0 });
  } else Object.assign(c, { x: 0, y: 0, dx: 0, dy: 0, placed: true });
  fieldKick(fv);
}

// c の大きさが変わった後。子・下の兄弟の枝・重なったカードをずらす(fieldlayout.js)
function fieldSettle(fv, c) {
  if (!c.placed) return;
  fieldCollect(fv);
  fieldSettleLayout(fv.cards, c);
  fieldKick(fv);
}

// ------------------------------------------------------------------ 操作
function fieldFocus(leaf, c) {
  const fv = leaf.fv;
  if (fv.cur && fv.cur !== c) fieldClearSel(fv.cur); // 選択は1枚のカードの中だけ
  fv.cur = c;
  fv.follow = true;
  if (leaf.path !== c.full) { leaf.path = c.full; renderSidebar(); scheduleSave(); }
  fieldCursor(leaf, c.i);
}

function fieldCursor(leaf, i) {
  const fv = leaf.fv, c = fv.cur;
  c.i = clamp(i, 0, Math.max(0, c.list.length - 1));
  fieldScrollTo(c);
  const en = c.list[c.i];
  if (fv.ui?.path) fv.ui.path.textContent = en ? en.full : c.full;
  clearTimeout(fv.prefetch);
  if (en?.isDir) fv.prefetch = setTimeout(() => fieldChildren(en.full), FIELD.PREFETCH);
  fieldKick(fv);
}

// カーソルを動かした先。アイコンのタイルの中では ↑↓ は1段、←→ は1つずつ(端では null = 木の移動に任せる)
function fieldStep(c, dir) {
  const i = c.i, n = c.list.length, it = fieldItem(c, i);
  if (!n) return null;
  if (!it.tile) {
    if (dir === 'down') return Math.min(n - 1, i + 1);
    if (dir === 'up') return Math.max(0, i - 1);
    return null;
  }
  const nf = n - c.nd, k = i - c.nd;
  if (dir === 'down') return k + c.cols < nf ? i + c.cols : Math.floor(k / c.cols) < Math.floor((nf - 1) / c.cols) ? n - 1 : i;
  if (dir === 'up') return k >= c.cols ? i - c.cols : c.nd > 0 ? c.nd - 1 : i;
  if (dir === 'left') return it.col > 0 ? i - 1 : null;
  if (dir === 'right') return it.col < c.cols - 1 && k + 1 < nf ? i + 1 : i;
  return null;
}

// カーソルの行を開く。フォルダなら右に子のカードを開いて移り、ファイルなら既定のアプリをタブか分割に取り込んで開く。
// where: ファイルを開く場所('tab' | 'split' | 'window'。省略は設定「ファイルを開く場所」)
async function fieldEnter(leaf, where) {
  const fv = leaf.fv, c = fv.cur, en = c.list[c.i];
  if (!en) return;
  freqRecord(en.full, en.isDir ? 'dir' : 'file');
  if (!en.isDir) { openFileAt(fieldTabOf(leaf), leaf, en.full, where); return; }
  let k = c.kids.find((x) => x.full === en.full);
  if (!k) {
    k = fieldCard(en.full, c, c.i);
    c.kids.push(k);
    fieldCollect(fv);
    await fieldLoad(fv, k); // 大きさが決まってから置く(ふつうは先読みしてあるのですぐ)
    fieldPlace(fv, k);
  }
  if (fv.cur === c && c.i === k.row && k.placed) fieldFocus(leaf, k);
}

// 親のカードへ。一番上のカードなら、ひとつ上のフォルダのカードを左に足す。close: 今のカードを閉じる
async function fieldBack(leaf, close) {
  const fv = leaf.fv, c = fv.cur;
  if (!c.parent && !(await fieldReroot(fv))) return;
  const p = c.parent;
  p.i = c.row;
  if (close) { p.kids = p.kids.filter((k) => k !== c); fieldCollect(fv); }
  fieldFocus(leaf, p);
}

// 今のカードを、兄弟フォルダ(親のカードの次・前のフォルダ)の中身に切り替える
async function fieldSibling(leaf, d) {
  const fv = leaf.fv, c = fv.cur, p = c.parent;
  if (!p?.list.length) return;
  let j = c.row + d;
  while (j >= 0 && j < p.list.length && !p.list[j].isDir) j += d;
  const en = p.list[j];
  if (!en) return;
  freqRecord(en.full, 'dir');
  p.i = j;
  fieldScrollTo(p);
  const open = p.kids.find((k) => k.full === en.full);
  if (open) { // その兄弟がもう開いていれば、そのカードへ移る(今のカードは閉じる)
    p.kids = p.kids.filter((k) => k !== c);
    fieldCollect(fv);
    fieldFocus(leaf, open);
    return;
  }
  Object.assign(c, { full: en.full, name: en.name, row: j, kids: [], entries: null, list: [], err: '', i: 0, sy: 0 });
  fieldApply(c);
  fieldCollect(fv);
  fieldFocus(leaf, c);
  if (!(await fieldLoad(fv, c))) return;
  // 新しく開いた時と同じく、親のカードの行の順を守る場所へ移す(今の場所からなめらかに動く)
  if (c.parent) { fieldCollect(fv); c.y = fieldOrderY(fv.cards, c, fieldWantY(c)); fieldKick(fv); }
  if (fv.cur === c) fieldCursor(leaf, c.i);
}

// ひとつ上のフォルダのカードを根にして、今までの木をその子としてつなぐ
async function fieldReroot(fv) {
  const old = fv.root;
  const up = fieldParentPath(old.full);
  if (!up) return false;
  const root = fieldCard(up, null, 0);
  await fieldLoad(fv, root);
  const row = root.list.findIndex((en) => en.isDir && en.full === old.full);
  if (row < 0) return false; // 隠しフォルダから始めて、隠しファイルを出さない設定の時など
  Object.assign(old, { parent: root, row });
  Object.assign(root, { kids: [old], i: row });
  fieldScrollTo(root);
  fv.root = root;
  // 今の木はそのままで、左に置く(今までの根の見出しが、そのフォルダの行にそろう位置)
  const x = old.x - root.w - FIELD.GAP, y = old.y + FIELD.HEAD / 2 - fieldRowY(root, row);
  Object.assign(root, { x, y, dx: x + 60, dy: y, alpha: 0, placed: true });
  fieldCollect(fv);
  return true;
}

// 今のカードを読み直す(F5)。カーソルの項目と、まだあるフォルダの子のカードは残す
async function fieldReload(leaf) {
  const fv = leaf.fv, c = fv?.cur;
  if (!c) return;
  if (!(await fieldLoad(fv, c, true))) return;
  fieldSettle(fv, c);
  if (fv.cur === c) fieldCursor(leaf, c.i);
}

async function fieldNewTree(leaf, path) {
  const fv = leaf.fv;
  const root = fv.root = fv.cur = fieldCard(path, null, 0);
  fieldCollect(fv);
  await fieldLoad(fv, root);
  if (fv.root !== root) return;
  fieldPlace(fv, root);
  Object.assign(fv.cam, { x: root.w / 2, y: root.h / 2 });
  fieldFocus(leaf, root);
}

// カードの見出しの ︙ の位置(フィールドの座標)
const fieldMenuBtn = (c) => ({ x: c.dx + c.w - 26, y: c.dy + 4, w: 22, h: FIELD.HEAD - 8 });

// 表示の設定のメニュー。x, y は画面の座標
function fieldMenu(leaf, c, x, y) {
  const v = c.view;
  const set = (p) => () => fieldSetView(leaf, c, p);
  const list = v.mode === 'list';
  popup([
    { header: `表示（${c.name}）` },
    { label: 'リスト', checked: list, run: set({ mode: 'list' }) },
    { label: 'アイコン', checked: !list, run: set({ mode: 'icon' }) },
    { sep: true },
    { header: '並べ替え' },
    ...[['name', '名前'], ['date', '更新日時'], ['size', 'サイズ'], ['type', '種類']].map(([k, l]) => ({ label: l, checked: v.sort === k, run: set({ sort: k }) })),
    { label: '逆順', checked: v.desc, run: set({ desc: !v.desc }) },
    { sep: true },
    { header: '列（リストのとき）' },
    { label: 'サイズ', checked: v.size, disabled: !list, run: set({ size: !v.size }) },
    { label: '更新日時', checked: v.date, disabled: !list, run: set({ date: !v.date }) },
    { sep: true },
    { label: 'このフォルダの表示を元に戻す', disabled: !fieldViewStore()[c.full], run: set(null) },
    { label: '開いた履歴を消す（すべてのフォルダ）', run: () => { freqClear(); fieldSetView(leaf, c, fieldViewStore()[c.full] ? {} : null); toast('開いた履歴を消しました'); } },
  ], x, y);
}

function fieldScreen(fv, wx, wy) {
  const r = fv.canvas.getBoundingClientRect(), z = fv.cam.z;
  return [r.left + r.width / 2 + (wx - fv.cam.x) * z, r.top + r.height / 2 + (wy - fv.cam.y) * z];
}

async function fieldOnKey(leaf, e) {
  const fv = leaf.fv, c = fv.cur;
  if (!c || e.metaKey) return;
  const k = e.key;
  if (e.altKey) { // Alt+Enter のほかは、シェルのショートカット(Alt+← など)に任せる
    if (k === 'Enter') { e.preventDefault(); fieldProps(leaf); }
    return;
  }
  const zoom = (f) => { fv.cam.tz = clamp(f ? fv.cam.tz * f : 1, 0.3, 2); fieldKick(fv); };
  // ↑↓・PageUp/Down・Home/End: Shift を押していれば起点から範囲を選び、押していなければ選択を解く
  const go = (i) => {
    if (e.shiftKey) { if (c.anchor == null || !c.sel?.size) c.anchor = c.i; fieldSelRange(c, c.anchor, clamp(i, 0, c.list.length - 1)); } else fieldClearSel(c);
    fieldCursor(leaf, i);
  };
  const step = (dir, times = 1) => { let i = c.i; for (let t = 0; t < times; t++) { const s = fieldStep({ ...c, i }, dir); if (s == null) break; i = s; } go(i); };
  const page = Math.max(1, Math.round(c.viewH / (fieldItem(c, c.i).tile ? FIELD.TILEH : FIELD.ROWH)) - 1);
  if (e.ctrlKey) {
    const kl = k.toLowerCase();
    if (k === 'ArrowDown') fieldSibling(leaf, 1);
    else if (k === 'ArrowUp') fieldSibling(leaf, -1);
    else if (kl === 'c' && e.shiftKey) fieldCopyPath(leaf);
    else if (kl === 'n' && e.shiftKey) fieldNewFolder(leaf);
    else if (kl === 'c') fieldCopy(leaf, false);
    else if (kl === 'x') fieldCopy(leaf, true);
    else if (kl === 'v') fieldPaste(leaf, c.full);
    else if (kl === 'h') toggleHidden();
    else if (kl === 'a') { fieldSelRange(c, 0, c.list.length - 1); fieldKick(fv); }
    else if (k === 'Enter') { const en = fieldEn(leaf); if (en?.isDir) { const tab = fieldTabOf(leaf); if (tab) splitLeaf(tab, leaf, 'row', 'field', { path: en.full }); } else fieldEnter(leaf, 'split'); }
    else return;
  } else if (k === 'F2') fieldStartRename(leaf);
  else if (k === 'Delete') fieldDelete(leaf, e.shiftKey);
  else if (k === 'F4' && e.shiftKey) fieldTerminal(leaf);
  else if (k === 'ArrowDown') step('down');
  else if (k === 'ArrowUp') step('up');
  else if (k === 'PageDown') step('down', page);
  else if (k === 'PageUp') step('up', page);
  else if (k === 'Home') go(0);
  else if (k === 'End') go(Infinity);
  else if (k === ' ') { fieldToggle(c, c.i); fieldKick(fv); }
  else if (k === 'Escape' && c.sel?.size) { fieldClearSel(c); fieldKick(fv); }
  else if (k === 'ArrowRight') {
    const s = fieldStep(c, 'right');
    if (s != null) fieldCursor(leaf, s); else if (c.list[c.i]?.isDir) fieldEnter(leaf);
  } else if (k === 'ArrowLeft') {
    const s = !e.shiftKey && fieldStep(c, 'left');
    if (s != null && s !== false) fieldCursor(leaf, s); else fieldBack(leaf, e.shiftKey);
  } else if (k === 'Enter') fieldEnter(leaf);
  else if (k === 'ContextMenu' || (k === 'F10' && e.shiftKey)) fieldItemMenu(leaf, ...fieldCursorScreen(fv));
  else if (k === 'F5') fieldReload(leaf); // ふつうは main.js のショートカットとして届く(app.js の onShortcut)
  else if (k === '+' || k === '=') zoom(1.2);
  else if (k === '-') zoom(1 / 1.2);
  else if (k === '0') zoom(0);
  else if (k.length === 1 && k !== ' ' && c.list.length) {
    // 文字を打つと、その文字で始まる次の項目へ飛ぶ
    const q = k.toLowerCase(), list = c.list;
    for (let d = 1; d <= list.length; d++) { const j = (c.i + d) % list.length; if (list[j].name.toLowerCase().startsWith(q)) { fieldCursor(leaf, j); break; } }
  } else return;
  e.preventDefault();
}

// ------------------------------------------------------------------ ファイルの操作(右クリック・キー)
// 対象はカーソルの項目(項目が無ければカードのフォルダ)。操作の後は、そのフォルダを表示しているカードをすべて読み直す。
// 中身は fileops.js(api.fs)、確認・プロパティ・端末は files.js の fileConfirm / showPropsFor / openTerminalAt を使う。
const fieldEn = (leaf) => { const c = leaf.fv.cur; return c?.list[c.i] || null; };
// 複数選択。選べるのは1枚のカードの中だけ(c.sel = 選んだ項目のパス。c.anchor = 範囲選択の起点)。
// 何も選んでいなければ、カーソルの項目だけが対象
const fieldSel = (c) => c.sel || (c.sel = new Set());
function fieldClearSel(c) { if (c?.sel?.size) c.sel.clear(); if (c) c.anchor = null; }
function fieldSelRange(c, from, to) {
  const s = fieldSel(c);
  s.clear();
  for (let j = Math.min(from, to); j <= Math.max(from, to); j++) if (c.list[j]) s.add(c.list[j].full);
}
function fieldToggle(c, idx) {
  const s = fieldSel(c), en = c.list[idx];
  if (!en) return;
  if (s.has(en.full)) s.delete(en.full); else s.add(en.full);
  c.anchor = idx;
}
function fieldTargets(leaf) {
  const c = leaf.fv.cur, s = c?.sel;
  const picked = s?.size ? c.list.filter((e) => s.has(e.full)) : [];
  if (picked.length) return picked;
  const en = fieldEn(leaf);
  return en ? [en] : [];
}
const fieldNames = (list) => (list.length === 1 ? `「${list[0].name}」` : `${list.length} 個の項目`);
const fieldTabOf = (leaf) => findLeaf(leaf.id)?.tab;

function fieldEachLeaf(fn) {
  for (const t of S.tabs) for (const l of leaves(t.root)) if (l.type === 'field' && l.fv?.cards?.length) fn(l);
}
const fieldKickAll = () => fieldEachLeaf((l) => fieldKick(l.fv));

// dirs のどれかを表示しているカードを読み直す。消えたカードの中にいたら、残っている一番近い親のカードへ移る
async function fieldRefresh(dirs) {
  const set = new Set(dirs.filter(Boolean));
  for (const d of set) fieldForget(d);
  const jobs = [];
  fieldEachLeaf((l) => jobs.push((async () => {
    const fv = l.fv;
    for (const c of fv.cards) if (set.has(c.full) && (await fieldLoad(fv, c))) fieldSettle(fv, c);
    fieldCollect(fv);
    let c = fv.cur;
    while (c && !fv.cards.includes(c)) c = c.parent;
    if (c && c !== fv.cur) fieldFocus(l, c); else if (fv.cur) fieldCursor(l, fv.cur.i);
  })()));
  await Promise.all(jobs);
}
// 隠しファイルの表示を切り替えた時など: すべてのカードを読み直す
function fieldReloadAll() {
  fieldEachLeaf((l) => { for (const c of l.fv.cards) fieldLoad(l.fv, c).then((ok) => { if (ok) { fieldSettle(l.fv, c); if (l.fv.cur === c) fieldCursor(l, c.i); } }); });
}
// カード c のカーソルを name の項目へ
function fieldCursorTo(leaf, c, name) {
  const j = c.list.findIndex((e) => e.name === name);
  if (j < 0) return;
  c.i = j;
  if (leaf.fv.cur === c) fieldCursor(leaf, j);
}

async function fieldCopy(leaf, cut) {
  const list = fieldTargets(leaf);
  if (!list.length) return;
  const paths = list.map((e) => e.full);
  await api.fs.clipWrite(paths, cut);
  S.fileClip = cut ? { paths: new Set(paths), cut } : null;
  reloadAllPaint();
  fieldKickAll();
  toast(`${fieldNames(list)}を${cut ? '切り取りました' : 'コピーしました'}`);
}

async function fieldPaste(leaf, into) {
  const c = leaf.fv.cur;
  const r = await api.fs.clipRead();
  if (!r.paths.length) { toast('貼り付けるファイルがありません'); return; }
  const mode = r.cut ? 'move' : 'copy';
  toast(`${mode === 'move' ? '移動' : 'コピー'}中…`, 60000);
  const res = await api.fs.transfer(mode, r.paths, into);
  if (r.cut) { S.fileClip = null; reloadAllPaint(); }
  await fieldRefresh([into, ...(r.cut ? r.paths.map(fieldParentPath) : [])]);
  if (res.names.length && c.full === into) fieldCursorTo(leaf, c, res.names[0]);
  if (res.errors.length) toast(`${res.errors.length} 件失敗: ${res.errors[0]}`);
  else toast(`${res.names.length} 個を${mode === 'move' ? '移動' : 'コピー'}しました`);
}

async function fieldDelete(leaf, permanent) {
  const c = leaf.fv.cur, list = fieldTargets(leaf);
  if (!list.length) return;
  const shown = list.slice(0, 10).map((e) => e.name).join('\n') + (list.length > 10 ? `\nほか ${list.length - 10} 個` : '');
  if (permanent && !(await fileConfirm(`完全に削除します（元に戻せません）\n${shown}`, '削除', true))) return;
  const r = await api.fs.remove(list.map((e) => e.full), permanent);
  fieldClearSel(c);
  await fieldRefresh([c.full]);
  if (!r.ok) toast(`削除できませんでした: ${r.errors[0]}${permanent ? '' : '（Shift+Delete で完全に削除）'}`);
  else toast(`${fieldNames(list)}を${permanent ? '削除しました' : 'ゴミ箱へ移動しました'}`);
}

async function fieldNewFolder(leaf) {
  const c = leaf.fv.cur;
  const r = await api.fs.mkdir(c.full, '新しいフォルダ');
  if (!r.ok) { toast(r.error); return; }
  await fieldRefresh([c.full]);
  fieldCursorTo(leaf, c, r.name);
  fieldStartRename(leaf);
}

// 名前の変更: カーソルの項目の上に入力欄を重ねる。Enter・欄の外で確定、Esc で取り消し
function fieldStartRename(leaf) {
  const fv = leaf.fv, c = fv.cur, en = fieldEn(leaf);
  if (!en || fv.renaming) return;
  // 動きを止めて、入力欄を項目にぴったり重ねる
  fieldScrollTo(c);
  if (fv.follow) { fv.cam.x = fv.cam.tx; fv.cam.y = fv.cam.ty; }
  fv.cam.z = fv.cam.tz;
  c.dx = c.x; c.dy = c.y;
  fieldDraw(fv);
  const it = fieldItem(c, c.i), z = fv.cam.z;
  const lx = c.dx + it.x + (it.tile ? 6 : 26), ly = c.dy + FIELD.HEAD - c.sy + it.y + (it.tile ? 76 : 2);
  const [sx, sy] = fieldScreen(fv, lx, ly);
  const inp = h('input', { class: 'field-ren', value: en.name, spellcheck: 'false' });
  Object.assign(inp.style, { left: sx + 'px', top: sy + 'px', width: Math.max(120, ((it.tile ? it.w - 12 : c.w - 40) * z)) + 'px', height: 24 * z + 'px', fontSize: C.appearance.fontSize * z + 'px' });
  document.body.append(inp);
  fv.renaming = inp;
  inp.focus();
  const dot = en.isDir ? -1 : en.name.lastIndexOf('.');
  inp.setSelectionRange(0, dot > 0 ? dot : en.name.length); // 拡張子は選ばない
  let done = false;
  const finish = async (ok) => {
    if (done) return;
    done = true;
    fv.renaming = null;
    const val = inp.value.trim();
    inp.remove();
    fv.canvas?.focus();
    if (!ok || !val || val === en.name) return;
    const r = await api.fs.rename(en.full, val);
    if (!r.ok) { toast(r.error); return; }
    await fieldRefresh([c.full]);
    fieldCursorTo(leaf, c, r.name);
  };
  inp.addEventListener('keydown', (e) => {
    e.stopPropagation();
    if (e.key === 'Enter') { e.preventDefault(); finish(true); } else if (e.key === 'Escape') { e.preventDefault(); finish(false); }
  });
  inp.addEventListener('blur', () => finish(true));
}

function fieldCopyPath(leaf) {
  const list = fieldTargets(leaf);
  const p = list.length ? list.map((e) => e.full).join('\n') : leaf.fv.cur.full; // 複数なら1行に1つ
  api.fs.copyText(p).then(() => toast(list.length > 1 ? `${list.length} 個のパスをコピーしました` : 'パスをコピーしました'));
}
const fieldTerminal = (leaf) => { const en = fieldEn(leaf); openTerminalAt(fieldTabOf(leaf), leaf, en?.isDir ? en.full : leaf.fv.cur.full); };
const fieldProps = (leaf, ofCard) => { const c = leaf.fv.cur, list = ofCard ? [] : fieldTargets(leaf); showPropsFor(list.length ? list.map((e) => e.full) : [c.full], c.full); };

// カードを閉じる(どのカードでも)。中にいたら、その親のカードへ移る
function fieldCloseCard(leaf, c) {
  const fv = leaf.fv, p = c.parent;
  if (!p) return;
  p.kids = p.kids.filter((k) => k !== c);
  fieldCollect(fv);
  if (!fv.cards.includes(fv.cur)) { p.i = c.row; fieldFocus(leaf, p); }
}

// 項目の右クリック(メニューキー)
function fieldItemMenu(leaf, x, y) {
  const c = leaf.fv.cur, en = fieldEn(leaf);
  if (!en) { fieldCardMenu(leaf, c, x, y); return; }
  const tab = fieldTabOf(leaf), n = fieldTargets(leaf).length;
  popup([
    n > 1 ? { header: `${n} 個を選択中(切り取り・コピー・削除・パスのコピー・プロパティは全部が対象)` } : null,
    { label: en.isDir ? 'カードを開く' : '開く', icon: en.isDir ? 'folder' : 'file', key: 'Enter', run: () => fieldEnter(leaf) },
    en.isDir ? null : { label: '新しいタブで開く', icon: 'plus', run: () => fieldEnter(leaf, 'tab') },
    en.isDir || !tab ? null : { label: '右に分割して開く', icon: 'split-h', key: 'Ctrl+Enter', run: () => fieldEnter(leaf, 'split') },
    en.isDir ? null : { label: '別のウィンドウで開く', icon: 'window', run: () => fieldEnter(leaf, 'window') },
    en.isDir ? { label: '新しいタブで開く', icon: 'field', key: '中クリック', run: () => openTab('field', { path: en.full }) } : null,
    en.isDir && tab ? { label: 'フォルダ表示で右に開く', icon: 'split-h', run: () => splitLeaf(tab, leaf, 'row', 'files', { path: en.full }) } : null,
    { sep: true },
    { label: '切り取り', key: 'Ctrl+X', run: () => fieldCopy(leaf, true) },
    { label: 'コピー', key: 'Ctrl+C', run: () => fieldCopy(leaf, false) },
    { label: '貼り付け', key: 'Ctrl+V', run: () => fieldPaste(leaf, c.full) },
    en.isDir ? { label: 'このフォルダの中に貼り付け', run: () => fieldPaste(leaf, en.full) } : null,
    { label: '名前を変更', icon: 'text', key: 'F2', run: () => fieldStartRename(leaf) },
    { sep: true },
    { label: 'ゴミ箱へ移動', icon: 'close', key: 'Delete', run: () => fieldDelete(leaf, false) },
    { label: '完全に削除', danger: true, key: 'Shift+Del', run: () => fieldDelete(leaf, true) },
    { sep: true },
    { label: 'パスをコピー', key: 'Ctrl+Shift+C', run: () => fieldCopyPath(leaf) },
    { label: '端末で開く', key: 'Shift+F4', run: () => fieldTerminal(leaf) },
    { label: 'プロパティ', key: 'Alt+Enter', run: () => fieldProps(leaf) },
  ].filter(Boolean), x, y);
}

// カードの見出し・空いた所の右クリック(そのフォルダへの操作)
function fieldCardMenu(leaf, c, x, y) {
  popup([
    { header: c.name },
    { label: '新しいフォルダ', icon: 'folder', key: 'Ctrl+Shift+N', run: () => fieldNewFolder(leaf) },
    { label: '貼り付け', key: 'Ctrl+V', run: () => fieldPaste(leaf, c.full) },
    { sep: true },
    { label: '表示の設定…', key: '︙', run: () => fieldMenu(leaf, c, x, y) },
    { label: '隠しファイルを表示', checked: !!C.behavior.showHidden, key: 'Ctrl+H', run: toggleHidden },
    { label: '再読込', icon: 'reload', key: 'F5', run: () => fieldReload(leaf) },
    { sep: true },
    { label: 'パスをコピー', run: () => api.fs.copyText(c.full).then(() => toast('パスをコピーしました')) },
    { label: '端末で開く', run: () => openTerminalAt(fieldTabOf(leaf), leaf, c.full) },
    { label: 'プロパティ', run: () => fieldProps(leaf, true) },
    c.parent ? { sep: true } : null,
    c.parent ? { label: 'このカードを閉じる', icon: 'close', key: 'Shift+←', run: () => fieldCloseCard(leaf, c) } : null,
  ].filter(Boolean), x, y);
}

// 何もない所の右クリック
function fieldSpaceMenu(leaf, x, y) {
  const fv = leaf.fv;
  popup([
    { label: '操作中のカードへ戻る', run: () => { fv.follow = true; fieldKick(fv); fv.canvas.focus(); } },
    { label: '拡大を元に戻す', key: '0', run: () => { fv.cam.tz = 1; fieldKick(fv); } },
    { sep: true },
    { label: 'ホームから始め直す', icon: 'home', run: async () => fieldNewTree(leaf, await api.fs.home()) },
    { label: '隠しファイルを表示', checked: !!C.behavior.showHidden, key: 'Ctrl+H', run: toggleHidden },
  ], x, y);
}

// カーソルの項目の下(画面の座標)。キーボードで開くメニューの位置
function fieldCursorScreen(fv) {
  const c = fv.cur, it = fieldItem(c, c.i);
  return fieldScreen(fv, c.dx + it.x + 30, c.dy + FIELD.HEAD - c.sy + it.y + it.h);
}

// ------------------------------------------------------------------ サムネイル(アイコン表示の画像)
const fieldThumbs = new Map(); // パス → ImageBitmap | null(読めない) | 'loading'。古いものから捨てる
const fieldThumbQ = [];
let fieldThumbRun = 0;
function fieldThumb(fv, en) {
  const t = fieldThumbs.get(en.full);
  if (t !== undefined) return t === 'loading' ? null : t;
  fieldThumbs.set(en.full, 'loading');
  fieldThumbQ.push([fv, en]);
  fieldThumbPump();
  return null;
}
function fieldThumbPump() {
  while (fieldThumbRun < 4 && fieldThumbQ.length) {
    const [fv, en] = fieldThumbQ.pop(); // 新しく頼まれたもの(今見えているもの)から
    fieldThumbRun++;
    const img = new Image();
    const done = (bmp) => {
      fieldThumbs.set(en.full, bmp);
      if (fieldThumbs.size > FIELD.THUMBS) { const [k, v] = fieldThumbs.entries().next().value; v?.close?.(); fieldThumbs.delete(k); }
      fieldThumbRun--;
      fieldKick(fv);
      fieldThumbPump();
    };
    img.onload = () => {
      const s = Math.min(1, 160 / Math.max(img.naturalWidth, img.naturalHeight, 1));
      createImageBitmap(img, { resizeWidth: Math.max(1, Math.round(img.naturalWidth * s)), resizeHeight: Math.max(1, Math.round(img.naturalHeight * s)), resizeQuality: 'medium' })
        .then(done, () => done(null));
    };
    img.onerror = () => done(null);
    img.src = 'file://' + en.full.split('/').map(encodeURIComponent).join('/');
  }
}

// ------------------------------------------------------------------ 描画
function fieldKick(fv) {
  if (!fv.raf) fv.raf = requestAnimationFrame((t) => fieldTick(fv, t));
}

function fieldTick(fv, t) {
  fv.raf = 0;
  if (!fv.canvas?.isConnected) { fv.last = 0; return; }
  const dt = fv.last ? Math.min(64, t - fv.last) : 0;
  fv.last = t;
  const k = 1 - Math.exp(-dt / FIELD.TAU);
  let moving = false;
  const ease = (o, a, b, eps = 0.3) => {
    const d = b - o[a];
    if (Math.abs(d) < eps) { o[a] = b; return; }
    o[a] += d * k;
    moving = true;
  };
  const cam = fv.cam;
  if (fv.follow && fv.cur) { cam.tx = fv.cur.x + fv.cur.w / 2; cam.ty = fv.cur.y + fv.cur.h / 2; }
  if (fv.follow) { ease(cam, 'x', cam.tx); ease(cam, 'y', cam.ty); }
  ease(cam, 'z', cam.tz, 0.001);
  for (const c of fv.cards) if (c.placed) { ease(c, 'dx', c.x); ease(c, 'dy', c.y); ease(c, 'alpha', 1, 0.01); }
  fieldDraw(fv);
  if (moving) fieldKick(fv); else fv.last = 0;
}

// 幅に収まるよう末尾を … で切る
function fieldFit(ctx, s, w) {
  if (ctx.measureText(s).width <= w) return s;
  let lo = 0, hi = s.length;
  while (lo < hi) { const m = (lo + hi + 1) >> 1; if (ctx.measureText(s.slice(0, m) + '…').width <= w) lo = m; else hi = m - 1; }
  return s.slice(0, lo) + '…';
}

function fieldDraw(fv) {
  const { canvas, cam } = fv;
  const ctx = canvas.getContext('2d');
  const W = canvas.clientWidth, H = canvas.clientHeight;
  const dpr = window.devicePixelRatio || 1;
  if (canvas.width !== Math.round(W * dpr) || canvas.height !== Math.round(H * dpr)) { canvas.width = Math.round(W * dpr); canvas.height = Math.round(H * dpr); }
  const a = C.appearance;
  const font = `${a.fontSize}px ${getComputedStyle(document.body).fontFamily}`;
  ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
  ctx.fillStyle = a.bg;
  ctx.fillRect(0, 0, W, H);
  const z = cam.z;
  ctx.setTransform(dpr * z, 0, 0, dpr * z, dpr * (W / 2 - cam.x * z), dpr * (H / 2 - cam.y * z));
  const view = { x0: cam.x - W / 2 / z, x1: cam.x + W / 2 / z, y0: cam.y - H / 2 / z, y1: cam.y + H / 2 / z };
  ctx.font = font;
  ctx.textBaseline = 'middle';

  // 線: 親のカードのフォルダの行 → 子のカードの見出し。操作中のカードへの道筋は強調する
  const path = new Set();
  for (let c = fv.cur; c; c = c.parent) path.add(c);
  for (const c of fv.cards) {
    const p = c.parent;
    if (!p || !c.placed || !p.placed) continue;
    const ax = p.dx + p.w, ay = p.dy + fieldRowY(p, c.row), bx = c.dx, by = c.dy + FIELD.HEAD / 2;
    if (bx < view.x0 || ax > view.x1 || Math.max(ay, by) < view.y0 || Math.min(ay, by) > view.y1) continue;
    const on = path.has(c);
    ctx.strokeStyle = on ? a.accent : a.line;
    ctx.lineWidth = on ? 2 : 1.4;
    ctx.globalAlpha = c.alpha;
    ctx.beginPath();
    ctx.moveTo(ax, ay);
    const mx = (ax + bx) / 2;
    ctx.bezierCurveTo(mx, ay, mx, by, bx, by);
    ctx.stroke();
  }
  ctx.globalAlpha = 1;
  for (const c of fv.cards) {
    if (!c.placed || c.dx > view.x1 || c.dx + c.w < view.x0 || c.dy > view.y1 || c.dy + c.h < view.y0) continue;
    fieldDrawCard(fv, ctx, a, c, font, path.has(c));
  }
  ctx.globalAlpha = 1;
}

function fieldDrawCard(fv, ctx, a, c, font, onPath) {
  const x = c.dx, y = c.dy, w = c.w, h = c.h, R = FIELD.ROWH, HD = FIELD.HEAD, v = c.view;
  const focus = c === fv.cur;
  ctx.globalAlpha = c.alpha;
  ctx.save();
  ctx.shadowColor = 'rgba(0,0,0,.16)';
  ctx.shadowBlur = focus ? 22 : 12;
  ctx.shadowOffsetY = focus ? 5 : 3;
  ctx.fillStyle = a.panel;
  ctx.beginPath();
  ctx.roundRect(x, y, w, h, 10);
  ctx.fill();
  ctx.restore();
  ctx.strokeStyle = focus ? a.accent : a.line;
  ctx.lineWidth = focus ? 2 : 1;
  ctx.stroke();

  // 見出し: フォルダ名・件数・︙(表示の設定)
  const n = c.list.length;
  const count = c.loading ? '…' : c.err ? '開けません' : String(n);
  const cw = ctx.measureText(count).width;
  ctx.fillStyle = a.accent;
  ctx.beginPath();
  ctx.arc(x + 16, y + HD / 2, 5, 0, Math.PI * 2);
  ctx.fill();
  ctx.fillStyle = a.text;
  ctx.fillText(fieldFit(ctx, c.name, w - 72 - cw), x + 28, y + HD / 2);
  ctx.fillStyle = c.err ? a.danger : a.dim;
  ctx.fillText(count, x + w - 34 - cw, y + HD / 2);
  const mb = fieldMenuBtn(c);
  if (fv.hoverMenu === c) { ctx.fillStyle = a.panel2; ctx.beginPath(); ctx.roundRect(mb.x, mb.y, mb.w, mb.h, 5); ctx.fill(); }
  ctx.fillStyle = fieldViewStore()[c.full] ? a.accent : a.dim; // 表示を変えたフォルダは ︙ に色を付ける
  for (let d = -1; d <= 1; d++) { ctx.beginPath(); ctx.arc(mb.x + mb.w / 2, mb.y + mb.h / 2 + d * 5, 1.6, 0, Math.PI * 2); ctx.fill(); }
  ctx.strokeStyle = a.line;
  ctx.lineWidth = 1;
  ctx.beginPath();
  ctx.moveTo(x + 8, y + HD - 0.5);
  ctx.lineTo(x + w - 8, y + HD - 0.5);
  ctx.stroke();

  if (!n) {
    ctx.fillStyle = c.err ? a.danger : a.dim;
    ctx.fillText(c.loading ? '読み込み中…' : c.err ? fieldFit(ctx, c.err, w - 28) : '空', x + 14, y + HD + R / 2);
    return;
  }
  const open = new Set(c.kids.map((k) => k.row));
  const top = y + HD - c.sy; // 中身の座標 → フィールドの座標
  ctx.save();
  ctx.beginPath();
  ctx.rect(x, y + HD, w, c.viewH + 4);
  ctx.clip();
  // 見えている項目だけ描く
  const first = (() => {
    const r0 = Math.floor(c.sy / R);
    if (r0 < c.nd) return r0;
    return c.nd + Math.max(0, Math.floor((c.sy - c.nd * R - 6) / FIELD.TILEH)) * c.cols;
  })();
  const colsW = (v.size ? FIELD.SIZEW : 0) + (v.date ? FIELD.DATEW : 0);
  for (let idx = first; idx < n; idx++) {
    const it = fieldItem(c, idx);
    if (it.y > c.sy + c.viewH) break;
    const en = c.list[idx], ix = x + it.x, iy = top + it.y, sel = idx === c.i;
    const base = S.fileClip?.cut && S.fileClip.paths.has(en.full) ? c.alpha * 0.45 : c.alpha; // 切り取り中は薄く
    ctx.globalAlpha = base;
    const marked = !!c.sel?.has(en.full);
    if (marked || (sel && (focus || onPath || open.has(idx)))) {
      ctx.fillStyle = a.panel2;
      ctx.beginPath();
      ctx.roundRect(ix + 5, iy + 2, it.w - 10, it.h - 3, 6);
      ctx.fill();
      if (sel && focus) { ctx.strokeStyle = a.accent; ctx.lineWidth = 1.5; ctx.stroke(); }
    }
    const col = en.isDir ? a.accent : FIELD_COLORS[en.kind] || a.dim;
    if (!it.tile) {
      const ry = iy + R / 2;
      const rank = c.hot.get(idx);
      if (rank != null) { // 行き先になりそうな行の印(左端の縦線。上位ほど濃い)
        ctx.fillStyle = a.accent;
        ctx.globalAlpha = base * [1, 0.7, 0.45][rank];
        ctx.beginPath();
        ctx.roundRect(ix + 4, iy + 7, 3, R - 14, 1.5);
        ctx.fill();
        ctx.globalAlpha = base;
      }
      ctx.beginPath();
      ctx.arc(ix + 18, ry, en.isDir ? 4.5 : 3.5, 0, Math.PI * 2);
      if (open.has(idx)) { ctx.strokeStyle = col; ctx.lineWidth = 2; ctx.stroke(); } else { ctx.fillStyle = col; ctx.fill(); }
      const lw = FIELD.CARDW - 60;
      if (en._lf !== font) { en._lf = font; en._lab = fieldFit(ctx, en.name, lw); }
      ctx.fillStyle = en.isDir || sel || marked ? a.text : a.dim;
      ctx.fillText(en._lab, ix + 30, ry);
      if (colsW) {
        ctx.fillStyle = a.dim;
        let rx = ix + w - 14;
        if (v.date) { const s = en.dateStr || ''; ctx.fillText(s, rx - ctx.measureText(s).width, ry); rx -= FIELD.DATEW; }
        if (v.size && !en.isDir && en.size != null) { const s = fmtSize(en.size); ctx.fillText(s, rx - ctx.measureText(s).width, ry); }
      }
      if (en.isDir) { ctx.fillStyle = a.dim; ctx.fillText('›', ix + FIELD.CARDW - (colsW ? 22 : 18), ry); }
    } else {
      // タイル: サムネイル(画像)か種類の色の板 + 名前
      const bx = ix + 10, by = iy + 8, bw = it.w - 20, bh = 64;
      const bmp = en.kind === 'image' ? fieldThumb(fv, en) : null;
      if (bmp) {
        const s = Math.min(bw / bmp.width, bh / bmp.height);
        const dw = bmp.width * s, dh = bmp.height * s;
        ctx.drawImage(bmp, bx + (bw - dw) / 2, by + (bh - dh) / 2, dw, dh);
      } else {
        ctx.fillStyle = col;
        ctx.globalAlpha = base * 0.22;
        ctx.beginPath();
        ctx.roundRect(bx + 8, by + 4, bw - 16, bh - 8, 8);
        ctx.fill();
        ctx.globalAlpha = base;
        ctx.fillStyle = col;
        const ext = (en.ext || '').toUpperCase().slice(0, 5);
        if (ext) ctx.fillText(ext, bx + bw / 2 - ctx.measureText(ext).width / 2, by + bh / 2);
      }
      const rank = c.hot.get(idx);
      if (rank != null) {
        ctx.fillStyle = a.accent;
        ctx.globalAlpha = base * [1, 0.7, 0.45][rank];
        ctx.beginPath();
        ctx.arc(bx + bw - 4, by + 4, 4.5, 0, Math.PI * 2);
        ctx.fill();
        ctx.globalAlpha = base;
      }
      if (en._tf !== font) { en._tf = font; en._tlab = fieldFit(ctx, en.name, it.w - 12); }
      ctx.fillStyle = sel ? a.text : a.dim;
      ctx.fillText(en._tlab, ix + it.w / 2 - ctx.measureText(en._tlab).width / 2, iy + 8 + bh + 16);
    }
  }
  ctx.globalAlpha = c.alpha;
  ctx.restore();
  // 続きがある時は右端にスクロールの目安
  if (c.contentH > c.viewH) {
    const th = Math.max(16, (c.viewH - 4) * (c.viewH / c.contentH));
    const ty = y + HD + 2 + (c.viewH - 4 - th) * (c.sy / (c.contentH - c.viewH));
    ctx.fillStyle = a.line;
    ctx.beginPath();
    ctx.roundRect(x + w - 6, ty, 3, th, 1.5);
    ctx.fill();
  }
}

// 画面の点 → { カード, 項目 }(見出しの上は項目 -1、︙ の上は menu: true)。カードの外なら null
// ドラッグ中にマウスの横に出す札(ファイル名)。data URL で返す
function fieldDragImage(list) {
  const en = list[0];
  const a = C.appearance; // 等倍で描く(nativeImage は data URL を等倍の画像として読む)
  const font = `${a.fontSize}px ${getComputedStyle(document.body).fontFamily}`;
  const cv = document.createElement('canvas'), ctx = cv.getContext('2d');
  ctx.font = font;
  const label = list.length > 1 ? `📄 ${en.name} ほか ${list.length - 1} 個` : (en.isDir ? '📁 ' : '📄 ') + en.name;
  const w = Math.min(320, Math.ceil(ctx.measureText(label).width) + 20), hgt = a.fontSize + 14;
  cv.width = w; cv.height = hgt;
  ctx.fillStyle = a.panel;
  ctx.beginPath(); ctx.roundRect(0.5, 0.5, w - 1, hgt - 1, 6); ctx.fill();
  ctx.strokeStyle = a.line; ctx.stroke();
  ctx.font = font; ctx.fillStyle = a.text; ctx.textBaseline = 'middle';
  ctx.fillText(fieldFit(ctx, label, w - 20), 10, hgt / 2);
  return cv.toDataURL('image/png');
}

function fieldHit(fv, ev) {
  const r = fv.canvas.getBoundingClientRect();
  const z = fv.cam.z;
  const wx = (ev.clientX - r.left - r.width / 2) / z + fv.cam.x;
  const wy = (ev.clientY - r.top - r.height / 2) / z + fv.cam.y;
  for (let i = fv.cards.length - 1; i >= 0; i--) {
    const c = fv.cards[i];
    if (!c.placed || wx < c.dx || wx > c.dx + c.w || wy < c.dy || wy > c.dy + c.h) continue;
    const mb = fieldMenuBtn(c);
    if (wx >= mb.x && wx <= mb.x + mb.w && wy >= mb.y && wy <= mb.y + mb.h) return { c, row: -1, menu: true };
    const cy = wy - c.dy - FIELD.HEAD + c.sy, cx = wx - c.dx;
    if (wy - c.dy < FIELD.HEAD || cy > c.contentH) return { c, row: -1 };
    let idx;
    if (cy < c.nd * FIELD.ROWH) idx = Math.floor(cy / FIELD.ROWH);
    else {
      const col = Math.floor((cx - 6) / FIELD.TILEW), row = Math.floor((cy - c.nd * FIELD.ROWH - 6) / FIELD.TILEH);
      idx = col >= 0 && col < c.cols ? c.nd + row * c.cols + col : -1;
    }
    return { c, row: idx >= 0 && idx < c.list.length ? idx : -1 };
  }
  return null;
}

// ------------------------------------------------------------------ ペイン
function buildField(tab, leaf, head, body) {
  const home = btn('home', 'ホーム', async () => { fieldNewTree(leaf, await api.fs.home()); canvas.focus(); });
  const path = h('span', { class: 'ptitle field-path' });
  head.append(ico('field'), path, home, ...commonButtons(tab, leaf));

  const canvas = h('canvas', { class: 'field', tabindex: '0' });
  const hint = h('div', { class: 'field-hint' }, '↑↓ 選ぶ　→ フォルダを開く　← 親へ（Shift で閉じる）　Ctrl+↑↓ 兄弟フォルダへ　Enter・ダブルクリック 開く　Ctrl・Shift+クリック 複数選択　ドラッグ 他のアプリへ　右クリック・メニューキー 操作　︙ 表示の設定　F2 名前の変更　Delete ゴミ箱へ');
  body.append(canvas, hint);

  const fresh = !leaf.fv?.cards;
  if (fresh) leaf.fv = { root: null, cur: null, cards: [], cam: { x: 0, y: 0, z: 1, tx: 0, ty: 0, tz: 1 }, follow: true, raf: 0, last: 0 };
  const fv = leaf.fv;
  fv.canvas = canvas;
  fv.id = leaf.id;
  fv.ui = { path };
  if (fv.raf) { cancelAnimationFrame(fv.raf); fv.raf = 0; }
  fv.last = 0;

  canvas.addEventListener('keydown', (e) => fieldOnKey(leaf, e));
  canvas.addEventListener('pointerdown', (e) => {
    canvas.focus();
    const hit = fieldHit(fv, e);
    // 項目の上で押したときだけ、そのままドラッグすると項目を外へ渡せる(何もない所のドラッグはフィールドを動かす)
    fv.dragFrom = e.button === 0 && hit && hit.row >= 0 ? hit.c.list[hit.row] : null;
    canvas.draggable = !!fv.dragFrom;
    fv.clearOnClick = false;
    if (hit) {
      const prev = fv.cur;
      if (hit.c !== fv.cur) fieldFocus(leaf, hit.c);
      if (hit.menu) { e.preventDefault(); fieldMenu(leaf, hit.c, e.clientX, e.clientY + 8); return; }
      if (hit.row >= 0) {
        const c = hit.c, en = c.list[hit.row];
        // Shift+クリック: 起点から範囲を選ぶ。Ctrl+クリック: 1つずつ足し引き(初めは、それまでのカーソルの項目も入れる)。
        // ただのクリック: 選択を解く。選んである項目の上なら、ドラッグで全部を渡せるように、離した時(ドラッグしなかった時)に解く
        if (e.button === 0 && e.shiftKey) { if (c.anchor == null || !c.sel?.size) c.anchor = prev === c ? c.i : hit.row; fieldSelRange(c, c.anchor, hit.row); }
        else if (e.button === 0 && e.ctrlKey) { if (!c.sel?.size && prev === c && c.i !== hit.row) fieldToggle(c, c.i); fieldToggle(c, hit.row); }
        else if (e.button === 0) { if (c.sel?.has(en.full)) fv.clearOnClick = true; else fieldClearSel(c); c.anchor = hit.row; }
        fieldCursor(leaf, hit.row);
      }
      return;
    }
    // 何もない所のドラッグでフィールドを動かす(次にキーを押すと操作中のカードへ戻る)
    canvas.setPointerCapture(e.pointerId);
    const sx = e.clientX, sy = e.clientY, cx = fv.cam.x, cy = fv.cam.y;
    const move = (ev) => { fv.follow = false; fv.cam.x = cx - (ev.clientX - sx) / fv.cam.z; fv.cam.y = cy - (ev.clientY - sy) / fv.cam.z; fieldKick(fv); };
    const up = () => { canvas.removeEventListener('pointermove', move); canvas.removeEventListener('pointerup', up); canvas.removeEventListener('pointercancel', up); };
    canvas.addEventListener('pointermove', move);
    canvas.addEventListener('pointerup', up);
    canvas.addEventListener('pointercancel', up);
  });
  // 項目のドラッグ: ファイルを OS のドラッグとして渡す(Claude・エディタ・ブラウザ・取り込んだアプリなどに落とせる)
  // 選んである項目の上から始めたら、選んだ項目を全部渡す
  canvas.addEventListener('dragstart', (e) => {
    e.preventDefault();
    const en = fv.dragFrom, c = fv.cur;
    fv.dragFrom = null;
    fv.clearOnClick = false;
    canvas.draggable = false;
    if (!en) return;
    const list = c?.sel?.has(en.full) ? c.list.filter((x) => c.sel.has(x.full)) : [en];
    api.fs.drag(list.map((x) => x.full), fieldDragImage(list));
  });
  canvas.addEventListener('click', () => { if (fv.clearOnClick) { fv.clearOnClick = false; fieldClearSel(fv.cur); fieldKick(fv); } });
  // ︙ の上ではカーソルの形と背景を変える
  canvas.addEventListener('pointermove', (e) => {
    const hit = fieldHit(fv, e);
    const c = hit?.menu ? hit.c : null;
    canvas.style.cursor = c ? 'pointer' : '';
    if (fv.hoverMenu !== c) { fv.hoverMenu = c; fieldKick(fv); }
  });
  // 右クリック: 項目の上なら項目の操作、見出し・カードの空いた所ならそのフォルダの操作、︙ なら表示の設定、何もない所ならフィールドの操作
  canvas.addEventListener('contextmenu', (e) => {
    e.preventDefault();
    canvas.focus();
    const hit = fieldHit(fv, e);
    if (!hit) { fieldSpaceMenu(leaf, e.clientX, e.clientY); return; }
    if (hit.c !== fv.cur) fieldFocus(leaf, hit.c);
    if (hit.menu) fieldMenu(leaf, hit.c, e.clientX, e.clientY);
    else if (hit.row >= 0) {
      if (!hit.c.sel?.has(hit.c.list[hit.row].full)) fieldClearSel(hit.c); // 選んでいない項目の上なら、その項目だけが対象
      fieldCursor(leaf, hit.row);
      fieldItemMenu(leaf, e.clientX, e.clientY);
    }
    else fieldCardMenu(leaf, hit.c, e.clientX, e.clientY);
  });
  // ダブルクリック: ファイルは既定のアプリで開き、フォルダは右にカードを開く
  canvas.addEventListener('dblclick', (e) => { const hit = fieldHit(fv, e); if (hit && hit.row >= 0) fieldEnter(leaf); });
  // 中クリック: フォルダを新しいタブ(フィールド)で開く
  canvas.addEventListener('auxclick', (e) => {
    if (e.button !== 1) return;
    const hit = fieldHit(fv, e), en = hit && hit.row >= 0 ? hit.c.list[hit.row] : null;
    if (en?.isDir) { e.preventDefault(); openTab('field', { path: en.full, background: true }); toast(`「${en.name}」を新しいタブで開きました`); }
  });
  canvas.addEventListener('wheel', (e) => {
    e.preventDefault();
    if (e.ctrlKey) { fv.cam.tz = clamp(fv.cam.tz * (e.deltaY < 0 ? 1.1 : 1 / 1.1), 0.3, 2); fieldKick(fv); return; }
    const hit = fieldHit(fv, e);
    if (hit && !e.shiftKey && hit.c.contentH > hit.c.viewH) { // カードの上ではカードの中を送る
      const c = hit.c;
      c.sy = clamp(c.sy + Math.sign(e.deltaY) * 3 * FIELD.ROWH, 0, c.contentH - c.viewH);
      fieldKick(fv);
      return;
    }
    fv.follow = false;
    fv.cam.x += (e.shiftKey ? e.deltaY : e.deltaX) / fv.cam.z;
    fv.cam.y += (e.shiftKey ? 0 : e.deltaY) / fv.cam.z;
    fieldKick(fv);
  }, { passive: false });
  new ResizeObserver(() => fieldKick(fv)).observe(canvas);

  if (fresh) {
    const start = leaf.path ? Promise.resolve(leaf.path) : api.fs.home();
    start.then((p) => fieldNewTree(leaf, p));
  } else {
    fieldCursor(leaf, fv.cur.i);
  }
  if (tab.activePane === leaf.id) requestAnimationFrame(() => canvas.focus());
}
