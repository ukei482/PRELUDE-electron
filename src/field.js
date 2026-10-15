'use strict';
// フィールド表示のファイラ。開いたフォルダを1枚の「カード」(中身の一覧)にして、親子のカードを線でつなぎ、左→右の木としてフィールドに並べる。
// カードの中は ↑↓ で選び、→ で子フォルダのカードを右に開き、← で親のカードへ戻る(Shift+← で今のカードを閉じる)。
// Ctrl+↑↓ で、今のカードを兄弟フォルダ(親のカードの次・前のフォルダ)の中身に切り替える。
// 操作中のカードへカメラがなめらかに移動して、画面の中央に置く。開いたカードは残る。
// 読み込みは開いたフォルダ1つずつ(fileops.js の fs:list = readdir だけ。サイズ・日付は読まない)。読んだ結果はパスごとに覚えておき、
// カーソルがフォルダに止まったら中身を先読みしておく。
// app.js より先に読み込まれる。h / ico / btn / api / C / baseName / clamp / commonButtons などは app.js、decorate(種類の判定)は files.js のものを呼び出し時に使う。

const FIELD = { CARDW: 320, GAP: 90, VGAP: 18, ROWH: 28, HEAD: 34, ROWS: 15, TAU: 90, PREFETCH: 120, CACHE: 400 };
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

// ------------------------------------------------------------------ カード
// row: 親のカードの何行目のフォルダか。kids: 開いている子のカード
const fieldCard = (full, parent, row) => ({ full, name: baseName(full) || '/', parent, row, kids: [], entries: null, err: '', loading: false, i: 0, top: 0, x: 0, y: 0, h: 0, dx: null, dy: null, alpha: 1 });
const fieldParentPath = (p) => (p === '/' ? null : p.replace(/\/+[^/]+\/*$/, '') || '/');
const fieldRows = (c) => Math.max(1, Math.min(c.entries?.length || 0, FIELD.ROWS));
const fieldCardH = (c) => FIELD.HEAD + fieldRows(c) * FIELD.ROWH + 8;
// row の行の中心(カードの上端から)。スクロールで見えていない行は、見えている端の行にそろえる
const fieldRowY = (c, row) => FIELD.HEAD + (clamp(row - c.top, 0, fieldRows(c) - 1) + 0.5) * FIELD.ROWH;

function fieldScrollTo(c) {
  const rows = fieldRows(c);
  if (c.i < c.top) c.top = c.i;
  if (c.i >= c.top + rows) c.top = c.i - rows + 1;
  c.top = clamp(c.top, 0, Math.max(0, (c.entries?.length || 0) - rows));
}

async function fieldLoad(fv, c, force) {
  if (force) fieldForget(c.full);
  const full = c.full;
  c.loading = true;
  fieldKick(fv);
  const r = await fieldChildren(full);
  if (c.full !== full) return false; // 読んでいる間に Ctrl+↑↓ で別のフォルダに替わった
  c.loading = false;
  c.entries = r.ok ? r.entries : [];
  c.err = r.ok ? '' : r.error;
  c.i = clamp(c.i, 0, Math.max(0, c.entries.length - 1));
  fieldScrollTo(c);
  return true;
}

// 左→右の木。子のカードは、親のカードのそのフォルダの行に見出しをそろえて置き、重なる時は下へずらす
function fieldLayout(fv) {
  const cards = [];
  const place = (c, depth, y) => {
    c.x = depth * (FIELD.CARDW + FIELD.GAP);
    c.y = y;
    c.h = fieldCardH(c);
    cards.push(c);
    let bottom = y + c.h, next = -Infinity;
    for (const k of [...c.kids].sort((a, b) => a.row - b.row)) {
      const b = place(k, depth + 1, Math.max(y + fieldRowY(c, k.row) - FIELD.HEAD / 2, next));
      next = b + FIELD.VGAP;
      bottom = Math.max(bottom, b);
    }
    return bottom;
  };
  place(fv.root, 0, 0);
  fv.cards = cards;
  // 新しいカードは、親のカードのその行から伸びてくる
  for (const c of cards) {
    if (c.dx != null) continue;
    const p = c.parent;
    if (p && p.dx != null) { c.dx = p.dx + FIELD.CARDW - 60; c.dy = p.dy + fieldRowY(p, c.row) - FIELD.HEAD / 2; c.alpha = 0; } else { c.dx = c.x; c.dy = c.y; }
  }
  fieldKick(fv);
}

// ------------------------------------------------------------------ 操作
function fieldFocus(leaf, c) {
  const fv = leaf.fv;
  fv.cur = c;
  fv.follow = true;
  if (leaf.path !== c.full) { leaf.path = c.full; renderSidebar(); scheduleSave(); }
  fieldCursor(leaf, c.i);
}

function fieldCursor(leaf, i) {
  const fv = leaf.fv, c = fv.cur;
  c.i = clamp(i, 0, Math.max(0, (c.entries?.length || 0) - 1));
  const top = c.top;
  fieldScrollTo(c);
  if (c.top !== top && c.kids.length) fieldLayout(fv); // 子のカードへの線の出どころが動く
  const en = c.entries?.[c.i];
  if (fv.ui?.path) fv.ui.path.textContent = en ? en.full : c.full;
  clearTimeout(fv.prefetch);
  if (en?.isDir) fv.prefetch = setTimeout(() => fieldChildren(en.full), FIELD.PREFETCH);
  fieldKick(fv);
}

// カーソルの行を開く。フォルダなら右に子のカードを開いて移り、ファイルなら既定のアプリで開く
async function fieldEnter(leaf) {
  const fv = leaf.fv, c = fv.cur, en = c.entries?.[c.i];
  if (!en) return;
  if (!en.isDir) { api.fs.open(en.full); return; }
  let k = c.kids.find((x) => x.full === en.full);
  if (!k) {
    k = fieldCard(en.full, c, c.i);
    c.kids.push(k);
    fieldLayout(fv);
    await fieldLoad(fv, k);
    fieldLayout(fv);
  }
  if (fv.cur === c && c.i === k.row) fieldFocus(leaf, k);
}

// 親のカードへ。一番上のカードなら、ひとつ上のフォルダのカードを左に足す。close: 今のカードを閉じる
async function fieldBack(leaf, close) {
  const fv = leaf.fv, c = fv.cur;
  if (!c.parent && !(await fieldReroot(fv))) return;
  const p = c.parent;
  p.i = c.row;
  if (close) { p.kids = p.kids.filter((k) => k !== c); fieldLayout(fv); }
  fieldFocus(leaf, p);
}

// 今のカードを、兄弟フォルダ(親のカードの次・前のフォルダ)の中身に切り替える
async function fieldSibling(leaf, d) {
  const fv = leaf.fv, c = fv.cur, p = c.parent;
  if (!p?.entries) return;
  let j = c.row + d;
  while (j >= 0 && j < p.entries.length && !p.entries[j].isDir) j += d;
  const en = p.entries[j];
  if (!en) return;
  p.i = j;
  fieldScrollTo(p);
  const open = p.kids.find((k) => k.full === en.full);
  if (open) { // その兄弟がもう開いていれば、そのカードへ移る(今のカードは閉じる)
    p.kids = p.kids.filter((k) => k !== c);
    fieldLayout(fv);
    fieldFocus(leaf, open);
    return;
  }
  Object.assign(c, { full: en.full, name: en.name, row: j, kids: [], entries: null, err: '', i: 0, top: 0 });
  fieldLayout(fv);
  fieldFocus(leaf, c);
  if (await fieldLoad(fv, c)) { fieldLayout(fv); if (fv.cur === c) fieldCursor(leaf, c.i); }
}

// ひとつ上のフォルダのカードを根にして、今までの木をその子としてつなぐ
async function fieldReroot(fv) {
  const old = fv.root;
  const up = fieldParentPath(old.full);
  if (!up) return false;
  const root = fieldCard(up, null, 0);
  await fieldLoad(fv, root);
  const row = root.entries.findIndex((en) => en.full === old.full);
  if (row < 0) return false; // 隠しフォルダから始めて、隠しファイルを出さない設定の時など
  Object.assign(old, { parent: root, row });
  Object.assign(root, { kids: [old], i: row });
  fieldScrollTo(root);
  const ox = old.x, oy = old.y;
  fv.root = root;
  fieldLayout(fv);
  // 座標の基準が変わるので、今あるカードとカメラを同じだけずらして、画面が跳ねないようにする
  const sx = old.x - ox, sy = old.y - oy;
  for (const k of fv.cards) if (k !== root) { k.dx += sx; k.dy += sy; }
  Object.assign(fv.cam, { x: fv.cam.x + sx, y: fv.cam.y + sy });
  root.dx = old.dx - FIELD.CARDW - FIELD.GAP;
  root.dy = old.dy - (old.y - root.y);
  return true;
}

// 今のカードを読み直す(F5)。カーソルの項目と、まだあるフォルダの子のカードは残す
async function fieldReload(leaf) {
  const fv = leaf.fv, c = fv?.cur;
  if (!c) return;
  const at = c.entries?.[c.i]?.full;
  if (!(await fieldLoad(fv, c, true))) return;
  const j = c.entries.findIndex((en) => en.full === at);
  if (j >= 0) c.i = j;
  c.kids = c.kids.filter((k) => (k.row = c.entries.findIndex((en) => en.isDir && en.full === k.full)) >= 0);
  fieldLayout(fv);
  if (fv.cur === c) fieldCursor(leaf, c.i);
}

async function fieldNewTree(leaf, path) {
  const fv = leaf.fv;
  fv.root = fv.cur = fieldCard(path, null, 0);
  fieldLayout(fv);
  Object.assign(fv.cam, { x: FIELD.CARDW / 2, y: fv.root.h / 2 });
  await fieldLoad(fv, fv.root);
  fieldLayout(fv);
  fieldFocus(leaf, fv.root);
}

async function fieldOnKey(leaf, e) {
  const fv = leaf.fv, c = fv.cur;
  if (!c || e.altKey || e.metaKey) return;
  const k = e.key;
  const zoom = (f) => { fv.cam.tz = clamp(f ? fv.cam.tz * f : 1, 0.3, 2); fieldKick(fv); };
  if (e.ctrlKey) {
    if (k === 'ArrowDown') fieldSibling(leaf, 1);
    else if (k === 'ArrowUp') fieldSibling(leaf, -1);
    else return;
  } else if (k === 'ArrowDown') fieldCursor(leaf, c.i + 1);
  else if (k === 'ArrowUp') fieldCursor(leaf, c.i - 1);
  else if (k === 'PageDown') fieldCursor(leaf, c.i + FIELD.ROWS);
  else if (k === 'PageUp') fieldCursor(leaf, c.i - FIELD.ROWS);
  else if (k === 'Home') fieldCursor(leaf, 0);
  else if (k === 'End') fieldCursor(leaf, Infinity);
  else if (k === 'ArrowRight') { if (c.entries?.[c.i]?.isDir) fieldEnter(leaf); }
  else if (k === 'Enter') fieldEnter(leaf);
  else if (k === 'ArrowLeft') fieldBack(leaf, e.shiftKey);
  else if (k === 'F5') fieldReload(leaf); // ふつうは main.js のショートカットとして届く(app.js の onShortcut)
  else if (k === '+' || k === '=') zoom(1.2);
  else if (k === '-') zoom(1 / 1.2);
  else if (k === '0') zoom(0);
  else if (k.length === 1 && k !== ' ' && c.entries?.length) {
    // 文字を打つと、その文字で始まる次の項目へ飛ぶ
    const q = k.toLowerCase(), list = c.entries;
    for (let d = 1; d <= list.length; d++) { const j = (c.i + d) % list.length; if (list[j].name.toLowerCase().startsWith(q)) { fieldCursor(leaf, j); break; } }
  } else return;
  e.preventDefault();
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
  if (fv.follow && fv.cur) { cam.tx = fv.cur.x + FIELD.CARDW / 2; cam.ty = fv.cur.y + fv.cur.h / 2; }
  if (fv.follow) { ease(cam, 'x', cam.tx); ease(cam, 'y', cam.ty); }
  ease(cam, 'z', cam.tz, 0.001);
  for (const c of fv.cards) { ease(c, 'dx', c.x); ease(c, 'dy', c.y); ease(c, 'alpha', 1, 0.01); }
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
    if (!p) continue;
    const ax = p.dx + FIELD.CARDW, ay = p.dy + fieldRowY(p, c.row), bx = c.dx, by = c.dy + FIELD.HEAD / 2;
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
    if (c.dx > view.x1 || c.dx + FIELD.CARDW < view.x0 || c.dy > view.y1 || c.dy + c.h < view.y0) continue;
    fieldDrawCard(fv, ctx, a, c, font, path.has(c));
  }
  ctx.globalAlpha = 1;
}

function fieldDrawCard(fv, ctx, a, c, font, onPath) {
  const x = c.dx, y = c.dy, w = FIELD.CARDW, h = c.h, R = FIELD.ROWH, HD = FIELD.HEAD;
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

  // 見出し: フォルダ名と件数
  const n = c.entries?.length || 0;
  const count = c.loading ? '…' : c.err ? '開けません' : String(n);
  const cw = ctx.measureText(count).width;
  ctx.fillStyle = a.accent;
  ctx.beginPath();
  ctx.arc(x + 16, y + HD / 2, 5, 0, Math.PI * 2);
  ctx.fill();
  ctx.fillStyle = a.text;
  ctx.fillText(fieldFit(ctx, c.name, w - 50 - cw), x + 28, y + HD / 2);
  ctx.fillStyle = c.err ? a.danger : a.dim;
  ctx.fillText(count, x + w - 14 - cw, y + HD / 2);
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
  const rows = fieldRows(c);
  ctx.save();
  ctx.beginPath();
  ctx.rect(x, y + HD, w, h - HD);
  ctx.clip();
  for (let j = 0; j < rows; j++) {
    const idx = c.top + j, en = c.entries[idx];
    if (!en) break;
    const ry = y + HD + j * R + R / 2;
    if (idx === c.i && (focus || onPath || open.has(idx))) {
      ctx.fillStyle = a.panel2;
      ctx.beginPath();
      ctx.roundRect(x + 5, ry - R / 2 + 2, w - 10, R - 3, 6);
      ctx.fill();
      if (focus) { ctx.strokeStyle = a.accent; ctx.lineWidth = 1.5; ctx.stroke(); }
    }
    const col = en.isDir ? a.accent : FIELD_COLORS[en.kind] || a.dim;
    ctx.beginPath();
    ctx.arc(x + 18, ry, en.isDir ? 4.5 : 3.5, 0, Math.PI * 2);
    if (open.has(idx)) { ctx.strokeStyle = col; ctx.lineWidth = 2; ctx.stroke(); } else { ctx.fillStyle = col; ctx.fill(); }
    if (en._lf !== font) { en._lf = font; en._lab = fieldFit(ctx, en.name, w - 60); }
    ctx.fillStyle = en.isDir || idx === c.i ? a.text : a.dim;
    ctx.fillText(en._lab, x + 30, ry);
    if (en.isDir) { ctx.fillStyle = a.dim; ctx.fillText('›', x + w - 18, ry); }
  }
  ctx.restore();
  // 続きがある時は右端にスクロールの目安
  if (n > rows) {
    const th = Math.max(16, (h - HD - 8) * (rows / n));
    const ty = y + HD + 2 + (h - HD - 8 - th) * (c.top / (n - rows));
    ctx.fillStyle = a.line;
    ctx.beginPath();
    ctx.roundRect(x + w - 6, ty, 3, th, 1.5);
    ctx.fill();
  }
}

// 画面の点 → { カード, 行 }(見出しの上は行 -1)。カードの外なら null
function fieldHit(fv, ev) {
  const r = fv.canvas.getBoundingClientRect();
  const z = fv.cam.z;
  const wx = (ev.clientX - r.left - r.width / 2) / z + fv.cam.x;
  const wy = (ev.clientY - r.top - r.height / 2) / z + fv.cam.y;
  for (let i = fv.cards.length - 1; i >= 0; i--) {
    const c = fv.cards[i];
    if (wx < c.dx || wx > c.dx + FIELD.CARDW || wy < c.dy || wy > c.dy + c.h) continue;
    const j = Math.floor((wy - c.dy - FIELD.HEAD) / FIELD.ROWH);
    return { c, row: j >= 0 && j < fieldRows(c) && c.top + j < (c.entries?.length || 0) ? c.top + j : -1 };
  }
  return null;
}

// ------------------------------------------------------------------ ペイン
function buildField(tab, leaf, head, body) {
  const home = btn('home', 'ホーム', async () => { fieldNewTree(leaf, await api.fs.home()); canvas.focus(); });
  const path = h('span', { class: 'ptitle field-path' });
  head.append(ico('field'), path, home, ...commonButtons(tab, leaf));

  const canvas = h('canvas', { class: 'field', tabindex: '0' });
  const hint = h('div', { class: 'field-hint' }, '↑↓ 選ぶ　→ フォルダを開く　← 親へ（Shift で閉じる）　Ctrl+↑↓ 兄弟フォルダへ　Enter 開く　F5 読み直し　+/− 拡大');
  body.append(canvas, hint);

  const fresh = !leaf.fv?.cards;
  if (fresh) leaf.fv = { root: null, cur: null, cards: [], cam: { x: 0, y: 0, z: 1, tx: 0, ty: 0, tz: 1 }, follow: true, raf: 0, last: 0 };
  const fv = leaf.fv;
  fv.canvas = canvas;
  fv.ui = { path };
  if (fv.raf) { cancelAnimationFrame(fv.raf); fv.raf = 0; }
  fv.last = 0;

  canvas.addEventListener('keydown', (e) => fieldOnKey(leaf, e));
  canvas.addEventListener('pointerdown', (e) => {
    canvas.focus();
    const hit = fieldHit(fv, e);
    if (hit) {
      if (hit.c !== fv.cur) fieldFocus(leaf, hit.c);
      if (hit.row >= 0) fieldCursor(leaf, hit.row);
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
  canvas.addEventListener('dblclick', (e) => { const hit = fieldHit(fv, e); if (hit && hit.row >= 0) fieldEnter(leaf); });
  canvas.addEventListener('wheel', (e) => {
    e.preventDefault();
    if (e.ctrlKey) { fv.cam.tz = clamp(fv.cam.tz * (e.deltaY < 0 ? 1.1 : 1 / 1.1), 0.3, 2); fieldKick(fv); return; }
    const hit = fieldHit(fv, e);
    if (hit && !e.shiftKey && (hit.c.entries?.length || 0) > FIELD.ROWS) { // カードの上ではカードの中を送る
      const c = hit.c;
      c.top = clamp(c.top + Math.sign(e.deltaY) * 3, 0, c.entries.length - FIELD.ROWS);
      if (c.kids.length) fieldLayout(fv); else fieldKick(fv);
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
