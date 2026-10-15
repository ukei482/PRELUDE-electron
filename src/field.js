'use strict';
// フィールド表示のファイラ。フォルダの親子を左→右の木として広いフィールドに描き、矢印キーでたどる。
// 選んだ項目へカメラがなめらかに移動して、画面の中央に置く。開いた枝は残る(Shift+← で畳む)。
// 読み込みは開いたフォルダ1つずつ(main.js の fs:children = readdir だけ)。読んだ結果はパスごとに覚えておき、
// カーソルがフォルダに止まったら中身を先読みしておく。
// 木に並ぶのはフォルダだけ。ファイルはフォルダごとに1つの「まとめノード」にして、→ でカード(リスト)を開いて選ぶ
// (ファイルが FIELD.LOOSE 件以下ならまとめずに並べる)。
// app.js より先に読み込まれる。h / ico / btn / api / C / baseName / iconForEntry / commonButtons などは呼び出し時に app.js のものを使う。

const FIELD = { COLW: 280, ROWH: 28, LABELW: 190, TAU: 90, PREFETCH: 120, CACHE: 400, FAN: 24, LOOSE: 3, CARDW: 340, CARDROWS: 14, CARDHEAD: 30 };
const FIELD_COLORS = { image: '#d9a441', archive: '#b57edc', code: '#5fb3a1', text: '#8fa7c9' };
const FIELD_KINDS = { image: '画像', archive: '圧縮', code: 'コード', text: 'テキスト', file: 'その他' };

// ------------------------------------------------------------------ 読み込み(キャッシュ・先読み)
const fieldCache = new Map(); // key → entries。古いものから捨てる
const fieldPending = new Map(); // key → 読み込み中の Promise(同じフォルダを二重に読まない)
const fieldKey = (full) => (C.behavior.showHidden ? '1' : '0') + full;

function fieldChildren(full) {
  const key = fieldKey(full);
  if (fieldCache.has(key)) return Promise.resolve({ ok: true, entries: fieldCache.get(key) });
  if (fieldPending.has(key)) return fieldPending.get(key);
  const p = api.fs.children(full, C.behavior.showHidden).then((r) => {
    fieldPending.delete(key);
    if (r.ok) {
      fieldCache.set(key, r.entries);
      if (fieldCache.size > FIELD.CACHE) fieldCache.delete(fieldCache.keys().next().value);
    }
    return r;
  });
  fieldPending.set(key, p);
  return p;
}
const fieldForget = (full) => fieldCache.delete(fieldKey(full));

// ------------------------------------------------------------------ 木
const fieldNode = (en, parent) => ({ name: en.name, full: en.full, isDir: en.isDir, parent, kids: null, open: false, loading: false, err: '', x: 0, y: 0, dx: null, dy: null, label: null, tw: 0 });
// フォルダの中身 → 子ノード。old(パス → ノード)にあるものは引き継ぐ(開いていた枝・まとめノードを残す)
function fieldMakeKids(n, entries, old = new Map()) {
  const reuse = (en) => { const o = old.get(en.full); if (o && !o.bundle && o.isDir === en.isDir) { o.parent = n; return o; } return fieldNode(en, n); };
  const files = entries.filter((en) => !en.isDir);
  const kids = entries.filter((en) => en.isDir).map(reuse);
  if (files.length <= FIELD.LOOSE) return kids.concat(files.map(reuse));
  const key = n.full + '\0files';
  const b = old.get(key) || Object.assign(fieldNode({ name: '', full: key, isDir: false }, n), { bundle: true });
  const count = {};
  for (const en of files) { const k = iconForEntry(en); count[k] = (count[k] || 0) + 1; }
  Object.assign(b, {
    parent: n, files, name: `ファイル ${files.length}`, label: null,
    mix: Object.entries(count).sort((x, y) => y[1] - x[1]).slice(0, 3).map(([k, c]) => `${FIELD_KINDS[k]} ${c}`).join(' · '),
  });
  kids.push(b);
  return kids;
}
const fieldParentPath = (p) => (p === '/' ? null : p.replace(/\/+[^/]+\/*$/, '') || '/');

// 左→右の木。閉じたフォルダとファイルが1行ずつ使い、開いたフォルダは子の中ほどに置く(子が多い時は上寄せ)
function fieldLayout(fv) {
  let row = 0;
  const vis = [];
  const walk = (n, d) => {
    n.x = d * FIELD.COLW;
    vis.push(n);
    if (n.open && n.kids && n.kids.length) {
      for (const k of n.kids) walk(k, d + 1);
      const first = n.kids[0].y;
      n.y = Math.min((first + n.kids[n.kids.length - 1].y) / 2, first + 6 * FIELD.ROWH);
    } else {
      n.y = row++ * FIELD.ROWH;
    }
  };
  walk(fv.root, 0);
  fv.vis = vis;
  if (fv.card && !vis.includes(fv.card.node)) fv.card = null;
  // 新しく見えたノードは親の位置から伸びてくる(子が多いフォルダは横にだけ滑らせる。縦に集めると全部が画面内に入って重い)
  for (const n of vis) {
    if (n.dx != null) continue;
    const p = n.parent;
    n.dx = p?.dx ?? n.x;
    n.dy = p && p.kids.length <= FIELD.FAN ? p.dy : n.y;
  }
  fieldKick(fv);
}

// 畳んでいた枝をもう一度開いた時も、親の位置から伸ばす
function fieldSpawnFrom(n) {
  const walk = (k) => { k.dx = n.dx; k.dy = k.parent.kids.length <= FIELD.FAN ? n.dy : k.y; if (k.open && k.kids) k.kids.forEach(walk); };
  if (n.kids) n.kids.forEach(walk);
}

async function fieldOpen(fv, n) {
  if (!n.isDir) return;
  if (!n.kids) {
    n.loading = true;
    fieldKick(fv);
    const r = await fieldChildren(n.full);
    n.loading = false;
    n.kids = r.ok ? fieldMakeKids(n, r.entries) : [];
    n.err = r.ok ? '' : r.error;
  }
  const wasOpen = n.open;
  n.open = true;
  fieldLayout(fv);
  if (!wasOpen) fieldSpawnFrom(n); // 並べた後の行の位置を使う
}

function fieldClose(fv, n) {
  if (!n.open) return;
  n.open = false;
  if (fv.sel !== n && fieldIsUnder(fv.sel, n)) fv.sel = n;
  fieldLayout(fv);
}
const fieldIsUnder = (k, n) => { for (let p = k?.parent; p; p = p.parent) if (p === n) return true; return false; };

// フォルダを読み直す(F5)。開いていた枝は同じパスのものを引き継ぐ
async function fieldReload(fv, n) {
  if (!n.isDir) return;
  fieldForget(n.full);
  const old = new Map((n.kids || []).map((k) => [k.full, k]));
  const r = await fieldChildren(n.full);
  n.kids = r.ok ? fieldMakeKids(n, r.entries, old) : [];
  n.err = r.ok ? '' : r.error;
  if (fv.sel !== n && fieldIsUnder(fv.sel, n) && !n.kids.some((k) => k === fv.sel || fieldIsUnder(fv.sel, k))) fv.sel = n;
  fieldLayout(fv);
}

// 一番上の親より上へ: 親フォルダを新しい根にして、今までの木をその子としてつなぐ
async function fieldReroot(fv) {
  const old = fv.root;
  const up = fieldParentPath(old.full);
  if (!up) return false;
  const root = fieldNode({ name: baseName(up) || '/', full: up, isDir: true }, null);
  const r = await fieldChildren(up);
  root.kids = r.ok ? fieldMakeKids(root, r.entries, new Map([[old.full, old]])) : [old];
  root.err = r.ok ? '' : r.error;
  if (!root.kids.includes(old)) root.kids.unshift(old);
  old.parent = root;
  root.open = true;
  root.dx = old.dx - FIELD.COLW;
  root.dy = old.dy;
  fv.root = root;
  fieldLayout(fv);
  return true;
}

function fieldNewTree(leaf, path) {
  const fv = leaf.fv;
  fv.root = fieldNode({ name: baseName(path) || '/', full: path, isDir: true }, null);
  fv.sel = fv.root;
  fv.vis = [];
  fieldLayout(fv);
  fv.cam.x = fv.cam.y = 0;
  fv.follow = true;
  fieldOpen(fv, fv.root).then(() => fieldSelect(leaf, fv.root.kids?.[0] || fv.root));
}

// ------------------------------------------------------------------ 選択
function fieldSelect(leaf, n) {
  const fv = leaf.fv;
  if (!n) return;
  if (fv.card && fv.card.node !== n) fv.card = null;
  fv.sel = n;
  fv.follow = true;
  // 開いている場所 = 選んだ項目のあるフォルダ(タブ名・次回起動時の根に使う)
  const where = n.parent ? n.parent.full : n.full;
  if (leaf.path !== where) { leaf.path = where; renderSidebar(); scheduleSave(); }
  fieldShowPath(fv);
  clearTimeout(fv.prefetch);
  if (n.isDir && !n.kids) fv.prefetch = setTimeout(() => fieldChildren(n.full), FIELD.PREFETCH);
  fieldKick(fv);
}

function fieldShowPath(fv) {
  const n = fv.sel;
  if (!fv.ui?.path || !n) return;
  fv.ui.path.textContent = fv.card ? fv.card.node.files[fv.card.i].full : n.bundle ? `${n.parent.full}（${n.name}）` : n.full;
}

// ------------------------------------------------------------------ カード(まとめノードのファイル一覧)
function fieldOpenCard(fv, n) {
  if (!n?.bundle) return;
  fv.card = { node: n, i: 0, top: 0 };
  fv.follow = true;
  fieldShowPath(fv);
  fieldKick(fv);
}
function fieldCardMove(fv, i) {
  const c = fv.card;
  const len = c.node.files.length, rows = Math.min(len, FIELD.CARDROWS);
  c.i = clamp(i, 0, len - 1);
  if (c.i < c.top) c.top = c.i;
  if (c.i >= c.top + rows) c.top = c.i - rows + 1;
  fieldShowPath(fv);
  fieldKick(fv);
}
// カードの位置(フィールド上)。まとめノードの右に、見出しをノードの行にそろえて下へ伸ばす。target: 動き終わった後の位置
function fieldCardRect(fv, target) {
  const n = fv.card.node;
  const rows = Math.min(n.files.length, FIELD.CARDROWS);
  const x = (target ? n.x : n.dx) + 14 + n.tw + 34, y = (target ? n.y : n.dy) - FIELD.CARDHEAD / 2;
  return { x, y, w: FIELD.CARDW, h: FIELD.CARDHEAD + rows * FIELD.ROWH + 6, rows };
}

function fieldActivate(leaf, n) {
  const fv = leaf.fv;
  if (!n) return;
  if (n.bundle) { fieldOpenCard(fv, n); return; }
  if (!n.isDir) { api.fs.open(n.full); return; }
  if (n.open) fieldClose(fv, n); else fieldOpen(fv, n);
}

async function fieldOnKey(leaf, e) {
  const fv = leaf.fv;
  const s = fv.sel;
  if (!s || e.altKey || e.ctrlKey || e.metaKey) return;
  const sibs = s.parent ? s.parent.kids : [s];
  const i = sibs.indexOf(s);
  const go = (j) => fieldSelect(leaf, sibs[clamp(j, 0, sibs.length - 1)]);
  const zoom = (f) => { fv.cam.tz = clamp(f ? fv.cam.tz * f : 1, 0.3, 2); fieldKick(fv); };
  const k = e.key;
  if (fv.card) {
    const c = fv.card, files = c.node.files;
    if (k === 'ArrowDown') fieldCardMove(fv, c.i + 1);
    else if (k === 'ArrowUp') fieldCardMove(fv, c.i - 1);
    else if (k === 'PageDown') fieldCardMove(fv, c.i + FIELD.CARDROWS);
    else if (k === 'PageUp') fieldCardMove(fv, c.i - FIELD.CARDROWS);
    else if (k === 'Home') fieldCardMove(fv, 0);
    else if (k === 'End') fieldCardMove(fv, files.length - 1);
    else if (k === 'Enter') api.fs.open(files[c.i].full);
    else if (k === 'ArrowLeft' || k === 'Escape') { fv.card = null; fieldSelect(leaf, c.node); }
    else if (k === 'F5') { fv.card = null; fieldReload(fv, c.node.parent); }
    else if (k.length === 1 && k !== ' ') {
      const q = k.toLowerCase();
      for (let d = 1; d <= files.length; d++) { const j = (c.i + d) % files.length; if (files[j].name.toLowerCase().startsWith(q)) { fieldCardMove(fv, j); break; } }
    } else if (k !== 'ArrowRight') return;
    e.preventDefault();
    return;
  }
  if (k === 'ArrowDown') go(i + 1);
  else if (k === 'ArrowUp') go(i - 1);
  else if (k === 'PageDown') go(i + 10);
  else if (k === 'PageUp') go(i - 10);
  else if (k === 'Home') go(0);
  else if (k === 'End') go(sibs.length - 1);
  else if (k === 'ArrowRight') {
    if (s.bundle) { fieldOpenCard(fv, s); e.preventDefault(); return; }
    if (!s.isDir) return;
    e.preventDefault();
    await fieldOpen(fv, s);
    if (fv.sel === s && s.kids?.length) fieldSelect(leaf, s.kids[0]);
    else fieldKick(fv);
    return;
  } else if (k === 'ArrowLeft') {
    e.preventDefault();
    if (!s.parent && !(await fieldReroot(fv))) return;
    const p = fv.sel.parent;
    if (e.shiftKey) fieldClose(fv, p);
    fieldSelect(leaf, p);
    return;
  } else if (k === 'Enter') fieldActivate(leaf, s);
  else if (k === 'F5') fieldReload(fv, s.isDir && s.open ? s : s.parent || s);
  else if (k === '+' || k === '=') zoom(1.2);
  else if (k === '-') zoom(1 / 1.2);
  else if (k === '0') zoom(0);
  else if (k.length === 1 && k !== ' ') {
    // 文字を打つと、その文字で始まる次の兄弟へ飛ぶ
    const q = k.toLowerCase();
    for (let d = 1; d <= sibs.length; d++) {
      const c = sibs[(i + d) % sibs.length];
      if (c.name.toLowerCase().startsWith(q)) { fieldSelect(leaf, c); break; }
    }
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
  const ease = (o, a, b) => {
    const d = o[b] - o[a];
    if (Math.abs(d) < 0.3) { o[a] = o[b]; return; }
    o[a] += d * k;
    moving = true;
  };
  const cam = fv.cam;
  if (fv.follow && fv.card) { const r = fieldCardRect(fv, true); cam.tx = r.x + r.w / 2 - 60; cam.ty = r.y + r.h / 2; }
  else if (fv.follow && fv.sel) { cam.tx = fv.sel.x + Math.min(fv.sel.tw, FIELD.LABELW) / 2 + 7; cam.ty = fv.sel.y; }
  if (fv.follow) { ease(cam, 'x', 'tx'); ease(cam, 'y', 'ty'); }
  ease(cam, 'z', 'tz');
  for (const n of fv.vis) {
    const nx = n.x - n.dx, ny = n.y - n.dy;
    if (Math.abs(nx) < 0.3 && Math.abs(ny) < 0.3) { n.dx = n.x; n.dy = n.y; continue; }
    n.dx += nx * k;
    n.dy += ny * k;
    moving = true;
  }
  fieldDraw(fv);
  if (moving) fieldKick(fv); else fv.last = 0;
}

function fieldLabel(ctx, n) {
  if (n.label != null) return;
  let s = n.name;
  if (ctx.measureText(s).width > FIELD.LABELW) {
    let lo = 0, hi = s.length;
    while (lo < hi) { const m = (lo + hi + 1) >> 1; if (ctx.measureText(s.slice(0, m) + '…').width <= FIELD.LABELW) lo = m; else hi = m - 1; }
    s = s.slice(0, lo) + '…';
  }
  n.label = s;
  n.tw = ctx.measureText(s).width;
}

function fieldDraw(fv) {
  const { canvas, cam } = fv;
  const ctx = canvas.getContext('2d');
  const W = canvas.clientWidth, H = canvas.clientHeight;
  const dpr = window.devicePixelRatio || 1;
  if (canvas.width !== Math.round(W * dpr) || canvas.height !== Math.round(H * dpr)) { canvas.width = Math.round(W * dpr); canvas.height = Math.round(H * dpr); }
  const a = C.appearance;
  const font = `${a.fontSize}px ${getComputedStyle(document.body).fontFamily}`;
  if (fv.font !== font) { fv.font = font; const clear = (n) => { n.label = null; n.kids?.forEach(clear); }; clear(fv.root); }
  ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
  ctx.fillStyle = a.bg;
  ctx.fillRect(0, 0, W, H);
  const z = cam.z;
  ctx.setTransform(dpr * z, 0, 0, dpr * z, dpr * (W / 2 - cam.x * z), dpr * (H / 2 - cam.y * z));
  const pad = FIELD.ROWH;
  const x0 = cam.x - W / 2 / z - FIELD.COLW, x1 = cam.x + W / 2 / z + pad;
  const y0 = cam.y - H / 2 / z - pad, y1 = cam.y + H / 2 / z + pad;
  ctx.font = font;
  ctx.textBaseline = 'middle';

  // 枝(親のラベルの右端 → 子の点)。子が多いフォルダは扇にすると線が塗りつぶしになるので、幹から櫛のように出す
  ctx.strokeStyle = a.line;
  ctx.lineWidth = 1.2;
  ctx.beginPath();
  for (const p of fv.vis) {
    if (!p.open || !p.kids?.length) continue;
    fieldLabel(ctx, p);
    const ax = p.dx + 14 + p.tw + 6, ay = p.dy;
    const first = p.kids[0], last = p.kids[p.kids.length - 1];
    if (ax > x1 || first.dx < x0 || Math.max(ay, last.dy) < y0 || Math.min(ay, first.dy) > y1) continue;
    if (p.kids.length <= FIELD.FAN) {
      for (const n of p.kids) {
        const bx = n.dx - 7, by = n.dy, mx = (ax + bx) / 2;
        ctx.moveTo(ax, ay);
        ctx.bezierCurveTo(mx, ay, mx, by, bx, by);
      }
      continue;
    }
    const sx = first.dx - 24, top = Math.max(first.dy, y0), bot = Math.min(last.dy, y1);
    const jy = clamp(ay, first.dy, last.dy), mx = (ax + sx) / 2;
    ctx.moveTo(ax, ay);
    ctx.bezierCurveTo(mx, ay, mx, jy, sx, jy);
    if (top < bot) { ctx.moveTo(sx, top); ctx.lineTo(sx, bot); }
    for (const n of p.kids) {
      if (n.dy < y0 || n.dy > y1) continue;
      ctx.moveTo(sx, n.dy);
      ctx.lineTo(n.dx - 7, n.dy);
    }
  }
  ctx.stroke();

  // 項目
  for (const n of fv.vis) {
    if (n.dy < y0 || n.dy > y1 || n.dx < x0 || n.dx > x1) continue;
    fieldLabel(ctx, n);
    const sel = n === fv.sel;
    if (sel) {
      ctx.fillStyle = a.panel2;
      ctx.strokeStyle = a.accent;
      ctx.lineWidth = 1.5;
      ctx.beginPath();
      ctx.roundRect(n.dx - 12, n.dy - 13, n.tw + 34, 26, 13);
      ctx.fill();
      ctx.stroke();
    }
    const kind = iconForEntry(n);
    const col = n.bundle ? a.dim : n.err ? a.danger : n.isDir ? a.accent : FIELD_COLORS[kind] || a.dim;
    ctx.beginPath();
    if (n.bundle) { ctx.roundRect(n.dx - 5, n.dy - 5, 10, 10, 2); ctx.roundRect(n.dx - 3, n.dy - 7, 10, 10, 2); }
    else ctx.arc(n.dx, n.dy, n.isDir ? 5 : 4, 0, Math.PI * 2);
    if (n.bundle) { ctx.strokeStyle = a.dim; ctx.lineWidth = 1.5; ctx.stroke(); }
    else if (n.isDir && n.open) { ctx.strokeStyle = col; ctx.lineWidth = 2; ctx.stroke(); } else { ctx.fillStyle = col; ctx.fill(); }
    ctx.fillStyle = n.err ? a.danger : n.isDir || sel ? a.text : a.dim;
    ctx.fillText(n.label, n.dx + 14, n.dy);
    // 補足: 読み込み中 / 空 / 開けない / 閉じたフォルダの中身の数(読んであれば)
    let note = '';
    if (n.bundle) note = fv.card?.node === n ? '' : n.mix;
    else if (n.loading) note = '…';
    else if (n.err) note = '開けません';
    else if (n.isDir && n.open && n.kids && !n.kids.length) note = '空';
    else if (n.isDir && !n.open) { const c = n.kids || fieldCache.get(fieldKey(n.full)); if (c) note = String(c.length); }
    if (note) {
      ctx.fillStyle = a.dim;
      ctx.fillText(note, n.dx + 14 + n.tw + (sel ? 16 : n.isDir && n.open ? 14 : 10), n.dy);
    }
  }
  if (fv.card) fieldDrawCard(fv, ctx, a);
}

// 幅に収まるよう末尾を … で切る
function fieldFit(ctx, s, w) {
  if (ctx.measureText(s).width <= w) return s;
  let lo = 0, hi = s.length;
  while (lo < hi) { const m = (lo + hi + 1) >> 1; if (ctx.measureText(s.slice(0, m) + '…').width <= w) lo = m; else hi = m - 1; }
  return s.slice(0, lo) + '…';
}

function fieldDrawCard(fv, ctx, a) {
  const c = fv.card, n = c.node, r = fieldCardRect(fv, false);
  const R = FIELD.ROWH, H = FIELD.CARDHEAD;
  // まとめノード → カードの見出しへ短い線
  ctx.strokeStyle = a.accent;
  ctx.lineWidth = 1.5;
  ctx.beginPath();
  ctx.moveTo(n.dx + 14 + n.tw + 18, n.dy);
  ctx.lineTo(r.x, n.dy);
  ctx.stroke();
  ctx.save();
  ctx.shadowColor = 'rgba(0,0,0,.18)';
  ctx.shadowBlur = 18;
  ctx.shadowOffsetY = 4;
  ctx.fillStyle = a.panel;
  ctx.beginPath();
  ctx.roundRect(r.x, r.y, r.w, r.h, 10);
  ctx.fill();
  ctx.restore();
  ctx.strokeStyle = a.line;
  ctx.lineWidth = 1;
  ctx.stroke();
  ctx.fillStyle = a.dim;
  ctx.fillText(fieldFit(ctx, `${n.name}　${n.mix}`, r.w - 28), r.x + 14, r.y + H / 2);
  ctx.save();
  ctx.beginPath();
  ctx.rect(r.x, r.y + H, r.w, r.h - H);
  ctx.clip();
  const font = ctx.font;
  for (let j = 0; j < r.rows; j++) {
    const en = n.files[c.top + j];
    if (!en) break;
    const y = r.y + H + j * R + R / 2;
    if (c.top + j === c.i) {
      ctx.fillStyle = a.panel2;
      ctx.beginPath();
      ctx.roundRect(r.x + 6, y - R / 2 + 1, r.w - 12, R - 2, 6);
      ctx.fill();
      ctx.strokeStyle = a.accent;
      ctx.lineWidth = 1.5;
      ctx.stroke();
    }
    ctx.fillStyle = FIELD_COLORS[iconForEntry(en)] || a.dim;
    ctx.beginPath();
    ctx.arc(r.x + 20, y, 4, 0, Math.PI * 2);
    ctx.fill();
    if (en._lf !== font) { en._lf = font; en._lab = fieldFit(ctx, en.name, r.w - 52); }
    ctx.fillStyle = a.text;
    ctx.fillText(en._lab, r.x + 32, y);
  }
  ctx.restore();
  // 続きがある時は右端にスクロールの目安
  const len = n.files.length;
  if (len > r.rows) {
    const th = Math.max(16, (r.h - H - 8) * (r.rows / len));
    const ty = r.y + H + 2 + (r.h - H - 8 - th) * (c.top / (len - r.rows));
    ctx.fillStyle = a.line;
    ctx.beginPath();
    ctx.roundRect(r.x + r.w - 6, ty, 3, th, 1.5);
    ctx.fill();
  }
}

// 画面の点 → フィールド上の点
function fieldWorld(fv, ev) {
  const r = fv.canvas.getBoundingClientRect();
  const z = fv.cam.z;
  return [(ev.clientX - r.left - r.width / 2) / z + fv.cam.x, (ev.clientY - r.top - r.height / 2) / z + fv.cam.y];
}
// カードの上なら行の番号(見出しの上は -1)、カードの外なら null
function fieldHitCard(fv, ev) {
  if (!fv.card) return null;
  const [wx, wy] = fieldWorld(fv, ev);
  const r = fieldCardRect(fv, false);
  if (wx < r.x || wx > r.x + r.w || wy < r.y || wy > r.y + r.h) return null;
  const j = Math.floor((wy - r.y - FIELD.CARDHEAD) / FIELD.ROWH);
  return j >= 0 && j < r.rows && fv.card.top + j < fv.card.node.files.length ? fv.card.top + j : -1;
}
// 画面の点 → フィールド上の項目
function fieldHit(fv, ev) {
  const [wx, wy] = fieldWorld(fv, ev);
  for (const n of fv.vis) if (Math.abs(wy - n.dy) < FIELD.ROWH / 2 && wx > n.dx - 12 && wx < n.dx + 22 + n.tw) return n;
  return null;
}

// ------------------------------------------------------------------ ペイン
function buildField(tab, leaf, head, body) {
  const home = btn('home', 'ホーム', async () => { fieldNewTree(leaf, await api.fs.home()); canvas.focus(); });
  const path = h('span', { class: 'ptitle field-path' });
  head.append(ico('field'), path, home, ...commonButtons(tab, leaf));

  const canvas = h('canvas', { class: 'field', tabindex: '0' });
  const hint = h('div', { class: 'field-hint' }, '→ 開く（ファイルのまとめはカードを開く）　← 親へ（Shift で畳む）　↑↓ 移動　Enter 開く/畳む　Esc カードを閉じる　F5 読み直し　+/− 拡大');
  body.append(canvas, hint);

  const fresh = !leaf.fv;
  if (fresh) leaf.fv = { root: null, sel: null, vis: [], cam: { x: 0, y: 0, z: 1, tx: 0, ty: 0, tz: 1 }, follow: true, raf: 0, last: 0 };
  const fv = leaf.fv;
  fv.canvas = canvas;
  fv.ui = { path };
  if (fv.raf) { cancelAnimationFrame(fv.raf); fv.raf = 0; }
  fv.last = 0;

  canvas.addEventListener('keydown', (e) => fieldOnKey(leaf, e));
  canvas.addEventListener('pointerdown', (e) => {
    canvas.focus();
    const ci = fieldHitCard(fv, e);
    if (ci != null) { if (ci >= 0) fieldCardMove(fv, ci); return; }
    const n = fieldHit(fv, e);
    if (n) { fieldSelect(leaf, n); return; }
    // 何もない所のドラッグでフィールドを動かす(次にキーを押すと選んだ項目へ戻る)
    canvas.setPointerCapture(e.pointerId);
    const sx = e.clientX, sy = e.clientY, cx = fv.cam.x, cy = fv.cam.y;
    const move = (ev) => { fv.follow = false; fv.cam.x = cx - (ev.clientX - sx) / fv.cam.z; fv.cam.y = cy - (ev.clientY - sy) / fv.cam.z; fieldKick(fv); };
    const up = () => { canvas.removeEventListener('pointermove', move); canvas.removeEventListener('pointerup', up); canvas.removeEventListener('pointercancel', up); };
    canvas.addEventListener('pointermove', move);
    canvas.addEventListener('pointerup', up);
    canvas.addEventListener('pointercancel', up);
  });
  canvas.addEventListener('dblclick', (e) => {
    const ci = fieldHitCard(fv, e);
    if (ci != null) { if (ci >= 0) api.fs.open(fv.card.node.files[ci].full); return; }
    fieldActivate(leaf, fieldHit(fv, e));
  });
  canvas.addEventListener('wheel', (e) => {
    e.preventDefault();
    if (e.ctrlKey) { fv.cam.tz = clamp(fv.cam.tz * (e.deltaY < 0 ? 1.1 : 1 / 1.1), 0.3, 2); fieldKick(fv); return; }
    if (fieldHitCard(fv, e) != null) { // カードの上ではカードの中を送る
      const c = fv.card, max = Math.max(0, c.node.files.length - FIELD.CARDROWS);
      c.top = clamp(c.top + Math.sign(e.deltaY) * 3, 0, max);
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
    path.textContent = fv.sel?.full || '';
    fieldKick(fv);
  }
  if (tab.activePane === leaf.id) requestAnimationFrame(() => canvas.focus());
}
