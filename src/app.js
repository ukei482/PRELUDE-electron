'use strict';
// PRELUDE renderer — 縦タブ / ブックマーク / 分割ペイン(Web・フォルダ・設定) を1つのウィンドウに。
// Webページ本体は main.js 側の WebContentsView で、ここの .web-slot の位置に重ねて表示される。

const api = window.preludeApi;
const $ = (s, el = document) => el.querySelector(s);

// ------------------------------------------------------------------ helpers
function h(tag, props, ...kids) {
  const el = document.createElement(tag);
  for (const [k, v] of Object.entries(props || {})) {
    if (v == null || v === false) continue;
    if (k === 'class') el.className = v;
    else if (k === 'style' && typeof v === 'object') Object.assign(el.style, v);
    else if (k === 'html') el.innerHTML = v;
    else if (k.startsWith('on') && typeof v === 'function') el.addEventListener(k.slice(2), v);
    else el.setAttribute(k, v === true ? '' : v);
  }
  for (const kid of kids.flat(Infinity)) if (kid != null && kid !== false) el.append(kid.nodeType ? kid : String(kid));
  return el;
}

let ICONS = {};
const ico = (name) => h('span', { class: 'ico', html: ICONS[name] || '' });
const btn = (icon, title, onclick) => h('button', { class: 'ibtn', title, onclick }, ico(icon));

// 画面ごとの窓(main.js の createShellWindow)。2枚目以降の画面(role=sub)は、設定・全画面・システム関係のボタンを省く
const Q = new URLSearchParams(location.search);
const WIN_KEY = Q.get('key') || 'main';
const IS_SUB = Q.get('role') === 'sub';
let seq = 0;
// ペインの id は全部の窓で重ならないようにする(main.js はペインを id で管理し、タブを別の窓へ移しても同じ id のまま)
const ID_PREFIX = `${WIN_KEY}${Math.random().toString(36).slice(2, 6)}-`;
const nid = (p) => `${ID_PREFIX}${p}${++seq}`;
const clamp = (v, lo, hi) => Math.min(hi, Math.max(lo, v));
const baseName = (p) => String(p || '').replace(/[\\/]+$/, '').split(/[\\/]/).pop() || String(p || '');
const hostOf = (u) => { try { return new URL(u).hostname; } catch { return ''; } };
const fmtSize = (n) => (n < 1024 ? n + ' B' : n < 1048576 ? (n / 1024).toFixed(1) + ' KB' : n < 1073741824 ? (n / 1048576).toFixed(1) + ' MB' : (n / 1073741824).toFixed(2) + ' GB');
const getPath = (o, p) => p.split('.').reduce((v, k) => (v == null ? undefined : v[k]), o);

// ------------------------------------------------------------------ state
let C = null; // config values
let SCHEMA = [];
const S = { tabs: [], active: null, bookmarks: [], screens: [] };

const activeTab = () => S.tabs.find((t) => t.id === S.active) || S.tabs[0];
const leaves = (n) => (n.k === 'leaf' ? [n] : [...leaves(n.a), ...leaves(n.b)]);
const activeLeaf = (tab) => (tab ? leaves(tab.root).find((l) => l.id === tab.activePane) || leaves(tab.root)[0] : null);
function findLeaf(id) {
  for (const tab of S.tabs) for (const leaf of leaves(tab.root)) if (leaf.id === id) return { tab, leaf };
  return null;
}
function findParent(node, target) {
  if (node.k !== 'split') return null;
  if (node.a === target || node.b === target) return node;
  return findParent(node.a, target) || findParent(node.b, target);
}
function replaceNode(tab, oldNode, newNode) {
  if (tab.root === oldNode) { tab.root = newNode; return; }
  const p = findParent(tab.root, oldNode);
  if (p.a === oldNode) p.a = newNode; else p.b = newNode;
}

function makeLeaf(type, o = {}) {
  const id = nid('p');
  if (type === 'web') return { k: 'leaf', id, type, url: o.url || '', title: '', favicon: '', loading: false, canBack: false, canFwd: false };
  if (type === 'files') {
    return {
      k: 'leaf', id, type, path: o.path ?? C.behavior.startPath ?? '', ...newFilesState(o),
      mode: o.mode || 'browse', dlId: o.dlId, filename: o.filename || '', dlMsg: '',
    };
  }
  if (type === 'app') return { k: 'leaf', id, type, cmd: o.cmd || '', cls: o.cls || '', name: o.name || '', icon: o.icon || '', webUrl: o.webUrl || '', title: '', running: false, apps: null, filter: '' };
  if (type === 'home') return { k: 'leaf', id, type };
  if (type === 'field') return { k: 'leaf', id, type, path: o.path ?? C.behavior.startPath ?? '' };
  return { k: 'leaf', id, type: 'settings' };
}

// ------------------------------------------------------------------ theme / sidebar
function applyTheme() {
  const a = C.appearance;
  const r = document.documentElement.style;
  for (const k of ['bg', 'panel', 'panel2', 'line', 'text', 'dim', 'accent', 'danger']) r.setProperty('--' + k, a[k]);
  r.setProperty('--fs', a.fontSize + 'px');
  r.setProperty('--sbw', a.sidebarWidth + 'px');
  const mode = C.behavior.sidebarMode;
  document.body.classList.toggle('no-sidebar', mode === 'hidden');
  document.body.classList.toggle('sb-compact', mode === 'compact');
  document.querySelectorAll('.sb-toggle').forEach((b) => { b.title = sidebarTip(); });
}
const sidebarTip = () => `Sidebar (next: ${{ full: 'compact', compact: 'hidden', hidden: 'full' }[C.behavior.sidebarMode] || 'compact'})`;
// 上のバーの PRELUDE: 全部表示 → 細い列(アイコンだけ)→ 非表示 → 全部表示 の順に回す
const SB_MODES = ['full', 'compact', 'hidden'];
const cycleSidebar = () => api.config.set('behavior.sidebarMode', SB_MODES[(SB_MODES.indexOf(C.behavior.sidebarMode) + 1) % SB_MODES.length]);

const tabTitle = (tab) => leafTitle(activeLeaf(tab));
function leafTitle(l) {
  if (!l) return '';
  if (l.type === 'web') return l.title || hostOf(l.url) || 'New tab';
  if (l.type === 'files') return l.mode === 'save' ? 'Choose save location' : baseName(l.path) || 'Folder';
  if (l.type === 'app') return l.title || l.name || l.cmd || 'App'; // 取り込んだ窓の題名(Chromium なら今のページ名)を優先
  if (l.type === 'home') return 'Home';
  if (l.type === 'field') return baseName(l.path) || 'Field';
  return 'Settings';
}
const leafIcon = (l) => (l.type === 'web' ? 'globe' : l.type === 'files' ? 'folder' : l.type === 'app' ? 'window' : l.type === 'home' ? 'home' : l.type === 'field' ? 'field' : 'settings');

// アプリのペインのアイコン(.desktop の Icon。実ブラウザのペインは Chromium のアイコン)。まだ読めていなければ汎用のアイコンを出し、読めたら描き直す
function appLeafIcon(l) {
  if (isDevLeaf(l)) return ico('code');
  if (l.iconUrl) return h('img', { class: 'fav', src: l.iconUrl, onerror: (e) => e.target.replaceWith(ico('window')) });
  const list = S.appList || [];
  const name = l.icon || (l.webUrl ? list.find((a) => /chrom/i.test(a.exec))?.icon : list.find((a) => a.exec === l.cmd)?.icon) || '';
  if (name && !l.iconTried) {
    l.iconTried = true;
    appIconUrl(name).then((u) => { if (u) { l.iconUrl = u; renderSidebar(); if (l.ui?.icon?.isConnected) l.ui.icon.replaceWith(appLeafIcon(l)); } });
  }
  return ico('window');
}

// サイドバーの項目に出すアイコン(Web はファビコン、アプリは .desktop のアイコン)
const leafIconEl = (l) => (l.type === 'web' && l.favicon ? h('img', { class: 'fav', src: l.favicon, onerror: (e) => e.target.replaceWith(ico('globe')) })
  : l.type === 'app' ? appLeafIcon(l) : ico(leafIcon(l)));
const leafPlace = (l) => (l.type === 'web' ? l.url : l.type === 'app' ? l.webUrl || l.cmd : l.path) || '';
// 音が鳴っている(または消してある)Web ペインの音のボタン。押すと消す/戻す
function soundBtn(ls) {
  const web = ls.filter((l) => l.type === 'web' && (l.audible || l.muted));
  if (!web.length) return null;
  const muted = web.every((l) => l.muted);
  return h('button', {
    class: 'ibtn snd' + (muted ? ' muted' : ''), title: muted ? 'Unmute' : 'Mute',
    onclick: (e) => { e.stopPropagation(); setMuted(ls, !muted); },
  }, ico(muted ? 'volume-off' : 'volume'));
}
const setMuted = (ls, on) => ls.forEach((l) => { if (l.type === 'web') webCmd(l, 'mute', on); });

// ドラッグで並べ替える。list は並び順そのもの(S.tabs・S.bookmarks)で、同じ list の中だけで動かせる
let sbDrag = null;
function sortable(el, list, item, done) {
  el.draggable = true;
  el.ondragstart = (e) => { sbDrag = { list, item }; e.dataTransfer.effectAllowed = 'move'; el.classList.add('dragging'); };
  el.ondragend = () => { sbDrag = null; document.querySelectorAll('#sidebar .drop-a, #sidebar .drop-b, #sidebar .dragging').forEach((x) => x.classList.remove('drop-a', 'drop-b', 'dragging')); };
  const after = (e) => { const r = el.getBoundingClientRect(); return e.clientY > r.top + r.height / 2; };
  el.ondragover = (e) => {
    if (sbDrag?.list !== list || sbDrag.item === item) return;
    e.preventDefault();
    e.dataTransfer.dropEffect = 'move';
    const b = after(e);
    el.classList.toggle('drop-b', b);
    el.classList.toggle('drop-a', !b);
  };
  el.ondragleave = () => el.classList.remove('drop-a', 'drop-b');
  el.ondrop = (e) => {
    if (sbDrag?.list !== list || sbDrag.item === item) return;
    e.preventDefault();
    const b = after(e);
    list.splice(list.indexOf(sbDrag.item), 1);
    list.splice(list.indexOf(item) + (b ? 1 : 0), 0, sbDrag.item);
    done();
  };
}

function renderSidebar() {
  renderBookmarks();

  $('#tab-count').textContent = S.tabs.length > 1 ? String(S.tabs.length) : '';
  const tl = $('#tab-list');
  tl.replaceChildren();
  tl.oncontextmenu = (e) => {
    if (e.target !== tl) return;
    e.preventDefault();
    popup([
      { label: 'New tab', icon: 'plus', run: () => openTab(C.behavior.startKind) },
      { label: 'Reopen closed tab', icon: 'back', key: C.shortcuts.reopenTab, disabled: !S.closed.length, run: reopenClosedTab },
    ], e.clientX, e.clientY);
  };
  for (const tab of S.tabs) {
    const l = activeLeaf(tab);
    const ls = leaves(tab.root);
    const place = leafPlace(l);
    // 分割したタブは Arc と同じく1行をペインの数で分けて、それぞれのアイコンと名前を出す(押した側のペインへ)。
    // 幅が足りなければ名前を消してアイコンだけにする。7枚以上は 6 枚まで出して残りは +N。細い列では今のペインのアイコンと +N だけ
    const seg = (x) => h('span', {
      class: 'seg' + (x.id === l.id ? ' cur' : ''), title: leafTitle(x) + (leafPlace(x) ? '\n' + leafPlace(x) : ''),
      onclick: (e) => { e.stopPropagation(); tab.activePane = x.id; showTab(tab); },
    }, h('span', { class: 'seg-in' }, leafIconEl(x), h('span', { class: 'label' }, leafTitle(x)), soundBtn([x])));
    const multi = ls.length > 1;
    const shown = ls.length > 7 ? ls.slice(0, 6) : ls;
    if (multi && !shown.includes(l)) shown[shown.length - 1] = l; // 今のペインは必ず出す
    const body = multi ? shown.map(seg) : [leafIconEl(l), h('span', { class: 'label' }, tabTitle(tab)), soundBtn(ls)];
    const el = h('div', {
      class: 'item' + (multi ? ' multi' : '') + (ls.length > shown.length ? ' more' : '') + (tab.id === S.active ? ' active' : '') + (ls.some((x) => x.loading) ? ' loading' : ''),
      title: multi ? '' : tabTitle(tab) + (place && place !== tabTitle(tab) ? '\n' + place : ''),
      onclick: () => showTab(tab),
      onauxclick: (e) => { if (e.button === 1) closeTab(tab); },
      oncontextmenu: (e) => { e.preventDefault(); tabMenu(tab, e.clientX, e.clientY); },
    }, h('span', { class: 'num', title: `Tab ${tab.num} (launcher: ${tab.num}${tab.num <= 9 ? `, Ctrl+${tab.num}` : ''})` }, String(tab.num ?? '')),
    ...body, ls.length > shown.length ? h('span', { class: 'badge hidden-n', title: `${ls.length - shown.length} more` }, '+' + (ls.length - shown.length)) : null,
    multi ? h('span', { class: 'badge cnt' }, '+' + (ls.length - 1)) : null,
    h('button', { class: 'ibtn x', title: 'Close', onclick: (e) => { e.stopPropagation(); closeTab(tab); } }, ico('close')));
    sortable(el, S.tabs, tab, render);
    tl.append(el);
  }
}

// ブックマーク欄(今のスペースの .prelude)。フォルダはスペースの中のサブフォルダで1段だけ。▸ で開閉し、閉じたフォルダは覚えておく。
// 並びは、開いているもの(タブの順)が先で、残りは名前順(space.js の spaceSort)。番号 b1, b2… はこの並びで振る。
// 細い列ではフォルダはアイコン1つで、押すと中身をメニューで出す
const bmIcon = (b) => (b.kind === 'folder' ? 'folder' : b.kind === 'web' ? 'globe' : b.kind === 'app' ? 'window' : b.kind === 'tab' ? 'split-h' : 'folder');
const bmChildren = (f) => S.bookmarks.filter((b) => b.folder === f.id);
const bmOpen = (b) => S.tabs.some((t) => t.bookmarkId === b.id);
function sortBookmarks() {
  S.bookmarks = spaceSort(S.bookmarks, S.tabs.map((t) => t.bookmarkId));
  let n = 0;
  for (const b of S.bookmarks) if (b.kind !== 'folder') b.num = ++n;
}
function renderBookmarks() {
  sortBookmarks();
  renderSpaceButton();
  const bl = $('#bm-list');
  bl.replaceChildren();
  bl.oncontextmenu = (e) => {
    if (e.target !== bl && !e.target.classList.contains('empty')) return;
    e.preventDefault();
    popup([
      { label: 'New folder', icon: 'folder', run: () => newBmFolder() },
      { label: 'Open space folder', icon: 'file', run: openBmDir },
    ], e.clientX, e.clientY);
  };
  bmDropTarget(bl, null);
  if (!S.bookmarks.length) bl.append(h('div', { class: 'empty' }, 'Add with ☆ on a pane'));
  const cur = activeTab()?.bookmarkId;
  for (const b of S.bookmarks) {
    if (b.folder) continue;
    if (b.kind !== 'folder') { bl.append(bmRow(b, cur)); continue; }
    const kids = bmChildren(b);
    const el = h('div', {
      class: 'item bm-folder' + (!b.open && kids.some((k) => k.id === cur) ? ' active' : '') + (kids.some(bmOpen) ? ' open' : ''),
      title: `${b.title} (${kids.length})`,
      onclick: (e) => {
        if (document.body.classList.contains('sb-compact')) {
          const r = e.currentTarget.getBoundingClientRect();
          popup([{ header: b.title }, ...(kids.length ? kids.map((k) => ({ label: k.title || k.target, icon: bmIcon(k), run: () => openBookmark(k) })) : [{ label: '(empty)', disabled: true, run: () => {} }])], r.right + 4, r.top);
          return;
        }
        setFolderOpen(b, !b.open);
      },
      oncontextmenu: (e) => { e.preventDefault(); bmFolderMenu(b, e.clientX, e.clientY); },
    }, h('span', { class: 'chev' }, b.open ? '▾' : '▸'), ico('folder'), h('span', { class: 'label' }, b.title), kids.some(bmOpen) ? h('span', { class: 'badge' }, '●') : null);
    bmDropTarget(el, b);
    bl.append(el);
    if (b.open) for (const k of kids) bl.append(bmRow(k, cur, true));
  }
}
function bmRow(b, cur, inFolder = false) {
  const open = bmOpen(b);
  const el = h('div', {
    class: 'item' + (inFolder ? ' in-folder' : '') + (cur === b.id ? ' active' : '') + (open ? ' open' : ''),
    title: `${b.title || b.target}\n${b.target}`,
    onclick: () => openBookmark(b),
    oncontextmenu: (e) => { e.preventDefault(); bmMenu(b, e.clientX, e.clientY); },
  }, h('span', { class: 'num', title: `Bookmark b${b.num} (launcher: b${b.num})` }, b.num ? 'b' + b.num : ''), ico(bmIcon(b)), h('span', { class: 'label' }, b.title || b.target), open ? h('span', { class: 'badge' }, '●') : null);
  el.draggable = true;
  el.ondragstart = (e) => { sbDrag = { bm: b }; e.dataTransfer.effectAllowed = 'move'; el.classList.add('dragging'); };
  el.ondragend = () => { sbDrag = null; document.querySelectorAll('#sidebar .drop-in, #sidebar .dragging').forEach((z) => z.classList.remove('drop-in', 'dragging')); };
  bmDropTarget(el, inFolder ? S.bookmarks.find((f) => f.id === b.folder) : null);
  return el;
}
// ブックマークのドラッグ。フォルダ(またはその中の行)に落とすとそのフォルダへ、フォルダの外の行・空いた所に落とすとスペースの直下へ動かす
function bmDropTarget(el, folder) {
  const ok = () => {
    const x = sbDrag?.bm;
    return !!x && x.folder !== (folder ? folder.id : '');
  };
  el.ondragover = (e) => {
    if (!ok()) return;
    e.preventDefault();
    e.stopPropagation();
    e.dataTransfer.dropEffect = 'move';
    el.classList.add('drop-in');
  };
  el.ondragleave = () => el.classList.remove('drop-in');
  el.ondrop = (e) => {
    if (!ok()) return;
    e.preventDefault();
    e.stopPropagation();
    el.classList.remove('drop-in');
    moveBm(sbDrag.bm, folder);
  };
}
function bmMenu(b, x, y) {
  const tab = S.tabs.find((t) => t.bookmarkId === b.id);
  popup([
    { label: 'Rename', icon: 'text', run: () => renameBookmark(b) },
    { label: 'Move to folder…', icon: 'folder', run: () => moveToFolderMenu(b, x, y) },
    tab ? { label: 'Overwrite with current layout', icon: 'split-h', run: () => overwriteBmFile(tab) } : null,
    { label: 'Show in folder', icon: 'file', run: () => openTab('files', { path: dirName(b.id) }) },
    { sep: true },
    { label: 'Delete (move to trash)', icon: 'close', danger: true, run: () => deleteBookmark(b) },
  ].filter(Boolean), x, y);
}
function moveToFolderMenu(b, x, y) {
  const folders = S.bookmarks.filter((f) => f.kind === 'folder');
  popup([
    { header: 'Move to folder' },
    ...folders.map((f) => ({ label: f.title, icon: 'folder', disabled: b.folder === f.id, run: () => moveBm(b, f) })),
    folders.length ? { sep: true } : null,
    { label: 'Move into new folder', icon: 'plus', run: async () => { const f = await newBmFolder(); if (f) moveBm(b, f); } },
    b.folder ? { label: 'Remove from folder', icon: 'up', run: () => moveBm(b, null) } : null,
  ].filter(Boolean), x, y);
}
function bmFolderMenu(f, x, y) {
  const kids = bmChildren(f);
  popup([
    { label: f.open ? 'Collapse' : 'Expand', icon: 'folder', run: () => setFolderOpen(f, !f.open) },
    { label: `Open all (${kids.length})`, icon: 'plus', disabled: !kids.length, run: async () => { for (const k of kids) await openBookmark(k); } },
    { label: 'Rename', icon: 'text', run: () => renameBookmark(f) },
    { label: 'Show in folder', icon: 'file', run: () => openTab('files', { path: f.id }) },
    { sep: true },
    // 中のブックマークは消さずにスペースの直下へ出してから、フォルダをゴミ箱へ
    { label: 'Delete folder (keep contents)', icon: 'close', danger: true, run: () => deleteBmFolder(f) },
  ], x, y);
}

// ---- 番号(名札)。タブは開いた時に空いている一番小さい番号(全部の画面の窓で重ならない。main.js が振る)で、並べ替えても変わらない。
// ブックマークは b1, b2… で、サイドバーの並び順(sortBookmarks)。ランチャーで「3」「b3」、Ctrl+1〜9、prelude.sh tab 3 で呼ぶ
const reportNums = () => api.tab.nums(S.tabs.map((t) => t.num).filter(Boolean));
async function renumberTab(tab) {
  const v = await askText('Tab number', String(tab.num));
  const n = parseInt(v, 10);
  if (!(n > 0) || n === tab.num) return;
  const other = S.tabs.find((t) => t.num === n);
  if (other) other.num = tab.num; // この窓のタブが使っていれば入れ替える
  else if (api.tab.where(n)) { toast(`Number ${n} is used by a tab on ${api.tab.where(n).label}`); return; }
  else api.tab.allocNum(n);
  tab.num = n;
  render();
}
function gotoTabNum(n) {
  const tab = S.tabs.find((t) => t.num === n);
  if (tab) showTab(tab); else api.tab.goto(String(n)); // 別の画面の窓にあれば、その窓が前に出て切り替える
}
api.tab.onGoto(({ bookmark, num, missing }) => {
  if (missing) { toast(`No tab ${num}`); return; }
  if (bookmark) {
    const b = S.bookmarks.find((x) => x.num === num && x.kind !== 'folder');
    if (b) openBookmark(b); else toast(`No bookmark b${num}`);
    return;
  }
  const tab = S.tabs.find((t) => t.num === num);
  if (tab) showTab(tab);
});

// タブの右クリックメニュー
function tabMenu(tab, x, y) {
  const l = activeLeaf(tab);
  const ls = leaves(tab.root);
  const i = S.tabs.indexOf(tab);
  const others = S.screens.filter((s) => s.key !== WIN_KEY);
  const web = ls.filter((p) => p.type === 'web');
  const muted = web.length && web.every((p) => p.muted);
  popup([
    isDevLeaf(l) ? { label: `Restart ${DEV_NAME}`, icon: 'reload', run: () => restartDevPrelude(l) } : null,
    ['web', 'files', 'field'].includes(l.type) ? { label: 'Reload', icon: 'reload', run: () => (l.type === 'web' ? webCmd(l, 'reload') : l.type === 'files' ? reloadDir(l) : fieldReload(l)) } : null,
    web.length ? { label: muted ? 'Unmute' : 'Mute', icon: muted ? 'volume' : 'volume-off', run: () => setMuted(ls, !muted) } : null,
    { label: 'Duplicate', icon: 'plus', run: () => duplicateTab(tab) },
    { label: `Change number… (now ${tab.num})`, icon: 'text', run: () => renumberTab(tab) },
    ['web', 'files', 'app'].includes(l.type) && leafPlace(l) && l.mode !== 'save' ? { label: 'Add bookmark', icon: 'bookmark', run: () => addBookmark(l, tab) } : null,
    ls.length > 1 ? { label: 'Bookmark whole layout', icon: 'split-h', run: () => addLayoutBookmark(tab) } : null,
    tab.bookmarkId || tab.bmFile ? { label: `Overwrite ${tab.bookmarkId ? 'bookmark' : 'file'} (${bmFileShort(tab.bookmarkId || tab.bmFile)}) with current layout`, icon: 'file', run: () => overwriteBmFile(tab) } : null,
    others.length ? { sep: true } : null,
    ...others.map((s) => ({ label: `Move to ${s.label}`, icon: 'window', run: () => moveTab(tab, s.key) })),
    { sep: true },
    { label: 'Reopen closed tab', icon: 'back', key: C.shortcuts.reopenTab, disabled: !S.closed.length, run: reopenClosedTab },
    { label: 'Close tabs below', disabled: i === S.tabs.length - 1, run: () => closeTabs(S.tabs.slice(i + 1)) },
    { label: 'Close other tabs', disabled: S.tabs.length < 2, run: () => closeTabs(S.tabs.filter((t) => t !== tab)) },
    { label: 'Close', icon: 'close', danger: true, key: C.shortcuts.closeTab, run: () => closeTab(tab) },
  ].filter(Boolean), x, y);
}

// ------------------------------------------------------------------ tabs / panes
// タブを前面に出す。アプリが終了して空になっているアプリペインは、同じアプリをもう一度起動して全面に出す
function showTab(tab) {
  S.active = tab.id;
  render();
  for (const l of leaves(tab.root)) if (l.type === 'app' && !l.running && l.dormant) { l.dormant = false; startApp(l, l.cmd, l.cls, l.name); }
}

// o.background: 今のタブのまま裏で開く(中クリック・Ctrl+クリックのリンク)。位置は設定「新しいタブの位置」(既定は Arc と同じ一番上)
function openTab(kind, o = {}) {
  if (kind === 'app' && !S.appSupported) kind = 'files';
  const leaf = makeLeaf(kind, o);
  const tab = { id: nid('t'), num: api.tab.allocNum(0), root: leaf, activePane: leaf.id, bookmarkId: o.bookmarkId || null };
  const pos = C.tabs?.newTabPosition;
  const cur = S.tabs.findIndex((t) => t.id === S.active);
  if (pos === 'top') S.tabs.unshift(tab);
  else if (pos === 'next' && cur >= 0) S.tabs.splice(cur + 1, 0, tab);
  else S.tabs.push(tab);
  if (!o.background || !S.tabs.some((t) => t.id === S.active)) S.active = tab.id;
  render();
  return tab;
}

function destroyLeaf(l) {
  if (l.type === 'web' && l.created) api.web.destroy(l.id);
  if (l.type === 'app' && l.running) api.app.close(l.id);
  if (l.mode === 'save' && l.dlId && !l.dlHandled) { l.dlHandled = true; api.dl.cancel(l.dlId); }
  if (l.type === 'files') api.fs.unwatch(l.id);
  if (l.type === 'field') fieldUnwatchAll(l.fv);
}

function closeTab(tab) {
  rememberClosed(tab);
  leaves(tab.root).forEach(destroyLeaf);
  const i = S.tabs.indexOf(tab);
  S.tabs.splice(i, 1);
  if (!S.tabs.length) { openTab(C.behavior.startKind); return; }
  if (S.active === tab.id) S.active = S.tabs[Math.min(i, S.tabs.length - 1)].id;
  render();
}

// まとめて閉じる(下のタブ・ほかのタブ)。今のタブが閉じたら、残ったうちの一番近いものへ
function closeTabs(list) {
  if (!list.length) return;
  for (const tab of list) {
    rememberClosed(tab);
    leaves(tab.root).forEach(destroyLeaf);
    S.tabs.splice(S.tabs.indexOf(tab), 1);
  }
  if (!S.tabs.length) { openTab(C.behavior.startKind); return; }
  if (!S.tabs.some((t) => t.id === S.active)) S.active = S.tabs[S.tabs.length - 1].id;
  render();
}

// 閉じたタブ(新しい順に最大 20)。中身は保存と同じ形(serNode)で持ち、開き直すと元の位置に戻す
S.closed = [];
function rememberClosed(tab) {
  const ls = leaves(tab.root);
  if (ls.length === 1 && ls[0].type === 'home') return; // 何もしていないホームだけのタブは覚えない
  S.closed.unshift({ num: tab.num, bookmarkId: tab.bookmarkId, root: serNode(tab.root), activeIndex: ls.findIndex((l) => l.id === tab.activePane), index: S.tabs.indexOf(tab) });
  S.closed.length = Math.min(S.closed.length, 20);
}
function reopenClosedTab() {
  const c = S.closed.shift();
  if (!c) { toast('No tab to reopen'); return; }
  // ブックマークのタブがもう開いていれば、ブックマークとのつながりは外す(1ブックマーク = 最大1タブ)
  const bookmarkId = c.bookmarkId && !S.tabs.some((t) => t.bookmarkId === c.bookmarkId) && S.bookmarks.some((b) => b.id === c.bookmarkId) ? c.bookmarkId : null;
  insertTab(c.root, { bookmarkId, num: c.num, activeIndex: c.activeIndex, index: c.index }); // 番号が空いていれば元の番号
}
// タブの複製。分割もそのまま写す(ブックマークとのつながりは写さない)
const duplicateTab = (tab) => insertTab(serNode(tab.root), { activeIndex: leaves(tab.root).findIndex((l) => l.id === tab.activePane), index: S.tabs.indexOf(tab) + 1 });
function insertTab(data, o) {
  const root = deserNode(data);
  const ls = leaves(root);
  const tab = { id: nid('t'), num: api.tab.allocNum(o.num || 0), root, activePane: (ls[o.activeIndex] || ls[0]).id, bookmarkId: o.bookmarkId || null, bmFile: o.bmFile || null };
  S.tabs.splice(clamp(o.index ?? 0, 0, S.tabs.length), 0, tab);
  S.active = tab.id;
  render();
  if (S.appSupported) for (const l of ls) if (l.type === 'app' && (l.cmd || l.webUrl)) startApp(l, l.cmd, l.cls, l.name);
  return tab;
}

// タブを別の画面の窓へ移す。ペインは壊さずに渡す(Webページ・取り込んだアプリは main.js が移った先の窓へ付け替える)。
// o.quiet: 残りのタブが無くなっても新しいタブを開かない(画面を外して全部移すとき)
const LIVE_ONLY = new Set(['el', 'ui', 'slot', 'vl', 'apps', 'entries', 'view', 'sel', 'ta', 'fv']); // 画面の部品など、移った先で作り直すもの
function moveTab(tab, to, o = {}) {
  const data = JSON.parse(JSON.stringify({ num: tab.num, bookmarkId: tab.bookmarkId, bmFile: tab.bmFile, activePane: tab.activePane, root: tab.root }, (k, v) => (LIVE_ONLY.has(k) ? undefined : v)));
  const i = S.tabs.indexOf(tab);
  S.tabs.splice(i, 1);
  if (S.active === tab.id) S.active = S.tabs[Math.min(i, S.tabs.length - 1)]?.id ?? null;
  api.tab.move(to, data);
  if (o.quiet) return;
  if (!S.tabs.length) openTab(C.behavior.startKind); else render();
}

// 別の窓から移ってきたタブ。ペインは id も状態もそのまま(Webビューは作り直さず、アプリも起動し直さない)
function adoptTab(data) {
  const ids = new Set(leaves(data.root).map((l) => l.id));
  const tab = { id: nid('t'), num: api.tab.allocNum(data.num || 0), root: data.root, activePane: ids.has(data.activePane) ? data.activePane : leaves(data.root)[0].id, bookmarkId: data.bookmarkId || null, bmFile: data.bmFile || null };
  // フォルダペインは一覧を持ってこない(大きい)ので、同じフォルダを読み直す。並べ方と戻る/進むの履歴は引き継ぐ
  for (const l of leaves(tab.root)) if (l.type === 'files') Object.assign(l, newFilesState(l), { hist: l.hist || { back: [], fwd: [] }, loaded: false, paintQ: false });
  S.tabs.unshift(tab);
  S.active = tab.id;
  render();
}

function splitLeaf(tab, leaf, dir, kind, o = {}) {
  if ((kind === 'files' || kind === 'field') && o.path == null) o.path = (leaf.type === 'files' && leaf.mode !== 'save') || leaf.type === 'field' ? leaf.path : C.behavior.startPath;
  const nl = makeLeaf(kind, o);
  replaceNode(tab, leaf, { k: 'split', dir, ratio: 0.5, a: leaf, b: nl });
  tab.activePane = nl.id;
  S.active = tab.id;
  render();
  return nl;
}

function closeLeaf(tab, leaf) {
  destroyLeaf(leaf);
  if (tab.root === leaf) { closeTab(tab); return; }
  const p = findParent(tab.root, leaf);
  const sib = p.a === leaf ? p.b : p.a;
  replaceNode(tab, p, sib);
  if (tab.activePane === leaf.id) tab.activePane = leaves(sib)[0].id;
  render();
}

function setActivePane(tab, leaf) {
  if (tab.activePane === leaf.id) return;
  tab.activePane = leaf.id;
  document.querySelectorAll('.pane').forEach((p) => p.classList.toggle('active', p.dataset.id === leaf.id));
}

// ------------------------------------------------------------------ spaces
// スペースは「スペースのフォルダ」(設定。既定は ~/スペース)の下のフォルダ1つ(space.js)。中の .prelude がそのスペースのブックマーク
// (S.bookmarks の id はファイルのフルパス)、サブフォルダがブックマークのフォルダ(id はフォルダのフルパス)。
// タブはスペースごとに持つ。切り替えると今のタブは全部閉じて(Web もアプリも終わる)、切り替え先で前に開いていたタブを開き直す。
// ブックマークから開いたタブはそのファイルを tab.bookmarkId に覚えていて、同じブックマークをもう一度開くとそのタブへ移る(1ファイル = 最大1タブ)
S.space = '';
S.spaceDir = '';
S.spaces = [];
S.wsSpaces = {}; // 今のスペース以外のタブ { 名前: { active, tabs } }
S.closedBy = {}; // スペースごとの閉じたタブ
const dirName = (p) => String(p).replace(/\/[^/]*$/, '') || '/';
async function spaceRoot() {
  const home = await api.fs.home();
  const d = String(C.behavior.spaceDir || '').trim().replace(/\/+$/, '');
  return !d ? home + '/スペース' : d === '~' || d.startsWith('~/') ? home + d.slice(1) : d;
}
async function loadSpaceList() {
  const r = await api.fs.spaceList(await spaceRoot(), SPACE_DEFAULT);
  if (r.error) toast(`Cannot read the space folder (${r.error})`);
  S.spaces = (r.names || []).sort((a, b) => a.localeCompare(b, 'ja', { numeric: true }));
  if (!S.spaces.length) S.spaces = [SPACE_DEFAULT];
}

// 今のスペースのフォルダを読み直して S.bookmarks を作る。消えたファイルに結びついていたタブは、ふつうのタブにする
let scanSeq = 0;
async function scanSpace() {
  const seq = ++scanSeq;
  const dir = S.spaceDir;
  const [r, home] = await Promise.all([api.fs.spaceRead(dir), api.fs.home()]);
  if (seq !== scanSeq || dir !== S.spaceDir) return;
  if (r.error) { toast(`Cannot read space "${S.space}" (${r.error})`); return; }
  const closed = folderClosed();
  const list = r.folders.map((n) => ({ id: dir + '/' + n, kind: 'folder', title: n, open: !closed.has(dir + '/' + n), folder: '' }));
  for (const f of r.files) {
    let d = null;
    try { if (!f.error) d = bmFileParse(f.text, home); } catch {}
    if (!d) continue; // 読めない・形の違うファイルは出さない(フォルダから開くと理由が出る)
    const title = bmFileShort(f.path);
    const b = { id: f.path, folder: f.folder ? dir + '/' + f.folder : '', ...bmFileToBookmark(title, d.root), root: d.root };
    if (b.kind === 'tab') b.target = layoutLabel(d.root);
    list.push(b);
  }
  S.bookmarks = list;
  const ids = new Set(list.map((b) => b.id));
  const gone = S.tabs.filter((t) => t.bookmarkId && !ids.has(t.bookmarkId));
  for (const t of gone) t.bookmarkId = null;
  renderSidebar();
  if (gone.length) scheduleSave();
}
// フォルダの中が変わったら読み直す(サブフォルダも見る)
let spaceWatched = [];
function watchSpace() {
  for (const d of spaceWatched) api.fs.unwatch('space:' + d);
  spaceWatched = [S.spaceDir, ...S.bookmarks.filter((b) => b.kind === 'folder').map((f) => f.id)];
  for (const d of spaceWatched) api.fs.watch('space:' + d, d);
}
let spaceScanTimer = 0;
function spaceOnChanged() {
  clearTimeout(spaceScanTimer);
  spaceScanTimer = setTimeout(async () => { await scanSpace(); watchSpace(); }, 150);
}

// 閉じてあるフォルダ(フルパス)。この PC の画面の中だけで覚える
function folderClosed() {
  try { return new Set(JSON.parse(localStorage.getItem('bmClosed') || '[]')); } catch { return new Set(); }
}
function setFolderOpen(f, open) {
  const s = folderClosed();
  if (open) s.delete(f.id); else s.add(f.id);
  try { localStorage.setItem('bmClosed', JSON.stringify([...s])); } catch {}
  f.open = open;
  renderSidebar();
}

// スペースに入る(起動時・切り替え)。保存してあったタブを開き直す
async function enterSpace(name) {
  S.space = name;
  S.spaceDir = (await spaceRoot()) + '/' + name;
  S.closed = S.closedBy[name] || [];
  S.bookmarks = [];
  await scanSpace();
  watchSpace();
  const saved = S.wsSpaces[name];
  delete S.wsSpaces[name];
  if (saved) restore(saved);
  if (!S.tabs.length) openTab(C.behavior.startKind);
  else render();
  if (S.appSupported) for (const t of S.tabs) for (const l of leaves(t.root)) if (l.type === 'app' && (l.cmd || l.webUrl)) startApp(l, l.cmd, l.cls, l.name);
}
let switching = false;
async function switchSpace(name) {
  if (switching || name === S.space) return;
  switching = true;
  try {
    clearTimeout(saveTimer);
    S.wsSpaces[S.space] = tabsData();
    S.closedBy[S.space] = S.closed;
    for (const t of S.tabs) leaves(t.root).forEach(destroyLeaf);
    S.tabs.length = 0;
    S.active = null;
    reportNums(); // 閉じたタブの番号を空けて、切り替え先のタブが 1 から使えるようにする
    await enterSpace(name);
    toast(`Space: ${name}`);
  } finally { switching = false; }
}
async function spaceMenu(x, y) {
  await loadSpaceList();
  popup([
    { header: 'Spaces' },
    ...S.spaces.map((n) => ({ label: n, checked: n === S.space, run: () => switchSpace(n) })),
    { sep: true },
    { label: 'New space…', icon: 'plus', run: newSpace },
    { label: `Rename "${S.space}"…`, icon: 'text', run: renameSpace },
    { label: 'Open space folder', icon: 'file', run: openBmDir },
  ], x, y);
}
function renderSpaceButton() {
  const b = $('#space-btn');
  if (!b) return;
  b.replaceChildren(h('span', { class: 'label' }, S.space || ''), h('span', { class: 'chev' }, '▾'));
  b.title = `Space: ${S.space} (click to switch)`;
}
async function newSpace() {
  const name = await askText('Space name', '');
  if (name == null || !name.trim()) return;
  const r = await api.fs.mkdir(await spaceRoot(), name.trim());
  if (!r.ok) { toast(`Cannot create (${r.error})`); return; }
  await loadSpaceList();
  switchSpace(r.name);
}
async function renameSpace() {
  const name = await askText('Space name', S.space);
  if (name == null || !name.trim() || name.trim() === S.space) return;
  const old = S.spaceDir;
  const r = await api.fs.rename(old, name.trim());
  if (!r.ok) { toast(`Cannot rename (${r.error})`); return; }
  S.closedBy[r.name] = S.closed;
  delete S.closedBy[S.space];
  S.space = r.name;
  S.spaceDir = dirName(old) + '/' + r.name;
  relinkTabs(old, S.spaceDir, true);
  await loadSpaceList();
  await scanSpace();
  watchSpace();
}
// ブックマークのファイル・フォルダが動いたので、結びついているタブ(と閉じたタブ)のパスを付け替える。prefix: フォルダごと動いた
function relinkTabs(from, to, prefix = false) {
  const map = (id) => (id === from ? to : prefix && id && id.startsWith(from + '/') ? to + id.slice(from.length) : id);
  for (const t of [...S.tabs, ...S.closed]) t.bookmarkId = map(t.bookmarkId);
  scheduleSave();
}

// ------------------------------------------------------------------ bookmarks
// 種類: web / files / app / tab(分割ごとの配置)。中身は bmfile.js の配置(root)で、開くとこの配置の新しいタブになる。folder はフォルダの id
// 分割ごとのブックマークの説明(中のペインの名前を | でつなぐ)
const layoutLabel = (root) => leaves(root).map((l) => (l.type === 'web' ? hostOf(l.url || '') : l.type === 'app' ? l.name : l.type === 'files' || l.type === 'field' ? baseName(l.path || '') : l.type === 'home' ? 'Home' : 'Settings') || l.type).join(' | ');
// 今のスペースに .prelude を作る。できたファイルのパス(作れなければ null)
async function saveBmFile(title, root, dir = S.spaceDir) {
  const r = await api.fs.bmSave(dir, bmFileName(title), bmFileText(title, root, await api.fs.home()));
  if (r.error) { toast(`Cannot save (${r.error})`); return null; }
  await scanSpace();
  return r.path;
}
// 今のタブの配置(分割・各ペインの中身)をそのままブックマークにする。開くと同じ配置で開く
async function addLayoutBookmark(tab) {
  const p = await saveBmFile(leaves(tab.root).map(leafTitle).join(' | '), serNode(tab.root));
  if (!p) return;
  if (!tab.bookmarkId) { tab.bookmarkId = p; tab.bmFile = null; renderSidebar(); scheduleSave(); }
  toast('Bookmarked whole layout');
}
// ペイン1つのブックマーク。そのペインだけのタブなら、タブもブックマークに結びつける
async function addBookmark(leaf, tab) {
  const kind = leaf.type === 'app' ? 'app' : leaf.type === 'web' ? 'web' : 'files';
  const target = kind === 'app' ? leaf.webUrl || leaf.cmd : kind === 'web' ? leaf.url : leaf.path;
  if (!target) return;
  if (S.bookmarks.some((b) => b.kind === kind && b.target === target)) { toast('Already bookmarked'); return; }
  const title = (kind === 'app' ? leaf.name : kind === 'web' ? leaf.title || hostOf(target) : baseName(target)) || target;
  const p = await saveBmFile(title, serNode(leaf));
  if (!p) return;
  if (tab && tab.root === leaf && !tab.bookmarkId) { tab.bookmarkId = p; tab.bmFile = null; renderSidebar(); scheduleSave(); }
  toast('Bookmark added');
}

// 新しいタブを置く位置(設定「新しいタブの位置」。既定は Arc と同じ一番上)
function newTabIndex() {
  const pos = C.tabs?.newTabPosition;
  const cur = S.tabs.findIndex((t) => t.id === S.active);
  return pos === 'top' ? 0 : pos === 'next' && cur >= 0 ? cur + 1 : S.tabs.length;
}
async function openBookmark(b) {
  const tab = S.tabs.find((t) => t.bookmarkId === b.id);
  if (tab) { showTab(tab); return; } // 1ブックマーク = 最大1タブ
  if (b.kind === 'folder') return;
  const root = await checkBmCommands(b.root, b.title);
  if (!root || S.tabs.some((t) => t.bookmarkId === b.id)) return;
  insertTab(root, { bookmarkId: b.id, index: newTabIndex() });
}

async function renameBookmark(b) {
  const name = await askText(b.kind === 'folder' ? 'Folder name' : 'Bookmark name', b.title);
  if (name == null || !name.trim() || name.trim() === b.title) return;
  const r = await api.fs.rename(b.id, b.kind === 'folder' ? name.trim() : bmFileName(name.trim()));
  if (!r.ok) { toast(`Cannot rename (${r.error})`); return; }
  relinkTabs(b.id, dirName(b.id) + '/' + r.name, b.kind === 'folder');
  await scanSpace();
  if (b.kind === 'folder') watchSpace();
}
// folder: 動かす先のフォルダ(null ならスペースの直下)
async function moveBm(b, folder) {
  const r = await api.fs.transfer('move', [b.id], folder ? folder.id : S.spaceDir);
  if (!r.ok) { toast(`Cannot move (${r.errors.join(', ')})`); return; }
  relinkTabs(b.id, (folder ? folder.id : S.spaceDir) + '/' + r.names[0]);
  if (folder && !folder.open) setFolderOpen(folder, true);
  await scanSpace();
}
async function deleteBookmark(b) {
  const r = await api.fs.remove([b.id], false);
  if (!r.ok) { toast(`Cannot delete (${r.errors.join(', ')})`); return; }
  await scanSpace();
  toast(`Moved to trash: ${b.title}`);
}
async function newBmFolder() {
  const name = await askText('Folder name', 'New folder');
  if (name == null || !name.trim()) return null;
  const r = await api.fs.mkdir(S.spaceDir, name.trim());
  if (!r.ok) { toast(`Cannot create (${r.error})`); return null; }
  await scanSpace();
  watchSpace();
  return S.bookmarks.find((f) => f.id === S.spaceDir + '/' + r.name) || null;
}
async function deleteBmFolder(f) {
  for (const k of bmChildren(f)) {
    const r = await api.fs.transfer('move', [k.id], S.spaceDir);
    if (!r.ok) { toast(`Cannot move (${r.errors.join(', ')})`); await scanSpace(); return; }
    relinkTabs(k.id, S.spaceDir + '/' + r.names[0]);
  }
  const r = await api.fs.remove([f.id], false);
  if (!r.ok) toast(`Cannot delete (${r.errors.join(', ')})`);
  await scanSpace();
  watchSpace();
}

// ------------------------------------------------------------------ bookmark files
// .prelude はスペースの外(人からもらったファイルなど)にもあって、フォルダ・フィールドから開ける(開くと同じ配置の新しいタブ)。
// スペースの外のファイルから開いたタブはそのファイルを覚えていて(tab.bmFile)、同じファイルをもう一度開くとそのタブへ移る。
const bmFileShort = (p) => baseName(p).replace(/\.prelude$/i, '');
// タブが結びついているファイル(ブックマークか、スペースの外のファイル)を今の配置で書き直す(タイトルはファイルのものを残す)
async function overwriteBmFile(tab) {
  const file = tab.bookmarkId || tab.bmFile;
  if (!file) return;
  const home = await api.fs.home();
  const r = await api.fs.bmRead(file);
  let title = bmFileShort(file);
  try { title = bmFileParse(r.text, home).title || title; } catch {}
  const w = await api.fs.bmWrite(file, bmFileText(title, serNode(tab.root), home));
  toast(w.error ? `Cannot save (${w.error})` : 'Overwritten with current layout');
  if (!w.error && tab.bookmarkId) scanSpace();
}
// ファイルを読んで、起動コマンドを確かめた配置 { title, root } にする。開かない時は null
async function readBmFile(full) {
  const r = await api.fs.bmRead(full);
  let d = null;
  try {
    if (r.error) throw new Error(r.error);
    d = bmFileParse(r.text, await api.fs.home());
  } catch (e) { toast(`Cannot open: ${baseName(full)} (${e.message})`); return null; }
  const root = await checkBmCommands(d.root, baseName(full));
  return root && { title: d.title || bmFileShort(full), root };
}
// ファイルは人からもらうこともあるので、インストール済みのアプリ(.desktop の Exec)と同じでない起動コマンドは、黙って起動しない。
// 既定は「起動しないで開く」(そのペインは「アプリを取り込む」の画面になる)
async function checkBmCommands(root, name) {
  const cmds = bmFileCommands(root);
  if (!cmds.length) return root;
  const known = new Set((await api.app.list().catch(() => [])).map((a) => a.exec));
  const unknown = cmds.filter((c) => !known.has(c));
  if (!unknown.length) return root;
  const body = h('div', {},
    h('div', {}, `"${name}" runs a command that does not match any installed app. Unless you made this file yourself, open it without running.`),
    h('div', { class: 'cmds' }, unknown.map((c) => h('code', {}, c))));
  const v = await fileDialog('Unknown command', body, [
    { label: 'Cancel', value: null },
    { label: 'Run', value: 'run', danger: true },
    { label: 'Open without running', value: 'strip', primary: true },
  ]);
  return v === 'run' ? root : v === 'strip' ? bmFileStripCommands(root, unknown) : null;
}
async function openBmFile(full) {
  const b = S.bookmarks.find((x) => x.id === full);
  if (b) { openBookmark(b); return; } // 今のスペースのブックマーク
  const open = S.tabs.find((t) => t.bmFile === full);
  if (open) { showTab(open); return; } // 1ファイル = 最大1タブ
  const d = await readBmFile(full);
  if (d) insertTab(d.root, { bmFile: full, index: newTabIndex() });
}
// ファイルを今のスペースに写す(元のファイルは残す)
async function importBmFile(full) {
  if (dirName(full) === S.spaceDir || dirName(dirName(full)) === S.spaceDir) { toast('Already in this space'); return; }
  const r = await api.fs.bmRead(full);
  if (r.error) { toast(`Cannot open: ${baseName(full)} (${r.error})`); return; }
  try { bmFileParse(r.text, await api.fs.home()); } catch (e) { toast(`Cannot open: ${baseName(full)} (${e.message})`); return; }
  const w = await api.fs.bmSave(S.spaceDir, baseName(full), r.text);
  if (w.error) { toast(`Cannot save (${w.error})`); return; }
  await scanSpace();
  toast(`Added to space "${S.space}"`);
}
function openBmDir() { openTab('files', { path: S.spaceDir }); }

// 0.10.0 までのサイドバーのブックマーク(設定の bookmarks)を、既定のスペースのファイルにする(主画面の窓で1回だけ)。
// 元のブックマークは設定の bookmarksBackup に残す。戻り値: 古い id → ファイルのパス
async function migrateBookmarks() {
  const old = C.bookmarks || [];
  if (IS_SUB || !old.length) return {};
  const m = spaceMigrate(old, bmFileRoot);
  if (!m.items.length) return {};
  const dir = (await spaceRoot()) + '/' + SPACE_DEFAULT;
  const home = await api.fs.home();
  const safe = (n) => bmFileName(n).replace(/\.prelude$/, '');
  const map = {};
  for (const it of m.items) {
    const r = await api.fs.bmSave(it.folder ? dir + '/' + safe(it.folder) : dir, bmFileName(it.title), bmFileText(it.title, it.root, home));
    if (r.error) { toast(`Cannot move bookmarks to space (${r.error})`, 8000); return {}; }
    map[it.id] = r.path;
  }
  await api.config.set('bookmarksMigrated', { ...(C.bookmarksMigrated || {}), ...map });
  await api.config.set('bookmarksBackup', old);
  await api.config.set('bookmarks', m.rest);
  toast(`Moved ${m.items.length} bookmarks to space "${SPACE_DEFAULT}"`, 6000);
  return map;
}

// ------------------------------------------------------------------ popups / modal / toast
let susp = 0;
function suspend(d) {
  const was = susp > 0;
  susp = Math.max(0, susp + d);
  if (was !== susp > 0) api.web.suspend(susp > 0); // ネイティブのWebビューはHTMLの上に出るので、メニュー中は隠す
}
const overlay = $('#overlay');
let pop = null;
function closePopup() {
  if (!pop) return;
  pop.bd.remove();
  pop.m.remove();
  const prev = pop.prev;
  pop = null;
  suspend(-1);
  if (prev?.isConnected) prev.focus({ preventScroll: true }); // メニューを開く前の入力先へ戻す
}
// items: { label, icon, run, key(右に出すショートカット), checked, disabled, danger } / { header } / { sep }
// ↑↓ で選んで Enter(opt.keyboard なら最初の項目を選んだ状態で開く)
function popup(items, x, y, opt = {}) {
  closePopup();
  const prev = document.activeElement;
  const bd = h('div', { class: 'backdrop', onpointerdown: closePopup, oncontextmenu: (e) => { e.preventDefault(); closePopup(); } });
  const choose = (it) => { if (it.disabled) return; closePopup(); it.run(); };
  const lead = items.some((it) => it.icon || it.checked != null); // アイコンのある項目があれば、ない項目も字下げを揃える
  const m = h('div', { class: 'menu', tabindex: '-1' }, items.map((it) => (it.header ? h('div', { class: 'mh' }, it.header)
    : it.sep ? h('div', { class: 'sep' })
    : h('div', {
      class: 'mi' + (it.danger ? ' danger' : '') + (it.disabled ? ' dis' : ''), style: it.danger ? { color: 'var(--danger)' } : null,
      onclick: () => choose(it), onpointermove: (e) => setAct(e.currentTarget),
    }, it.icon ? ico(it.icon) : it.checked != null ? h('span', { class: 'ico chk' }, it.checked ? '✓' : '') : lead ? h('span', { class: 'ico' }) : null, h('span', { class: 'ml' }, it.label), it.key ? h('span', { class: 'mk' }, it.key) : null))));
  const mis = [...m.querySelectorAll('.mi')];
  const live = items.filter((it) => !it.header && !it.sep);
  const setAct = (el) => mis.forEach((x) => x.classList.toggle('act', x === el));
  m.addEventListener('keydown', (e) => {
    const ok = mis.filter((x) => !x.classList.contains('dis'));
    const i = ok.findIndex((x) => x.classList.contains('act'));
    if (e.key === 'ArrowDown' || (e.key === 'Tab' && !e.shiftKey)) setAct(ok[(i + 1) % ok.length]);
    else if (e.key === 'ArrowUp' || (e.key === 'Tab' && e.shiftKey)) setAct(ok[(i - 1 + ok.length) % ok.length]);
    else if (e.key === 'Home') setAct(ok[0]);
    else if (e.key === 'End') setAct(ok[ok.length - 1]);
    else if ((e.key === 'Enter' || e.key === ' ') && i >= 0) choose(live[mis.indexOf(ok[i])]);
    else if (e.key === 'Escape' || e.key === 'ContextMenu') closePopup();
    else return;
    e.preventDefault();
    e.stopPropagation();
  });
  overlay.append(bd, m);
  m.style.left = clamp(x, 4, innerWidth - m.offsetWidth - 4) + 'px';
  m.style.top = clamp(y, 4, innerHeight - m.offsetHeight - (opt.bottom || 4)) + 'px';
  pop = { bd, m, prev };
  suspend(+1);
  m.focus({ preventScroll: true });
  if (opt.keyboard) setAct(mis.find((x) => !x.classList.contains('dis')));
}
const popupAt = (el, items) => { const r = el.getBoundingClientRect(); popup(items, r.left, r.bottom + 2); };

function askText(title, init = '') {
  return new Promise((resolve) => {
    const bd = h('div', { class: 'backdrop modal-bg' });
    const input = h('input', { type: 'text', value: init, spellcheck: 'false' });
    const done = (v) => { bd.remove(); box.remove(); suspend(-1); resolve(v); };
    const box = h('div', { class: 'modal' }, h('div', {}, title), input,
      h('div', { class: 'row' }, h('button', { class: 'textbtn', onclick: () => done(null) }, 'Cancel'), h('button', { class: 'textbtn primary', onclick: () => done(input.value) }, 'OK')));
    input.addEventListener('keydown', (e) => { if (e.key === 'Enter') done(input.value); if (e.key === 'Escape') done(null); });
    overlay.append(bd, box);
    suspend(+1);
    input.focus();
    input.select();
  });
}

let toastTimer = 0;
function toast(msg, ms = 2600) {
  const t = $('#toast');
  t.textContent = msg;
  t.style.display = 'block';
  clearTimeout(toastTimer);
  toastTimer = setTimeout(() => { t.style.display = 'none'; }, ms);
}

// ------------------------------------------------------------------ workspace rendering
let syncQueued = false;
function syncViews() {
  if (syncQueued) return;
  syncQueued = true;
  requestAnimationFrame(() => {
    syncQueued = false;
    const at = activeTab();
    for (const tab of S.tabs) {
      for (const l of leaves(tab.root)) {
        const isWeb = l.type === 'web' && l.created;
        if (!isWeb && !(l.type === 'app' && l.running)) continue;
        const send = isWeb ? api.web.bounds : api.app.bounds;
        if (tab === at && l.slot && l.slot.isConnected) {
          const r = l.slot.getBoundingClientRect();
          send(l.id, { x: r.left, y: r.top, width: r.width, height: r.height });
        } else send(l.id, null);
      }
    }
  });
}

function render() {
  renderSidebar();
  reportNums();
  const ws = $('#workspace');
  ws.replaceChildren();
  const tab = activeTab();
  if (tab) ws.append(buildNode(tab, tab.root));
  syncViews();
  scheduleSave();
}

function buildNode(tab, node) {
  if (node.k === 'leaf') return buildLeaf(tab, node);
  const a = buildNode(tab, node.a);
  const b = buildNode(tab, node.b);
  a.style.flex = `${node.ratio} 1 0`;
  b.style.flex = `${1 - node.ratio} 1 0`;
  const el = h('div', { class: 'split ' + node.dir });
  const div = h('div', { class: 'divider ' + node.dir, onpointerdown: (e) => dragDivider(e, node, el, a, b) });
  el.append(a, div, b);
  return el;
}

function dragDivider(e, node, el, a, b) {
  e.preventDefault();
  try { e.currentTarget.setPointerCapture(e.pointerId); } catch {}
  suspend(+1); // ドラッグ中はWebビューを隠して、マウスがシェルに届くようにする
  const row = node.dir === 'row';
  const move = (ev) => {
    const r = el.getBoundingClientRect();
    node.ratio = clamp(((row ? ev.clientX - r.left : ev.clientY - r.top)) / (row ? r.width : r.height), 0.1, 0.9);
    a.style.flex = `${node.ratio} 1 0`;
    b.style.flex = `${1 - node.ratio} 1 0`;
  };
  let finished = false;
  const up = () => {
    if (finished) return;
    finished = true;
    window.removeEventListener('pointermove', move);
    window.removeEventListener('pointerup', up);
    window.removeEventListener('pointercancel', up);
    window.removeEventListener('blur', up);
    syncViews();
    suspend(-1);
    scheduleSave();
  };
  window.addEventListener('pointermove', move);
  window.addEventListener('pointerup', up);
  window.addEventListener('pointercancel', up);
  window.addEventListener('blur', up);
}

function buildLeaf(tab, leaf) {
  const head = h('div', { class: 'pane-head' });
  const body = h('div', { class: 'pane-body' });
  const pane = h('div', {
    class: 'pane' + (tab.activePane === leaf.id ? ' active' : '') + (leaf.loading ? ' loading' : '') + (leaf.mode === 'save' ? ' saving' : '')
      + (leaf.type === 'app' && (leaf.running || leaf.webUrl || leaf.restarting) ? ' app-open' : ''), // アプリを取り込んでいるペインは白地にする
    'data-id': leaf.id,
    onpointerdown: () => setActivePane(tab, leaf),
  }, head, body);
  leaf.el = { pane, head, body };
  leaf.ui = {};
  if (leaf.type === 'web') buildWeb(tab, leaf, head, body);
  else if (leaf.type === 'app') buildApp(tab, leaf, head, body);
  else if (leaf.type === 'files') buildFiles(tab, leaf, head, body);
  else if (leaf.type === 'home') buildHome(tab, leaf, head, body);
  else if (leaf.type === 'field') buildField(tab, leaf, head, body);
  else buildSettings(tab, leaf, head, body);
  return pane;
}

function commonButtons(tab, leaf) {
  const splitBtn = (icon, title, dir) => {
    const b = btn(icon, title, () => popupAt(b, [
      { header: title },
      { label: 'Web', icon: 'globe', run: () => splitLeaf(tab, leaf, dir, 'web') },
      { label: 'Folder', icon: 'folder', run: () => splitLeaf(tab, leaf, dir, 'files') },
      { label: 'Field', icon: 'field', run: () => splitLeaf(tab, leaf, dir, 'field') },
      S.appSupported ? { label: 'App', icon: 'window', run: () => splitLeaf(tab, leaf, dir, 'app') } : null,
      IS_SUB ? null : { label: 'Settings', icon: 'settings', run: () => splitLeaf(tab, leaf, dir, 'settings') },
    ].filter(Boolean)));
    return b;
  };
  const realBrowser = leaf.type === 'web' && S.appSupported
    ? btn('window', 'Open in real Chromium (for sites like Google that block sign-in here)', () => toRealBrowser(tab, leaf)) : null;
  return [
    realBrowser,
    splitBtn('split-h', 'Split left/right', 'row'),
    splitBtn('split-v', 'Split top/bottom', 'col'),
    leaf.type !== 'settings' && leaf.type !== 'home' && (leaf.type !== 'app' || leaf.running) && leaf.mode !== 'save' ? btn('bookmark', 'Add bookmark', () => addBookmark(leaf, tab)) : null,
    btn('close', 'Close', () => closeLeaf(tab, leaf)),
  ].filter(Boolean);
}

// ------------------------------------------------------------------ real browser pane
// ElectronのWebビューは、Googleなどに「サポートされていないブラウザ」と判定されてログインできないことがある。
// そのときはペインを本物のChromiumのウィンドウ(アプリモード)に置き換える。プロファイルは専用で、ログインは保持される。
function toRealBrowser(tab, leaf, url = leaf.url || C.behavior.homepage) {
  const nl = makeLeaf('app', { webUrl: url });
  destroyLeaf(leaf);
  replaceNode(tab, leaf, nl);
  if (tab.activePane === leaf.id) tab.activePane = nl.id;
  render();
  startApp(nl, '');
}

// ------------------------------------------------------------------ web pane
const webCmd = (leaf, cmd, arg) => api.web.cmd(leaf.id, cmd, arg);

function buildWeb(tab, leaf, head, body) {
  const back = btn('back', 'Back (Alt+←)', () => webCmd(leaf, 'back'));
  const fwd = btn('forward', 'Forward (Alt+→)', () => webCmd(leaf, 'forward'));
  const rel = btn('reload', 'Reload (F5)', () => webCmd(leaf, leaf.loading ? 'stop' : 'reload'));
  const addr = h('input', {
    type: 'text', class: 'addr', spellcheck: 'false', placeholder: 'URL or search', value: leaf.url,
    onfocus: (e) => e.target.select(),
    onkeydown: (e) => { if (e.key === 'Enter') { webCmd(leaf, 'navigate', addr.value); webCmd(leaf, 'focus'); } },
  });
  leaf.ui = { back, fwd, rel, addr };
  head.append(back, fwd, rel, addr, ...commonButtons(tab, leaf));
  const slot = h('div', { class: 'web-slot' });
  body.append(slot);
  leaf.slot = slot;
  updateWebUI(leaf);
  if (!leaf.created) { leaf.created = true; api.web.create(leaf.id, leaf.url); }
  // ブックマークのタブの最初のペインは、そのブックマークのサイトに留める(別のサイトへのリンクは main.js の guardPinned が振り分ける)
  const bm = tab.bookmarkId && leaves(tab.root)[0] === leaf ? S.bookmarks.find((b) => b.id === tab.bookmarkId && b.kind === 'web') : null;
  webCmd(leaf, 'pin', bm ? bm.target : '');
}

function updateWebUI(leaf) {
  const u = leaf.ui;
  if (!u || !u.addr) return;
  if (document.activeElement !== u.addr) u.addr.value = leaf.url;
  u.back.disabled = !leaf.canBack;
  u.fwd.disabled = !leaf.canFwd;
  leaf.el?.pane.classList.toggle('loading', !!leaf.loading);
}

// ------------------------------------------------------------------ app pane
// 外部アプリのウィンドウを、この .web-slot の位置に重ねて表示する(main.js / appembed.js が KWin 経由で配置)
async function startApp(leaf, cmd, cls = '', name = '', icon = '') {
  if (leaf.webUrl) { // 実ブラウザ(Chromium)ペイン: ウィンドウの照合用クラスはペインごとに作る
    ({ cmd, cls } = await api.app.browserCmd(leaf.webUrl, 'prelude-' + leaf.id));
    name = hostOf(leaf.webUrl) || 'Chromium';
  }
  if (!cmd.trim()) return;
  if (await api.app.launch(leaf.id, cmd, cls)) {
    Object.assign(leaf, { cmd, cls, name: name || cmd.split(/\s+/)[0].split('/').pop(), running: true, dormant: false });
    if (icon && icon !== leaf.icon) Object.assign(leaf, { icon, iconUrl: '', iconTried: false });
    if (!leaf.webUrl) { try { localStorage.setItem('lastApp', JSON.stringify({ cmd, cls, name: leaf.name, icon: leaf.icon })); } catch {} }
    render();
  } else toast('Failed to launch');
}

// ファイルを開く。新しいタブか、leaf の右の分割に出す(設定「ファイルを開く場所」)。
// PDF など PRELUDE の中で表示できるもの(設定「PRELUDE の中で表示する拡張子」)は Web のペイン(Chromium の表示機能)で、
// それ以外は既定のアプリをペインに取り込む。
// where: 'tab' | 'split' | 'window'(取り込まずに別ウィンドウ)。取り込めない環境・既定のアプリが分からない時は別ウィンドウ
async function openFileAt(tab, leaf, full, where = C.behavior.fileOpen || 'tab') {
  const ext = /\.([^./]+)$/.exec(full)?.[1]?.toLowerCase();
  if (ext === BMFILE_EXT) { openBmFile(full); return; } // ブックマークのファイルは、どこで開いても同じ配置の新しいタブ
  const inside = String(C.behavior.viewInside || '').toLowerCase().split(/[\s,]+/).filter(Boolean);
  if (where !== 'window' && ext && inside.includes(ext)) {
    const url = 'file://' + full.split('/').map(encodeURIComponent).join('/');
    if (where === 'split' && tab && leaf) splitLeaf(tab, leaf, 'row', 'web', { url }); else openTab('web', { url });
    return;
  }
  const a = where !== 'window' && S.appSupported ? await api.fs.openWith(full) : null;
  if (!a || NO_EMBED.test(a.cmd)) {
    const err = await api.fs.open(full);
    if (err) toast(`Cannot open: ${baseName(full)} (${err})`);
    return;
  }
  const nl = where === 'split' && tab && leaf ? splitLeaf(tab, leaf, 'row', 'app', { cmd: a.cmd, cls: a.cls, name: a.name, icon: a.icon })
    : openTab('app', { cmd: a.cmd, cls: a.cls, name: a.name, icon: a.icon }).root;
  startApp(nl, a.cmd, a.cls, a.name, a.icon);
}

function buildApp(tab, leaf, head, body) {
  if (leaf.webUrl && !leaf.running) { head.append(ico('window'), h('span', { class: 'ptitle' }, 'Starting…'), ...commonButtons(tab, leaf)); return; }
  if (leaf.restarting) { head.append(ico('code'), h('span', { class: 'ptitle' }, `Restarting ${DEV_NAME}…`), ...commonButtons(tab, leaf)); return; }
  const icon = appLeafIcon(leaf);
  const title = h('span', { class: 'ptitle', title: leaf.name }, leaf.running ? leaf.title || leaf.name : 'Embed an app');
  leaf.ui = { icon, title };
  head.append(icon, title, ...(isDevLeaf(leaf) ? [btn('reload', `Restart ${DEV_NAME} (to test main.js changes)`, () => restartDevPrelude(leaf))] : []), ...commonButtons(tab, leaf));
  if (leaf.running) {
    const slot = h('div', { class: 'web-slot app-slot' });
    body.append(slot);
    leaf.slot = slot;
    return;
  }
  const cmdIn = h('input', {
    type: 'text', class: 'addr', spellcheck: 'false', placeholder: 'Command (e.g. kate, dolphin)',
    value: leaf.cmd, style: { margin: '0', height: '28px' },
    onkeydown: (e) => { if (e.key === 'Enter') startApp(leaf, cmdIn.value, leaf.cls); },
  });
  const search = h('input', {
    type: 'text', class: 'addr', spellcheck: 'false', placeholder: 'Search installed apps', value: leaf.filter,
    style: { margin: '0', height: '28px' }, oninput: () => { leaf.filter = search.value; paint(); },
  });
  const list = h('div', { class: 'app-list' });
  const paint = () => {
    list.replaceChildren();
    const q = leaf.filter.toLowerCase();
    for (const a of (leaf.apps || []).filter((x) => !q || x.name.toLowerCase().includes(q)).slice(0, 200)) {
      list.append(h('div', { class: 'app-row', title: a.exec, ondblclick: () => startApp(leaf, a.exec, a.cls, a.name, a.icon), onclick: () => { cmdIn.value = a.exec; leaf.cmd = a.exec; leaf.cls = a.cls; } }, a.name));
    }
  };
  let last = null;
  try { last = JSON.parse(localStorage.getItem('lastApp') || 'null'); } catch {}
  body.append(h('div', { class: 'app-launcher' },
    last && last.cmd ? h('div', { class: 'app-bar' }, h('button', { class: 'textbtn', onclick: () => startApp(leaf, last.cmd, last.cls, last.name, last.icon) }, `Launch last app "${last.name}"`)) : null,
    h('div', { class: 'app-bar' }, cmdIn, h('button', { class: 'textbtn', onclick: () => startApp(leaf, cmdIn.value, leaf.cls) }, 'Launch')),
    h('div', { class: 'app-hint' }, 'Double-click to launch. The window will be placed over this pane.'),
    search, list));
  if (leaf.apps) paint();
  else api.app.list().then((l) => { leaf.apps = l; paint(); });
}

// フォルダペインは files.js

// ------------------------------------------------------------------ settings pane
function buildSettings(tab, leaf, head, body) {
  head.append(ico('settings'), h('span', { class: 'ptitle' }, 'Settings'), ...commonButtons(tab, leaf));
  const wrap = h('div', { class: 'settings' });
  const set = (path, v) => api.config.set(path, v);
  for (const sec of SCHEMA) {
    wrap.append(h('h3', {}, sec.title));
    for (const f of sec.fields) {
      const val = getPath(C, f.path);
      let ctl;
      if (f.type === 'color') {
        const norm = (s) => (/^#[0-9a-f]{3}$/i.test(s) ? '#' + [...s.slice(1)].map((c) => c + c).join('') : s);
        const pick = h('input', { type: 'color', value: norm(val), onchange: () => { hex.value = pick.value; set(f.path, pick.value); } });
        const hex = h('input', {
          type: 'text', class: 'hex', value: val, spellcheck: 'false',
          onchange: () => { if (/^#([0-9a-f]{3}|[0-9a-f]{6})$/i.test(hex.value.trim())) { pick.value = norm(hex.value.trim()); set(f.path, hex.value.trim()); } else hex.value = getPath(C, f.path); },
        });
        ctl = h('span', { style: { display: 'flex', gap: '8px', alignItems: 'center' } }, pick, hex);
      } else if (f.type === 'number') {
        ctl = h('input', { type: 'number', min: f.min, max: f.max, value: val, onchange: (e) => { const n = Number(e.target.value); if (Number.isFinite(n)) set(f.path, clamp(n, f.min ?? -Infinity, f.max ?? Infinity)); } });
      } else if (f.type === 'bool') {
        ctl = h('input', { type: 'checkbox', checked: !!val || null, onchange: (e) => set(f.path, e.target.checked) });
      } else if (f.type === 'select') {
        ctl = h('select', { onchange: (e) => set(f.path, e.target.value) }, f.options.map(([v, l]) => h('option', { value: v, selected: v === val || null }, l)));
      } else {
        ctl = h('input', { type: 'text', value: val ?? '', spellcheck: 'false', onchange: (e) => set(f.path, e.target.value) });
      }
      wrap.append(h('div', { class: 'frm' }, h('label', {}, f.label), ctl));
    }
  }
  // デスクトップ(セッション)。シェルとして動いていないとき(開発版を手で起動した時など)は押せないようにして理由を出す
  const back = returnButton('textbtn');
  back.disabled = true;
  const note = h('span', { class: 'hint' }, 'Checking…');
  wrap.append(h('h3', {}, 'Desktop'), h('div', { class: 'frm' }, h('label', {}, 'Return to the Kubuntu (plasmashell) desktop'), back, note));
  api.session.state().then((s) => {
    sessionShell = !!s?.shell;
    back.disabled = !sessionShell;
    note.textContent = sessionShell ? 'Quits PRELUDE. The next login will also use Kubuntu' : 'Only available while PRELUDE runs as the desktop shell (shell-mode.sh on --now)';
  });
  // アップデート(GitHub の一番新しい版へ)。更新できるのはシェルとして動いているときだけ
  const upNote = h('span', { class: 'hint' }, 'Checking…');
  const upCheck = h('button', { class: 'textbtn' }, 'Check');
  const upRun = h('button', { class: 'textbtn primary', disabled: true, title: 'Prepares the new version and restarts PRELUDE (embedded apps will close)' }, 'Update and restart');
  const checkUpdate = async () => {
    upCheck.disabled = true;
    upNote.textContent = 'Checking…';
    const r = await api.update.check();
    upCheck.disabled = false;
    upRun.disabled = !(r.shell && r.available);
    upNote.textContent = r.error ? `Current ${r.current} (${r.error})`
      : r.available ? `Current ${r.current} → new version ${r.latest} available` + (r.shell ? '' : ' (can only update while running as the shell)')
        : `Current ${r.current} (up to date)`;
  };
  upCheck.onclick = checkUpdate;
  upRun.onclick = async () => {
    upRun.disabled = upCheck.disabled = true;
    upNote.textContent = 'Preparing the new version… (restarts when done)';
    const r = await runUpdate();
    if (!r?.updated) checkUpdate();
  };
  wrap.append(h('h3', {}, 'Update'), h('div', { class: 'frm' }, h('label', {}, 'PRELUDE version'), h('span', { style: { display: 'flex', gap: '8px' } }, upCheck, upRun), upNote));
  checkUpdate();
  wrap.append(h('div', { style: { marginTop: '24px' } },
    h('button', { class: 'textbtn danger', onclick: async () => { await api.config.reset(); render(); } }, 'Reset to defaults')));
  body.append(wrap);
}

// ------------------------------------------------------------------ persistence (workspace)
// タブの中身を保存する形(JSON)にする/戻す。閉じたタブ・タブの複製もこの形を使う
const serNode = (n) => (n.k === 'split'
  ? { k: 'split', dir: n.dir, ratio: n.ratio, a: serNode(n.a), b: serNode(n.b) }
  : { k: 'leaf', type: n.type, url: n.url, path: n.path, cmd: n.cmd, cls: n.cls, name: n.name, icon: n.icon, webUrl: n.webUrl, sort: n.type === 'files' ? n.sort : undefined });
const deserNode = (n) => (n.k === 'split'
  ? { k: 'split', dir: n.dir === 'col' ? 'col' : 'row', ratio: clamp(Number(n.ratio) || 0.5, 0.1, 0.9), a: deserNode(n.a), b: deserNode(n.b) }
  : makeLeaf(['web', 'files', 'settings', 'app', 'home', 'field'].includes(n.type) ? n.type : 'files', { url: n.url, path: n.path, cmd: n.cmd, cls: n.cls, name: n.name, icon: n.icon, webUrl: n.webUrl, sort: n.sort }));

// 保存はスペースごと(space.js の spaceWsSave)。今のスペースのタブと、ほかのスペースで開いていたタブ(S.wsSpaces)を窓ごとのファイルへ
const tabsData = () => ({ active: Math.max(0, S.tabs.findIndex((t) => t.id === S.active)), tabs: S.tabs.map((t) => ({ num: t.num, bookmarkId: t.bookmarkId || undefined, bmFile: t.bmFile || undefined, root: serNode(t.root) })) });
let saveTimer = 0;
function scheduleSave() {
  clearTimeout(saveTimer);
  if (!S.space) return; // 起動中(スペースに入る前)は保存しない
  saveTimer = setTimeout(() => api.ws.save(spaceWsSave(S.space, tabsData(), S.wsSpaces)), 600);
}

function restore(ws) {
  const ids = new Set(S.bookmarks.map((b) => b.id));
  for (const t of ws.tabs) {
    try {
      const root = deserNode(t.root);
      S.tabs.push({ id: nid('t'), num: api.tab.allocNum(t.num || 0), root, activePane: leaves(root)[0].id, bookmarkId: ids.has(t.bookmarkId) ? t.bookmarkId : null, bmFile: typeof t.bmFile === 'string' ? t.bmFile : null });
    } catch {}
  }
  if (S.tabs.length) S.active = S.tabs[clamp(ws.active | 0, 0, S.tabs.length - 1)].id;
}

// ------------------------------------------------------------------ events from main
function focusAddress() {
  const l = activeLeaf(activeTab());
  const a = l?.ui?.addr;
  if (a) { a.focus(); a.select(); }
}

api.onShortcut(({ action, paneId }) => {
  const tab = activeTab();
  const leaf = activeLeaf(tab);
  if (paneId) { const f = findLeaf(paneId); if (f && f.tab === tab) setActivePane(tab, f.leaf); }
  const cur = paneId ? findLeaf(paneId)?.leaf || leaf : leaf;
  if (action === 'newTab') openTab(C.behavior.startKind);
  else if (action === 'closeTab') closeTab(tab);
  else if (action === 'reopenTab') reopenClosedTab();
  else if (action.startsWith('goto:')) gotoTabNum(Number(action.slice(5)));
  else if (action === 'fullscreen') api.win.cmd('toggleFullscreen');
  else if (action === 'focusAddress') focusAddress();
  else if (action === 'launcher') openLauncher();
  else if (cur?.type === 'web' && ['reload', 'back', 'forward'].includes(action)) webCmd(cur, action);
  else if (cur?.type === 'files' && action === 'reload') reloadDir(cur);
  else if (cur?.type === 'field' && action === 'reload') fieldReload(cur);
  else if (cur?.type === 'files' && (action === 'back' || action === 'forward')) histGo(cur, action === 'back' ? -1 : 1);
});

api.web.onState((s) => {
  const f = findLeaf(s.paneId);
  if (!f) return;
  Object.assign(f.leaf, { url: s.url, title: s.title, loading: s.loading, canBack: s.canBack, canFwd: s.canFwd, audible: !!s.audible, muted: !!s.muted });
  updateWebUI(f.leaf);
  renderSidebar();
});
api.web.onFavicon(({ paneId, icon }) => { const f = findLeaf(paneId); if (f) { f.leaf.favicon = icon; renderSidebar(); } });
api.web.onFocused(({ paneId }) => { const f = findLeaf(paneId); if (f && f.tab === activeTab()) setActivePane(f.tab, f.leaf); });
// Google のログインに進んだ Web ペインは、本物の Chromium のペインに切り替える(main.js の guardGoogleSignin)
api.web.onToChromium(({ paneId, url, popup, site }) => {
  const f = findLeaf(paneId);
  if (!f || f.leaf.type !== 'web') return;
  toRealBrowser(f.tab, f.leaf, url);
  toast(popup ? 'Google sign-in happens in Chromium. Reopened this page in Chromium'
    : site ? 'Opened in Chromium (sign-in is shared with Chromium; change the sites in Settings)'
    : 'Google sign-in happens in Chromium (this tab will use Chromium from now on)');
});

// Web ペインから別のところで開く要求。disposition: alt(Alt+クリック)/ pinned(ブックマークのタブから別サイト)/
// background-tab(中クリック・Ctrl+クリック)/ そのほか(target=_blank など)。開き方は設定の「タブ」に従う
api.web.onOpenRequest(({ fromPaneId, url, disposition }) => {
  const f = findLeaf(fromPaneId);
  if (!f) return;
  const how = disposition === 'alt' ? C.tabs.altClick : disposition === 'pinned' ? C.tabs.pinnedOtherSite : C.tabs.linkOpen;
  if (how === 'split') splitLeaf(f.tab, f.leaf, 'row', 'web', { url });
  else if (how === 'here' || how === 'stay') webCmd(f.leaf, 'navigate', url);
  else openTab('web', { url, background: disposition === 'background-tab' });
});

S.dl = {};
api.dl.onStart(({ id, filename, fromPaneId, defaultDir, total }) => {
  S.dl[id] = { received: 0, total, state: '' };
  const f = fromPaneId && findLeaf(fromPaneId);
  const tab = f ? f.tab : activeTab();
  const src = f ? f.leaf : activeLeaf(tab);
  const w = src.el?.pane.getBoundingClientRect().width || 0;
  // 元のペインが狭ければ下に、広ければ右に保存先ペインを出す
  splitLeaf(tab, src, w && w < 640 ? 'col' : 'row', 'files', { path: defaultDir, mode: 'save', dlId: id, filename });
});
api.dl.onProgress(({ id, received, total }) => {
  Object.assign(S.dl[id] ||= {}, { received, total });
  for (const t of S.tabs) for (const l of leaves(t.root)) if (l.dlId === id) updateSaveBar(l);
});
api.dl.onDone(({ id, state }) => {
  (S.dl[id] ||= {}).state = state;
  for (const t of S.tabs) for (const l of leaves(t.root)) if (l.dlId === id) updateSaveBar(l);
});
api.dl.onSaved(({ path }) => toast('Saved: ' + path));
api.dl.onError(({ message }) => toast('Save failed: ' + message));

// 取り込んだ窓の題名が変わった(appembed.js の report)
api.app.onTitle(({ paneId, title }) => {
  const f = findLeaf(paneId);
  if (!f || f.leaf.type !== 'app' || f.leaf.title === title) return;
  f.leaf.title = title;
  if (f.leaf.ui?.title?.isConnected) f.leaf.ui.title.textContent = title || f.leaf.name;
  renderSidebar();
});

api.app.onExited(({ paneId, hadWindow }) => {
  const f = findLeaf(paneId);
  if (!f || !f.leaf.running) return;
  api.app.close(paneId);
  f.leaf.running = false;
  f.leaf.title = '';
  // 次にこのタブを開いたとき、同じアプリを起動し直す。ウィンドウが一度も見つからなかったときは、起動し直しても同じなので知らせるだけ
  f.leaf.dormant = !!hadWindow && !!(f.leaf.cmd || f.leaf.webUrl);
  if (!hadWindow) toast(`Window for "${f.leaf.name || f.leaf.cmd}" not found`);
  render();
});

// タブの移動(画面ごとの窓の間)。画面を外したときは、全部のタブを主画面へ移してから窓が閉じる
api.tab.onAdopt((data) => { try { adoptTab(data); } catch {} });
api.tab.onEvacuate((to) => {
  // 開いたばかりのホームだけのタブは移さない
  for (const tab of [...S.tabs]) {
    const ls = leaves(tab.root);
    if (ls.length === 1 && ls[0].type === 'home') continue;
    moveTab(tab, to, { quiet: true });
  }
  reportNums();
  clearTimeout(saveTimer);
  api.ws.save(spaceWsSave(S.space, { active: 0, tabs: [] }, S.wsSpaces)); // 次にこの画面をつないだときは新しいタブから始める
  api.tab.evacuated();
});
api.win.onScreens((list) => { S.screens = list || []; });

api.config.onChanged((values) => {
  const hidden = C.behavior.showHidden;
  const spaceDir = C.behavior.spaceDir;
  C = values;
  if (hidden !== C.behavior.showHidden) { reloadAllFiles(); fieldReloadAll(); }
  applyTheme();
  renderSidebar();
  syncViews();
  // スペースのフォルダが変わったら、同じ名前のスペース(無ければ Main など)を新しい場所で読む。タブはそのまま
  if (spaceDir !== C.behavior.spaceDir && S.space) {
    (async () => {
      await loadSpaceList();
      S.space = spacePick(S.space, S.spaces);
      S.spaceDir = (await spaceRoot()) + '/' + S.space;
      await scanSpace();
      watchSpace();
    })();
  }
});

// ------------------------------------------------------------------ boot
function openSettings() {
  const tab = S.tabs.find((t) => leaves(t.root).some((l) => l.type === 'settings'));
  if (tab) { S.active = tab.id; render(); } else openTab('settings');
}

async function boot() {
  const [icons, cfg, ws] = await Promise.all([api.icons(), api.config.get(), api.ws.load()]);
  ICONS = icons;
  C = cfg.values;
  SCHEMA = cfg.schema;
  applyTheme();
  initFiles();
  S.appSupported = await api.app.supported();
  api.dev.info().then((d) => { S.devSelf = d.self; }); // 自分が開発版なら、開発版を開くコマンドは出さない

  $('#new-tab').append(ico('plus'));
  $('#open-settings').append(ico('settings'), h('span', { class: 'label' }, 'Settings'));
  document.body.classList.toggle('sub', IS_SUB);
  api.win.screens().then((list) => { S.screens = list || []; });
  $('#new-tab').onclick = (e) => popupAt(e.currentTarget, [
    { header: 'New tab' },
    { label: 'Home', icon: 'home', run: () => openTab('home') },
    { label: 'Web', icon: 'globe', run: () => openTab('web') },
    { label: 'Folder', icon: 'folder', run: () => openTab('files') },
    { label: 'Field', icon: 'field', run: () => openTab('field') },
    S.appSupported ? { label: 'App', icon: 'window', run: () => openTab('app') } : null,
    S.appSupported && !S.devSelf ? { label: DEV_NAME, icon: 'code', run: () => openDevPrelude() } : null,
    IS_SUB ? null : { label: 'Settings', icon: 'settings', run: () => openSettings() },
  ].filter(Boolean));
  $('#open-settings').onclick = openSettings;
  $('#space-btn').onclick = (e) => { const r = e.currentTarget.getBoundingClientRect(); spaceMenu(r.left, r.bottom + 2); };
  // 上のバー: 左の PRELUDE を押すとサイドバーを切り替える。右は時計などのボタン(bar.js)
  Object.assign($('#top-menu'), { title: sidebarTip(), onclick: cycleSidebar });
  initBar();
  loadSessionState();
  // 起動して少ししてから、新しい版があるか確かめて知らせる(ネットワークの準備を待つ)
  if (!IS_SUB) setTimeout(async () => {
    const r = await api.update.check();
    if (r.shell && r.available) toast(`New version ${r.latest} available (update from Settings > Update, or "Update PRELUDE" in the launcher)`, 8000);
  }, 20000);
  // アプリのペインのアイコンを引くため、起動時にアプリの一覧を読んでおく(読めたらタブを描き直す)
  if (!S.appList) api.app.list().then((l) => { S.appList = l; renderSidebar(); });

  new ResizeObserver(() => syncViews()).observe($('#workspace'));
  window.addEventListener('keydown', (e) => { if (e.key === 'Escape') { closePopup(); closeLauncher(); closeQuick(); closeNotifications(); } });

  reportNums(); // 再読込の前に使っていた番号を main.js から消してから、保存した番号で戻す
  // 保存したタブ(スペースごと)。旧ブックマークをスペースへ移したときは、タブが覚えている古い id をファイルのパスに替える
  const saved = spaceWsLoad(ws);
  const moved = { ...(C.bookmarksMigrated || {}), ...(await migrateBookmarks()) };
  for (const sp of Object.values(saved.spaces)) for (const t of sp.tabs) if (t && moved[t.bookmarkId]) t.bookmarkId = moved[t.bookmarkId];
  S.wsSpaces = saved.spaces;
  await loadSpaceList();
  await enterSpace(spacePick(saved.space, S.spaces)); // 前回のタブを開き直し、取り込んでいたアプリを再起動する
}

window.__prelude = { S, openTab, splitLeaf, activeTab, activeLeaf, leaves, moveTab }; // デバッグ用
boot();
