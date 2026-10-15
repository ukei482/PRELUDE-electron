'use strict';
// フォルダペイン(ファイラ)。キーボードだけで操作できることを第一にする(F1 で操作の一覧)。
// 一覧は仮想スクロール: 見えている行だけを、使い回しの DOM で描く。ファイル操作は main 側の fileops.js(api.fs)。
// app.js の共通関数(h, ico, popup, toast, splitLeaf …)を使う。読み込みは app.js より前だが、呼ばれるのは起動後。

const ROW = 28;
const KINDS = {
  image: 'png jpg jpeg gif webp svg bmp ico avif heic tif tiff',
  video: 'mp4 mkv webm mov avi m4v wmv flv',
  audio: 'mp3 flac wav ogg opus m4a aac wma',
  archive: 'zip 7z rar tar gz tgz xz bz2 zst deb rpm iso appimage',
  code: 'js mjs cjs ts jsx tsx rs py c cc cpp h hpp java kt go rb php lua html css scss json toml yaml yml xml sh bash zsh bat ps1 sql vue svelte',
  text: 'txt md log csv tsv ini conf cfg',
  doc: 'pdf doc docx xls xlsx ppt pptx odt ods odp epub',
};
const KIND_OF = new Map();
for (const [k, list] of Object.entries(KINDS)) for (const e of list.split(' ')) KIND_OF.set(e, k);
// 種類 → アイコン(無いものは file アイコンを色違いで出す)
const KIND_ICON = { dir: 'folder', image: 'image', archive: 'archive', code: 'code', text: 'text' };

const collator = new Intl.Collator('ja', { numeric: true, sensitivity: 'base' });
const DF = new Intl.DateTimeFormat('ja-JP', { year: 'numeric', month: '2-digit', day: '2-digit', hour: '2-digit', minute: '2-digit' });
const TF = new Intl.DateTimeFormat('ja-JP', { hour: '2-digit', minute: '2-digit' });
let dayStart = 0;
function fmtDate(ms) {
  if (!ms) return '';
  if (ms >= dayStart) return '今日 ' + TF.format(ms);
  if (ms >= dayStart - 86400000) return '昨日 ' + TF.format(ms);
  return DF.format(ms);
}
const refreshDay = () => { const d = new Date(); d.setHours(0, 0, 0, 0); dayStart = d.getTime(); };

const sepOf = (p) => (/^[a-z]:\\/i.test(p) || (p.includes('\\') && !p.includes('/')) ? '\\' : '/');
const joinPath = (dir, name) => { const s = sepOf(dir); return dir.endsWith(s) ? dir + name : dir + s + name; };

// 列の幅(全フォルダペイン共通。名前の列は残りの幅)
const COLS = { type: 72, size: 84, date: 132 };
try { Object.assign(COLS, JSON.parse(localStorage.getItem('fileCols') || '{}')); } catch {}
const applyCols = () => { for (const [k, v] of Object.entries(COLS)) document.documentElement.style.setProperty('--cw-' + k, v + 'px'); };
applyCols();

function newFilesState(o) {
  return {
    entries: [], view: [], parent: null, err: '', sel: new Set(), cur: -1, anchor: 0, scrollTop: 0,
    sort: o.sort && ['name', 'type', 'size', 'date'].includes(o.sort.key) ? { key: o.sort.key, desc: !!o.sort.desc } : { key: 'name', desc: false },
    hist: { back: [], fwd: [] }, loadSeq: 0, busy: '', renaming: null,
  };
}

function decorate(dir, e) {
  const dot = e.name.lastIndexOf('.');
  const ext = !e.isDir && dot > 0 ? e.name.slice(dot + 1).toLowerCase() : '';
  return {
    name: e.name, full: joinPath(dir, e.name), isDir: e.isDir, link: e.link, ext,
    kind: e.isDir ? 'dir' : KIND_OF.get(ext) || 'file', size: undefined, mtime: 0, dateStr: '',
  };
}

function sortView(leaf) {
  const { key, desc } = leaf.sort;
  const d = desc ? -1 : 1;
  const byName = (a, b) => collator.compare(a.name, b.name);
  const cmp = key === 'size' ? (a, b) => ((a.size || 0) - (b.size || 0)) * d || byName(a, b)
    : key === 'date' ? (a, b) => (a.mtime - b.mtime) * d || byName(a, b)
    : key === 'type' ? (a, b) => collator.compare(a.ext, b.ext) * d || byName(a, b)
    : (a, b) => byName(a, b) * d;
  // フォルダは常に先。フォルダどうしはサイズが無いので名前順
  leaf.view = leaf.entries.slice().sort((a, b) => (b.isDir - a.isDir) || (a.isDir && key === 'size' ? byName(a, b) : cmp(a, b)));
}

// 並べ直しても、カーソルと選択は同じファイルに残す
function resort(leaf) {
  const curName = leaf.view[leaf.cur]?.name;
  sortView(leaf);
  if (curName != null) leaf.cur = leaf.view.findIndex((e) => e.name === curName);
  schedulePaint(leaf);
}

// ------------------------------------------------------------------ 読み込み
// opt: keep(同じフォルダの読み直し。選択とスクロールを保つ) / focusName(カーソルを置く名前) / selectNames / noHist
async function loadDir(leaf, p, opt = {}) {
  const seq = ++leaf.loadSeq;
  const target = p ?? leaf.path;
  const r = await api.fs.list(target, C.behavior.showHidden);
  if (seq !== leaf.loadSeq) return;
  const same = r.ok && r.path === leaf.path && leaf.entries.length > 0;
  if (!r.ok) {
    if (leaf.entries.length && r.path !== leaf.path) { toast('開けません: ' + r.error); if (leaf.ui?.addr) leaf.ui.addr.value = leaf.path; return; }
    Object.assign(leaf, { err: r.error, entries: [], view: [], cur: -1, sel: new Set() });
    updateFilesUI(leaf);
    schedulePaint(leaf);
    return;
  }
  refreshDay();
  const prevCur = leaf.cur;
  const prevName = leaf.view[leaf.cur]?.name;
  const old = same ? new Map(leaf.entries.map((e) => [e.name, e])) : null;
  if (!same && !opt.noHist && leaf.path && leaf.entries.length) {
    leaf.hist.back.push({ path: leaf.path, name: prevName });
    if (leaf.hist.back.length > 100) leaf.hist.back.shift();
    leaf.hist.fwd = [];
  }
  const changedDir = r.path !== leaf.path;
  Object.assign(leaf, { path: r.path, parent: r.parent, err: '' });
  leaf.entries = r.entries.map((e) => {
    const en = decorate(r.path, e);
    const o = old?.get(e.name);
    if (o) Object.assign(en, { size: o.size, mtime: o.mtime, dateStr: o.dateStr }); // 読み直しでサイズ・日付がちらつかないように
    return en;
  });
  sortView(leaf);
  const names = new Set(leaf.view.map((e) => e.name));
  if (opt.selectNames?.length) leaf.sel = new Set(opt.selectNames.filter((n) => names.has(n)));
  else leaf.sel = same ? new Set([...leaf.sel].filter((n) => names.has(n))) : new Set();
  const want = opt.focusName ?? opt.selectNames?.[0] ?? (same ? prevName : null);
  let cur = want != null ? leaf.view.findIndex((e) => e.name === want) : -1;
  if (cur < 0) cur = same ? Math.min(prevCur, leaf.view.length - 1) : leaf.view.length ? 0 : -1;
  leaf.cur = cur;
  leaf.anchor = Math.max(0, cur);
  if (opt.focusName != null && !opt.selectNames && cur >= 0) leaf.sel = new Set([leaf.view[cur].name]);
  if (!same) { leaf.scrollTop = 0; if (leaf.vl) leaf.vl.list.scrollTop = 0; }
  api.fs.watch(leaf.id, r.path);
  updateFilesUI(leaf);
  paintRows(leaf);
  if (cur >= 0) ensureVisible(leaf, cur);
  if (changedDir) { renderSidebar(); scheduleSave(); }
  fetchStats(leaf, seq);
}

// サイズ・日付を少しずつ読んで埋める(先に見えている所から)
async function fetchStats(leaf, seq) {
  const all = leaf.view.slice();
  const v = leaf.vl;
  const first = v ? Math.floor(v.list.scrollTop / ROW) : 0;
  const order = [...all.slice(first), ...all.slice(0, first)];
  const dir = leaf.path;
  for (let i = 0; i < order.length; i += 400) {
    const chunk = order.slice(i, i + 400);
    const st = await api.fs.stats(dir, chunk.map((e) => e.name));
    if (seq !== leaf.loadSeq) return;
    chunk.forEach((e, k) => {
      const s = st[k];
      if (!s) return;
      e.size = s[0] < 0 ? undefined : s[0];
      e.mtime = s[1];
      e.dateStr = fmtDate(s[1]);
    });
    schedulePaint(leaf);
  }
  if (leaf.sort.key === 'size' || leaf.sort.key === 'date') resort(leaf);
  updateStatus(leaf);
}

const reloadDir = (leaf) => loadDir(leaf, leaf.path, { keep: true });
function reloadAllFiles() {
  for (const t of S.tabs) for (const l of leaves(t.root)) if (l.type === 'files' && l.loaded) reloadDir(l);
}

// app.js の boot から呼ぶ(api・S はこのファイルより後に読まれる app.js で作られる)
function initFiles() {
  S.fileClip = null; // { paths:Set, cut } 切り取り中の項目を薄く出す
  api.fs.onChanged(({ id, dir }) => {
    const f = findLeaf(id);
    if (!f || f.leaf.type !== 'files') { api.fs.unwatch(id); return; }
    if (f.leaf.path === dir) reloadDir(f.leaf);
  });
  api.fs.onProgress((p) => {
    for (const t of S.tabs) for (const l of leaves(t.root)) {
      if (l.type !== 'files' || l.path !== p.destDir) continue;
      l.busy = p.finished ? '' : `${p.mode === 'move' ? '移動' : 'コピー'}中 ${p.done + 1}/${p.total}: ${p.name}`;
      updateStatus(l);
    }
  });
}

// ------------------------------------------------------------------ 描画
function schedulePaint(leaf) {
  if (leaf.paintQ) return;
  leaf.paintQ = true;
  requestAnimationFrame(() => { leaf.paintQ = false; paintRows(leaf); });
}

function makeRow() {
  const row = h('div', { class: 'frow' });
  row.ico = h('span', { class: 'ico' });
  row.nm = h('span', { class: 'name' });
  row.ty = h('span', { class: 'col type' });
  row.sz = h('span', { class: 'col size' });
  row.dt = h('span', { class: 'col date' });
  row.append(row.ico, row.nm, row.ty, row.sz, row.dt);
  return row;
}

function paintRows(leaf) {
  const v = leaf.vl;
  if (!v || !v.list.isConnected) return;
  v.err.textContent = leaf.err;
  v.err.style.display = leaf.err ? 'block' : 'none';
  v.empty.style.display = !leaf.err && leaf.loaded && !leaf.view.length && leaf.loadSeq ? 'block' : 'none';
  const n = leaf.view.length;
  v.spacer.style.height = n * ROW + 'px';
  const top = v.list.scrollTop;
  const first = Math.max(0, Math.floor(top / ROW) - 4);
  const last = Math.min(n, Math.ceil((top + v.list.clientHeight) / ROW) + 4);
  v.rows.style.transform = `translateY(${first * ROW}px)`;
  while (v.pool.length < last - first) { const r = makeRow(); v.pool.push(r); v.rows.append(r); }
  const cut = S.fileClip?.cut ? S.fileClip.paths : null;
  for (let i = 0; i < v.pool.length; i++) {
    const row = v.pool[i];
    const idx = first + i;
    if (idx >= last) { row.style.display = 'none'; continue; }
    const en = leaf.view[idx];
    row.style.display = '';
    row.dataset.i = idx;
    row.className = 'frow' + (leaf.sel.has(en.name) ? ' sel' : '') + (idx === leaf.cur ? ' cur' : '') + (cut?.has(en.full) ? ' cut' : '') + (en.name.startsWith('.') ? ' hid' : '');
    const icon = KIND_ICON[en.kind] || 'file';
    if (row.ico.dataset.k !== en.kind) { row.ico.innerHTML = ICONS[icon] || ''; row.ico.dataset.k = en.kind; row.ico.className = 'ico k-' + en.kind; }
    if (row.nm.textContent !== en.name) { row.nm.textContent = en.name; row.nm.title = en.name; }
    row.nm.classList.toggle('link', !!en.link);
    row.ty.textContent = en.isDir ? 'フォルダ' : en.ext ? en.ext.toUpperCase() : 'ファイル';
    row.sz.textContent = en.isDir || en.size == null ? '' : fmtSize(en.size);
    row.dt.textContent = en.dateStr;
    const renaming = leaf.renaming === en.name;
    row.nm.style.display = renaming ? 'none' : '';
    if (renaming && v.renIn.parentNode !== row) { row.insertBefore(v.renIn, row.ty); v.renIn.focus(); }
    else if (!renaming && v.renIn.parentNode === row) v.renIn.remove(); // 画面外へ流れたら確定(blur)
  }
  if (leaf.renaming != null && !v.renIn.isConnected) commitRename(leaf, true);
  updateStatus(leaf);
}

function ensureVisible(leaf, idx) {
  const l = leaf.vl?.list;
  if (!l) return;
  const y = idx * ROW;
  if (y < l.scrollTop) l.scrollTop = y;
  else if (y + ROW > l.scrollTop + l.clientHeight) l.scrollTop = y + ROW - l.clientHeight;
}

function updateStatus(leaf) {
  const st = leaf.ui?.status;
  if (!st) return;
  const n = leaf.view.length;
  const dirs = leaf.view.reduce((a, e) => a + e.isDir, 0);
  let left = `${n} 項目` + (dirs ? `（フォルダ ${dirs}）` : '');
  if (leaf.sel.size) {
    let size = 0;
    for (const e of leaf.view) if (leaf.sel.has(e.name) && !e.isDir) size += e.size || 0;
    left = `${leaf.sel.size} 個を選択` + (size ? `（${fmtSize(size)}）` : '') + ` / ${n} 項目`;
  }
  st.left.textContent = left;
  st.right.textContent = leaf.busy || 'F1: 操作の一覧';
  st.right.classList.toggle('busy', !!leaf.busy);
}

function updateFilesUI(leaf) {
  const u = leaf.ui;
  if (!u?.addr) return;
  if (document.activeElement !== u.addr) u.addr.value = leaf.path;
  u.up.disabled = !leaf.parent;
  u.back.disabled = !leaf.hist.back.length;
  u.fwd.disabled = !leaf.hist.fwd.length;
  for (const [k, el] of Object.entries(u.heads)) {
    el.classList.toggle('sorted', leaf.sort.key === k);
    el.dataset.dir = leaf.sort.key === k ? (leaf.sort.desc ? '▼' : '▲') : '';
  }
  updateSaveBar(leaf);
  updateStatus(leaf);
}

// ------------------------------------------------------------------ 選択・カーソル
const curEntry = (leaf) => leaf.view[leaf.cur];
// 操作の対象: 選択があれば選択、なければカーソルの1つ
function targets(leaf) {
  if (leaf.sel.size) return leaf.view.filter((e) => leaf.sel.has(e.name));
  const c = curEntry(leaf);
  return c ? [c] : [];
}

function selectRange(leaf, a, b, add) {
  const s = add ? new Set(leaf.sel) : new Set();
  for (let i = Math.min(a, b); i <= Math.max(a, b); i++) if (leaf.view[i]) s.add(leaf.view[i].name);
  leaf.sel = s;
}

// カーソル移動。Shift=範囲選択 / Ctrl=選択を変えずにカーソルだけ / なし=その1つを選択
function moveCur(leaf, idx, e = {}) {
  const n = leaf.view.length;
  if (!n) return;
  idx = clamp(idx, 0, n - 1);
  if (e.shiftKey) selectRange(leaf, leaf.anchor, idx, e.ctrlKey);
  else if (!e.ctrlKey) { leaf.sel = new Set([leaf.view[idx].name]); leaf.anchor = idx; }
  leaf.cur = idx;
  ensureVisible(leaf, idx);
  schedulePaint(leaf);
}

function toggleSel(leaf, idx) {
  const en = leaf.view[idx];
  if (!en) return;
  if (leaf.sel.has(en.name)) leaf.sel.delete(en.name); else leaf.sel.add(en.name);
  leaf.anchor = idx;
  schedulePaint(leaf);
}

// 名前の先頭を打つと、その名前へ飛ぶ(1秒あけると打ち直し)
function typeAhead(leaf, ch) {
  const now = Date.now();
  const ta = leaf.ta && now - leaf.ta.t < 1000 ? leaf.ta : { s: '', t: now };
  ta.s += ch.toLowerCase();
  ta.t = now;
  leaf.ta = ta;
  const n = leaf.view.length;
  // 同じ文字の連打は次の候補へ
  const repeat = ta.s.length > 1 && [...ta.s].every((c) => c === ta.s[0]);
  const q = repeat ? ta.s[0] : ta.s;
  const start = q.length === 1 ? leaf.cur + 1 : Math.max(0, leaf.cur);
  for (let k = 0; k < n; k++) {
    const i = (start + k) % n;
    if (leaf.view[i].name.toLowerCase().startsWith(q)) { moveCur(leaf, i); return; }
  }
}

// ------------------------------------------------------------------ 移動
const navTo = (leaf, p, name) => loadDir(leaf, p, { focusName: name });
function goUp(leaf) { if (leaf.parent) navTo(leaf, leaf.parent, baseName(leaf.path)); }
function histGo(leaf, dir) {
  const from = dir < 0 ? leaf.hist.back : leaf.hist.fwd;
  const to = dir < 0 ? leaf.hist.fwd : leaf.hist.back;
  const h1 = from.pop();
  if (!h1) return;
  to.push({ path: leaf.path, name: curEntry(leaf)?.name });
  loadDir(leaf, h1.path, { noHist: true, focusName: h1.name });
}

async function openEntries(tab, leaf, list, inNewPane) {
  if (!list.length) return;
  const dir = list.find((e) => e.isDir);
  if (inNewPane && dir) { splitLeaf(tab, leaf, 'row', 'files', { path: dir.full }); return; }
  if (list.length === 1 && dir) { freqRecord(dir.full, 'dir'); navTo(leaf, dir.full); return; }
  if (leaf.mode === 'save') return;
  const files = list.filter((e) => !e.isDir);
  if (files.length > 10 && !(await fileConfirm(`${files.length} 個のファイルを開きますか？`, '開く'))) return;
  for (const f of files) freqRecord(f.full, 'file');
  for (const f of files) api.fs.open(f.full).then((err) => { if (err) toast(`開けません: ${f.name}（${err}）`); });
}

function focusList(leaf) { leaf.vl?.list.focus({ preventScroll: true }); }

// 同じタブの次(前)のペインへフォーカスを移す
function focusPane(tab, leaf, step) {
  const all = leaves(tab.root);
  if (all.length < 2) return;
  const nx = all[(all.indexOf(leaf) + step + all.length) % all.length];
  setActivePane(tab, nx);
  if (nx.type === 'files') focusList(nx);
  else if (nx.type === 'web') webCmd(nx, 'focus');
  else (nx.ui?.addr || nx.el?.pane)?.focus?.();
}

// ------------------------------------------------------------------ 操作
async function doCopy(leaf, cut) {
  const t = targets(leaf);
  if (!t.length) return;
  await api.fs.clipWrite(t.map((e) => e.full), cut);
  S.fileClip = cut ? { paths: new Set(t.map((e) => e.full)), cut } : null;
  reloadAllPaint();
  toast(`${t.length} 個を${cut ? '切り取りました' : 'コピーしました'}`);
}
function reloadAllPaint() { for (const t of S.tabs) for (const l of leaves(t.root)) if (l.type === 'files') schedulePaint(l); }

async function doPaste(leaf) {
  const r = await api.fs.clipRead();
  if (!r.paths.length) { toast('貼り付けるファイルがありません'); return; }
  await runTransfer(leaf, r.cut ? 'move' : 'copy', r.paths, leaf.path);
  if (r.cut) { S.fileClip = null; reloadAllPaint(); }
}

async function runTransfer(leaf, mode, paths, destDir) {
  leaf.busy = `${mode === 'move' ? '移動' : 'コピー'}中…`;
  updateStatus(leaf);
  const res = await api.fs.transfer(mode, paths, destDir);
  leaf.busy = '';
  if (leaf.path === destDir) await loadDir(leaf, destDir, { keep: true, selectNames: res.names });
  else updateStatus(leaf);
  if (res.errors.length) toast(`${res.errors.length} 件失敗: ${res.errors[0]}`);
  else toast(`${res.names.length} 個を${mode === 'move' ? '移動' : 'コピー'}しました`);
}

async function doDelete(leaf, permanent) {
  const t = targets(leaf);
  if (!t.length) return;
  if (permanent) {
    const names = t.slice(0, 5).map((e) => e.name).join('、') + (t.length > 5 ? ` ほか ${t.length - 5} 個` : '');
    if (!(await fileConfirm(`完全に削除します（元に戻せません）\n${names}`, '削除', true))) return;
  }
  const r = await api.fs.remove(t.map((e) => e.full), permanent);
  leaf.sel = new Set();
  await reloadDir(leaf);
  if (!r.ok) toast(`${r.errors.length} 件失敗: ${r.errors[0]}${permanent ? '' : '（Shift+Delete で完全に削除）'}`);
  else toast(`${t.length} 個を${permanent ? '削除しました' : 'ゴミ箱へ移動しました'}`);
}

async function newFolder(leaf) {
  const r = await api.fs.mkdir(leaf.path, '新しいフォルダ');
  if (!r.ok) { toast(r.error); return; }
  await loadDir(leaf, leaf.path, { keep: true, focusName: r.name });
  startRename(leaf, leaf.cur);
}

function startRename(leaf, idx) {
  const en = leaf.view[idx];
  if (!en || !leaf.vl) return;
  leaf.cur = idx;
  ensureVisible(leaf, idx);
  leaf.renaming = en.name;
  const inp = leaf.vl.renIn;
  inp.value = en.name;
  paintRows(leaf);
  inp.focus();
  const dot = en.isDir ? -1 : en.name.lastIndexOf('.');
  inp.setSelectionRange(0, dot > 0 ? dot : en.name.length); // 拡張子は選ばない
}

async function commitRename(leaf, ok) {
  const name = leaf.renaming;
  if (name == null) return;
  leaf.renaming = null;
  const inp = leaf.vl.renIn;
  const val = inp.value.trim();
  if (inp.isConnected) { focusList(leaf); inp.remove(); }
  if (!ok || !val || val === name) { schedulePaint(leaf); return; }
  const r = await api.fs.rename(joinPath(leaf.path, name), val);
  if (!r.ok) { toast(r.error); schedulePaint(leaf); return; }
  await loadDir(leaf, leaf.path, { keep: true, focusName: r.name });
}

async function openTerminalHere(tab, leaf) {
  const c = curEntry(leaf);
  openTerminalAt(tab, leaf, leaf.sel.size <= 1 && c?.isDir && leaf.sel.has(c.name) ? c.full : leaf.path);
}
// dir で端末を開く。取り込む設定なら leaf の下に分割して出す(フィールド表示からも使う)
async function openTerminalAt(tab, leaf, dir) {
  if (S.appSupported && C.behavior.appOpen === 'embed') {
    const cmd = await api.fs.terminalCmd(dir);
    const bin = cmd.split(/\s+/)[0].split('/').pop();
    const app = (S.appList || []).find((a) => a.exec.split(/\s+/)[0].split('/').pop() === bin);
    const nl = splitLeaf(tab, leaf, 'col', 'app');
    startApp(nl, cmd, app?.cls || '', app?.name || bin, app?.icon || '');
  } else if (!(await api.fs.terminal(dir))) toast('端末を起動できませんでした（設定「端末のコマンド」）');
}

async function copyPaths(leaf) {
  const t = targets(leaf);
  const s = t.length ? t.map((e) => e.full).join('\n') : leaf.path;
  await api.fs.copyText(s);
  toast('パスをコピーしました');
}

function toggleHidden() {
  api.config.set('behavior.showHidden', !C.behavior.showHidden);
  toast(C.behavior.showHidden ? '隠しファイルを隠します' : '隠しファイルを表示します');
}

// ------------------------------------------------------------------ ダイアログ
// キーボードで閉じられる小さなダイアログ。Enter=既定のボタン、Esc=キャンセル(null)
function fileDialog(title, content, buttons) {
  return new Promise((resolve) => {
    const prev = document.activeElement;
    const bd = h('div', { class: 'backdrop modal-bg' });
    const done = (v) => { bd.remove(); box.remove(); suspend(-1); window.removeEventListener('keydown', onKey, true); prev?.focus?.({ preventScroll: true }); resolve(v); };
    const btns = buttons.map((b) => h('button', { class: 'textbtn' + (b.primary ? ' primary' : '') + (b.danger ? ' danger' : ''), onclick: () => done(b.value) }, b.label));
    const box = h('div', { class: 'modal fdialog' }, h('div', { class: 'ftitle' }, title), content, h('div', { class: 'row' }, btns));
    const onKey = (e) => {
      if (e.key === 'Escape') { e.preventDefault(); e.stopPropagation(); done(null); }
      else if (e.key === 'Enter') { e.preventDefault(); e.stopPropagation(); const b = buttons.find((x) => x.primary) || buttons[0]; done(b.value); }
      else if (e.key === 'Tab' || e.key === 'ArrowLeft' || e.key === 'ArrowRight') {
        // ボタン間の移動だけ(背後の一覧へ抜けない)
        e.preventDefault();
        const i = btns.indexOf(document.activeElement);
        btns[(i + (e.shiftKey || e.key === 'ArrowLeft' ? btns.length - 1 : 1)) % btns.length].focus();
      }
    };
    window.addEventListener('keydown', onKey, true);
    overlay.append(bd, box);
    suspend(+1);
    box.tabIndex = -1;
    box.focus();
  });
}
const fileConfirm = (msg, ok, danger) => fileDialog(msg, null, [
  { label: 'キャンセル', value: false },
  { label: ok, value: true, primary: true, danger },
]).then((v) => !!v);

function showProps(leaf) {
  const t = targets(leaf);
  return showPropsFor(t.length ? t.map((e) => e.full) : [leaf.path], leaf.path);
}
// paths のプロパティ。where: 複数の時に「場所」に出すフォルダ(フィールド表示からも使う)
async function showPropsFor(paths, where) {
  const items = await api.fs.props(paths);
  const kv = (k, v) => (v == null || v === '' ? null : h('div', { class: 'kv' }, h('span', { class: 'k' }, k), h('span', { class: 'v' }, String(v))));
  const sizeEl = h('span', { class: 'v' });
  let body;
  const needSize = items.some((it) => it.isDir);
  if (items.length === 1) {
    const it = items[0];
    body = h('div', { class: 'props' },
      kv('名前', it.name), kv('場所', it.full), it.error ? kv('エラー', it.error) : null,
      kv('種類', it.isDir ? `フォルダ（中身 ${it.count} 項目）` : 'ファイル'), it.link != null ? kv('リンク先', it.link) : null,
      h('div', { class: 'kv' }, h('span', { class: 'k' }, 'サイズ'), sizeEl),
      kv('更新日時', it.mtime ? DF.format(it.mtime) : ''), kv('作成日時', it.btime ? DF.format(it.btime) : ''),
      kv('権限', it.mode), kv('所有者(uid)', it.uid));
    sizeEl.textContent = it.isDir ? '計算中…' : `${fmtSize(it.size)}（${it.size.toLocaleString()} バイト）`;
  } else {
    const dirs = items.filter((x) => x.isDir).length;
    body = h('div', { class: 'props' }, kv('場所', where), kv('項目', `${items.length} 個（フォルダ ${dirs}、ファイル ${items.length - dirs}）`),
      h('div', { class: 'kv' }, h('span', { class: 'k' }, '合計サイズ'), sizeEl));
    sizeEl.textContent = '計算中…';
    if (!needSize) sizeEl.textContent = fmtSize(items.reduce((a, x) => a + (x.size || 0), 0));
  }
  if (needSize) {
    api.fs.dirSize(paths).then((r) => {
      sizeEl.textContent = `${fmtSize(r.size)}（ファイル ${r.files.toLocaleString()} 個）${r.partial ? ' ※時間切れのため途中まで' : ''}`;
    });
  }
  await fileDialog('プロパティ', body, [{ label: '閉じる', value: true, primary: true }]);
}

const FILE_KEYS = [
  ['↑ ↓ / PageUp PageDown / Home End', '移動'],
  ['Shift + 移動', '範囲を選択'],
  ['Ctrl + 移動', '選択を変えずにカーソルだけ移動'],
  ['Space / Insert', 'カーソルの項目の選択を切り替え（Insert は次へ進む）'],
  ['Ctrl+A / Esc', 'すべて選択 / 選択を解除'],
  ['文字を打つ', 'その名前へ飛ぶ'],
  ['Enter', '開く（フォルダなら入る）'],
  ['Ctrl+Enter', 'フォルダを右のペインで開く'],
  ['Backspace / Alt+↑', '上のフォルダへ'],
  ['Alt+← / Alt+→', '戻る / 進む'],
  ['Alt+Home', 'ホームフォルダ'],
  ['Ctrl+L', 'パスを入力（Esc で一覧へ戻る）'],
  ['F5 / Ctrl+R', '再読込'],
  ['Ctrl+C / Ctrl+X / Ctrl+V', 'コピー / 切り取り / 貼り付け'],
  ['F2', '名前を変更'],
  ['Delete / Shift+Delete', 'ゴミ箱へ / 完全に削除'],
  ['Ctrl+Shift+N', '新しいフォルダ'],
  ['Ctrl+Shift+C', 'パスをコピー'],
  ['Ctrl+H', '隠しファイルの表示を切り替え'],
  ['Shift+F4', 'ここで端末を開く'],
  ['Alt+Enter', 'プロパティ'],
  ['Menu / Shift+F10', '右クリックメニュー（↑↓ と Enter で選ぶ）'],
  ['Tab / Shift+Tab', '隣のペインへ'],
];
function showFileHelp() {
  const body = h('div', { class: 'props keys' }, FILE_KEYS.map(([k, v]) => h('div', { class: 'kv' }, h('span', { class: 'k' }, k), h('span', { class: 'v' }, v))));
  fileDialog('フォルダの操作', body, [{ label: '閉じる', value: true, primary: true }]);
}

// ------------------------------------------------------------------ メニュー
function fileMenu(tab, leaf, x, y, kb = false) {
  const t = targets(leaf);
  const one = t.length === 1 ? t[0] : null;
  const has = t.length > 0;
  const items = has ? [
    { label: '開く', icon: one?.isDir ? 'folder' : 'file', key: 'Enter', run: () => openEntries(tab, leaf, t) },
    one?.isDir ? { label: '右のペインで開く', icon: 'split-h', key: 'Ctrl+Enter', run: () => openEntries(tab, leaf, t, true) } : null,
    { sep: true },
    { label: '切り取り', key: 'Ctrl+X', run: () => doCopy(leaf, true) },
    { label: 'コピー', key: 'Ctrl+C', run: () => doCopy(leaf, false) },
    { label: '貼り付け', key: 'Ctrl+V', run: () => doPaste(leaf) },
    { label: '名前を変更', icon: 'text', key: 'F2', disabled: !one, run: () => startRename(leaf, leaf.view.indexOf(one)) },
    { sep: true },
    { label: 'ゴミ箱へ移動', icon: 'close', key: 'Delete', run: () => doDelete(leaf, false) },
    { label: '完全に削除', danger: true, key: 'Shift+Del', run: () => doDelete(leaf, true) },
    { sep: true },
    { label: 'パスをコピー', key: 'Ctrl+Shift+C', run: () => copyPaths(leaf) },
    { label: '端末で開く', key: 'Shift+F4', run: () => openTerminalHere(tab, leaf) },
    { label: 'プロパティ', key: 'Alt+Enter', run: () => showProps(leaf) },
  ] : [
    { label: '新しいフォルダ', icon: 'folder', key: 'Ctrl+Shift+N', run: () => newFolder(leaf) },
    { label: '貼り付け', key: 'Ctrl+V', run: () => doPaste(leaf) },
    { sep: true },
    { label: '隠しファイルを表示', checked: !!C.behavior.showHidden, key: 'Ctrl+H', run: toggleHidden },
    { label: '再読込', icon: 'reload', key: 'F5', run: () => reloadDir(leaf) },
    { sep: true },
    { label: 'パスをコピー', key: 'Ctrl+Shift+C', run: () => copyPaths(leaf) },
    { label: '端末で開く', key: 'Shift+F4', run: () => openTerminalHere(tab, leaf) },
    { label: 'プロパティ', key: 'Alt+Enter', run: () => showProps(leaf) },
  ];
  popup(items.filter(Boolean), x, y, { keyboard: kb });
}

// キーボードで開いたメニューはカーソルの行の下に出す
function fileMenuAtCursor(tab, leaf) {
  const v = leaf.vl;
  const lr = v.list.getBoundingClientRect();
  const y = leaf.cur >= 0 ? clamp(lr.top + (leaf.cur + 1) * ROW - v.list.scrollTop, lr.top, lr.bottom - 10) : lr.top;
  fileMenu(tab, leaf, lr.left + 40, y, true);
}

// ------------------------------------------------------------------ キー
function onFilesKey(tab, leaf, e) {
  if (e.target !== leaf.vl.list) return; // 名前変更の入力欄など
  const k = e.key;
  const ctrl = e.ctrlKey || e.metaKey;
  const page = Math.max(1, Math.floor(leaf.vl.list.clientHeight / ROW) - 1);
  const c = leaf.cur;
  let done = true;
  if (k === 'ArrowDown') moveCur(leaf, c < 0 ? 0 : c + 1, e);
  else if (k === 'ArrowUp' && e.altKey) goUp(leaf);
  // Alt+←/→・F5・Ctrl+R は普段は main.js のショートカットが先に受けて onShortcut から来る。ここは届いたとき用
  else if (k === 'ArrowLeft' && e.altKey) histGo(leaf, -1);
  else if (k === 'ArrowRight' && e.altKey) histGo(leaf, 1);
  else if (k === 'F5' || (ctrl && k === 'r')) reloadDir(leaf);
  else if (k === 'ArrowUp') moveCur(leaf, c < 0 ? 0 : c - 1, e);
  else if (k === 'PageDown') moveCur(leaf, c + page, e);
  else if (k === 'PageUp') moveCur(leaf, c - page, e);
  else if (k === 'Home' && e.altKey) api.fs.home().then((p) => navTo(leaf, p));
  else if (k === 'Home') moveCur(leaf, 0, e);
  else if (k === 'End') moveCur(leaf, leaf.view.length - 1, e);
  else if (k === 'Enter' && e.altKey) showProps(leaf);
  else if (k === 'Enter') openEntries(tab, leaf, ctrl ? [curEntry(leaf)].filter(Boolean) : leaf.sel.size > 1 && leaf.sel.has(curEntry(leaf)?.name) ? targets(leaf) : [curEntry(leaf)].filter(Boolean), ctrl);
  else if (k === 'Backspace') goUp(leaf);
  else if (k === ' ' && !(leaf.ta && Date.now() - leaf.ta.t < 1000)) toggleSel(leaf, c);
  else if (k === 'Insert') { toggleSel(leaf, c); moveCur(leaf, c + 1, { ctrlKey: true }); }
  else if (k === 'Escape') { leaf.sel = new Set(); schedulePaint(leaf); done = false; } // Esc は全体(ポップアップを閉じる)にも渡す
  else if (k === 'F1') showFileHelp();
  else if (k === 'F2') startRename(leaf, c);
  else if (k === 'F4' && e.shiftKey) openTerminalHere(tab, leaf);
  else if (k === 'Delete') doDelete(leaf, e.shiftKey);
  else if (k === 'ContextMenu' || (k === 'F10' && e.shiftKey)) fileMenuAtCursor(tab, leaf);
  else if (k === 'Tab') focusPane(tab, leaf, e.shiftKey ? -1 : 1);
  else if (ctrl && !e.altKey && k.length === 1) {
    const ch = k.toLowerCase();
    if (ch === 'a') { leaf.sel = new Set(leaf.view.map((x) => x.name)); schedulePaint(leaf); }
    else if (ch === 'c' && e.shiftKey) copyPaths(leaf);
    else if (ch === 'c') doCopy(leaf, false);
    else if (ch === 'x') doCopy(leaf, true);
    else if (ch === 'v') doPaste(leaf);
    else if (ch === 'n' && e.shiftKey) newFolder(leaf);
    else if (ch === 'h') toggleHidden();
    else if (ch === 's' && leaf.mode === 'save') chooseSave(tab, leaf);
    else done = false;
  } else if (k.length === 1 && !e.altKey && !ctrl) typeAhead(leaf, k);
  else done = false;
  if (done) { e.preventDefault(); e.stopPropagation(); }
}

// ------------------------------------------------------------------ マウス
function rowIndexAt(leaf, e) {
  const row = e.target.closest?.('.frow');
  return row && row.parentNode === leaf.vl.rows ? Number(row.dataset.i) : -1;
}

function onListPointerDown(tab, leaf, e) {
  const v = leaf.vl;
  if (e.target === v.renIn) return;
  if (e.button === 3) { e.preventDefault(); histGo(leaf, -1); return; } // マウスの戻る/進むボタン
  if (e.button === 4) { e.preventDefault(); histGo(leaf, 1); return; }
  if (e.button !== 0) return;
  if (e.target === v.list && e.offsetX >= v.list.clientWidth) return; // スクロールバー
  const idx = rowIndexAt(leaf, e);
  const ctrl = e.ctrlKey || e.metaKey;
  if (idx >= 0) {
    if (e.shiftKey) { selectRange(leaf, leaf.anchor, idx, ctrl); leaf.cur = idx; }
    else if (ctrl) { leaf.cur = idx; toggleSel(leaf, idx); }
    else if (!leaf.sel.has(leaf.view[idx].name)) { leaf.sel = new Set([leaf.view[idx].name]); leaf.cur = leaf.anchor = idx; }
    else { leaf.cur = idx; leaf.pendingSingle = idx; } // 複数選択中の1つを押した: 離したときに1つだけにする
    schedulePaint(leaf);
  }
  // 名前とアイコンの上はドラッグ(第2弾)用に空けておき、それ以外の所からは範囲選択
  bandSelect(leaf, e, idx < 0 && !ctrl && !e.shiftKey, !(idx >= 0 && e.target.closest('.name, .ico')));
}

function bandSelect(leaf, e, clearFirst, allowBand) {
  const v = leaf.vl;
  const lr = v.list.getBoundingClientRect();
  const y0 = e.clientY - lr.top + v.list.scrollTop;
  const x0 = e.clientX - lr.left;
  if (clearFirst) { leaf.sel = new Set(); leaf.cur = -1; schedulePaint(leaf); }
  const base = new Set(leaf.sel);
  let band = null;
  let lastY = e.clientY, lastX = e.clientX;
  const update = () => {
    if (!allowBand) return;
    const y1 = clamp(lastY - lr.top + v.list.scrollTop, 0, v.list.scrollHeight);
    const x1 = clamp(lastX - lr.left, 0, v.list.clientWidth);
    if (!band) {
      if (Math.abs(y1 - y0) < 4 && Math.abs(x1 - x0) < 4) return;
      band = h('div', { class: 'band' });
      v.list.append(band);
      leaf.pendingSingle = null;
    }
    Object.assign(band.style, { left: Math.min(x0, x1) + 'px', top: Math.min(y0, y1) + 'px', width: Math.abs(x1 - x0) + 'px', height: Math.abs(y1 - y0) + 'px' });
    const a = Math.floor(Math.min(y0, y1) / ROW);
    const b = Math.min(leaf.view.length - 1, Math.floor(Math.max(y0, y1) / ROW));
    const s = new Set(base);
    for (let i = Math.max(0, a); i <= b; i++) s.add(leaf.view[i].name);
    leaf.sel = s;
    if (b >= 0) leaf.cur = clamp(Math.floor(y1 / ROW), 0, leaf.view.length - 1);
    schedulePaint(leaf);
  };
  // 上下の端まで引っぱったら自動でスクロール
  const timer = setInterval(() => {
    const d = lastY < lr.top + 8 ? -ROW : lastY > lr.bottom - 8 ? ROW : 0;
    if (d && band) { v.list.scrollTop += d; update(); }
  }, 50);
  const move = (ev) => { lastY = ev.clientY; lastX = ev.clientX; update(); };
  const up = () => {
    clearInterval(timer);
    window.removeEventListener('pointermove', move);
    window.removeEventListener('pointerup', up);
    band?.remove();
    if (leaf.pendingSingle != null) { const en = leaf.view[leaf.pendingSingle]; if (en) { leaf.sel = new Set([en.name]); leaf.anchor = leaf.pendingSingle; } leaf.pendingSingle = null; schedulePaint(leaf); }
  };
  window.addEventListener('pointermove', move);
  window.addEventListener('pointerup', up);
}

// 列の幅を、見出しの左端をドラッグして変える
function dragCol(e, key) {
  e.preventDefault();
  e.stopPropagation();
  const x0 = e.clientX;
  const w0 = COLS[key];
  const move = (ev) => { COLS[key] = clamp(w0 - (ev.clientX - x0), 40, 400); applyCols(); };
  const up = () => {
    window.removeEventListener('pointermove', move);
    window.removeEventListener('pointerup', up);
    try { localStorage.setItem('fileCols', JSON.stringify(COLS)); } catch {}
  };
  window.addEventListener('pointermove', move);
  window.addEventListener('pointerup', up);
}

// ------------------------------------------------------------------ ペインの組み立て
function buildFiles(tab, leaf, head, body) {
  const back = btn('back', '戻る (Alt+←)', () => histGo(leaf, -1));
  const fwd = btn('forward', '進む (Alt+→)', () => histGo(leaf, 1));
  const up = btn('up', '上へ (Backspace)', () => goUp(leaf));
  const home = btn('home', 'ホーム (Alt+Home)', async () => navTo(leaf, await api.fs.home()));
  const drv = btn('drive', 'ドライブ・場所', async () => {
    const list = await api.fs.drives();
    popupAt(drv, list.map((d) => ({ label: d.label, icon: 'drive', run: () => { navTo(leaf, d.path); focusList(leaf); } })));
  });
  const addr = h('input', {
    type: 'text', class: 'addr', spellcheck: 'false', value: leaf.path, placeholder: 'パス',
    onfocus: (e) => e.target.select(),
    onkeydown: (e) => {
      if (e.key === 'Enter') { loadDir(leaf, addr.value); focusList(leaf); }
      else if (e.key === 'Escape') { addr.value = leaf.path; focusList(leaf); }
      else return;
      e.preventDefault();
      e.stopPropagation();
    },
  });
  head.append(back, fwd, up, home, drv, addr, ...commonButtons(tab, leaf));
  body.classList.add('files');

  const heads = {};
  const headCell = (key, label) => {
    const el = h('div', { class: 'fh ' + key, onclick: () => {
      leaf.sort = { key, desc: leaf.sort.key === key ? !leaf.sort.desc : key === 'date' || key === 'size' };
      resort(leaf);
      updateFilesUI(leaf);
      scheduleSave();
      focusList(leaf);
    } }, label, key !== 'name' ? h('span', { class: 'grip', onpointerdown: (e) => dragCol(e, key), onclick: (e) => e.stopPropagation() }) : null);
    heads[key] = el;
    return el;
  };
  const fhead = h('div', { class: 'fhead' }, h('span', { class: 'ico' }), headCell('name', '名前'), headCell('type', '種類'), headCell('size', 'サイズ'), headCell('date', '更新日時'));

  const list = h('div', { class: 'vlist', tabindex: '0' });
  const spacer = h('div', { class: 'vspacer' });
  const rows = h('div', { class: 'vrows' });
  const err = h('div', { class: 'ferr', style: { display: 'none' } });
  const empty = h('div', { class: 'fempty', style: { display: 'none' } }, '空のフォルダ');
  const renIn = h('input', { type: 'text', class: 'ren', spellcheck: 'false' });
  renIn.addEventListener('keydown', (e) => {
    e.stopPropagation();
    if (e.key === 'Enter') { e.preventDefault(); commitRename(leaf, true); }
    else if (e.key === 'Escape') { e.preventDefault(); commitRename(leaf, false); }
  });
  renIn.addEventListener('blur', () => commitRename(leaf, true));
  renIn.addEventListener('pointerdown', (e) => e.stopPropagation());
  list.append(spacer, rows);
  const wrap = h('div', { class: 'vwrap' }, list, err, empty);
  const status = h('div', { class: 'fstatus' });
  const st = { left: h('span', { class: 'l' }), right: h('span', { class: 'r' }) };
  status.append(st.left, st.right);
  body.append(fhead, wrap, status);
  leaf.vl = { list, spacer, rows, err, empty, renIn, pool: [] };
  leaf.ui = { back, fwd, up, addr, heads, status: st };

  list.addEventListener('scroll', () => { leaf.scrollTop = list.scrollTop; schedulePaint(leaf); });
  list.addEventListener('keydown', (e) => onFilesKey(tab, leaf, e));
  list.addEventListener('pointerdown', (e) => onListPointerDown(tab, leaf, e));
  list.addEventListener('dblclick', (e) => { const i = rowIndexAt(leaf, e); if (i >= 0) openEntries(tab, leaf, [leaf.view[i]]); });
  list.addEventListener('contextmenu', (e) => {
    e.preventDefault();
    const i = rowIndexAt(leaf, e);
    if (i < 0) { leaf.sel = new Set(); leaf.cur = -1; }
    else if (!leaf.sel.has(leaf.view[i].name)) { leaf.sel = new Set([leaf.view[i].name]); leaf.cur = leaf.anchor = i; }
    paintRows(leaf);
    fileMenu(tab, leaf, e.clientX, e.clientY);
  });
  new ResizeObserver(() => schedulePaint(leaf)).observe(list);

  if (leaf.mode === 'save') {
    const msg = h('span', { class: 'msg' });
    const bar = h('div', { class: 'savebar' }, msg,
      h('button', { class: 'textbtn primary', title: 'Ctrl+S', onclick: () => chooseSave(tab, leaf) }, 'ここに保存'),
      h('button', { class: 'textbtn', onclick: () => closeLeaf(tab, leaf) }, 'キャンセル'));
    leaf.ui.saveMsg = msg;
    body.append(bar);
  }

  if (leaf.loaded) {
    list.scrollTop = leaf.scrollTop;
    updateFilesUI(leaf);
    paintRows(leaf);
  } else {
    leaf.loaded = true;
    updateFilesUI(leaf);
    loadDir(leaf);
  }
  // 作り直したとき(タブ切替・分割など)、作業中のペインならキーボードの入力先にする
  if (tab === activeTab() && tab.activePane === leaf.id) {
    requestAnimationFrame(() => { const a = document.activeElement; if (!a || a === document.body) focusList(leaf); });
  }
}

function chooseSave(tab, leaf) {
  leaf.dlHandled = true;
  api.dl.choose(leaf.dlId, leaf.path);
  closeLeaf(tab, leaf);
}

function updateSaveBar(leaf) {
  if (leaf.mode !== 'save' || !leaf.ui?.saveMsg) return;
  const d = S.dl?.[leaf.dlId];
  let st = '';
  if (d) {
    if (d.state === 'completed') st = '（ダウンロード完了）';
    else if (d.state) st = '（ダウンロード失敗）';
    else if (d.total > 0) st = `（${Math.floor((d.received / d.total) * 100)}%）`;
    else if (d.received) st = `（${fmtSize(d.received)}）`;
  }
  leaf.ui.saveMsg.textContent = `「${leaf.filename}」の保存先を選んでください${st}（Ctrl+S でここに保存）`;
}
