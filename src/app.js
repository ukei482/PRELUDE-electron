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
const S = { tabs: [], active: null, bookmarks: [], foreignBm: [], screens: [] };

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
  document.body.classList.toggle('no-sidebar', !C.behavior.sidebarVisible);
}

function tabTitle(tab) {
  const l = activeLeaf(tab);
  if (!l) return '';
  if (l.type === 'web') return l.title || hostOf(l.url) || '新しいタブ';
  if (l.type === 'files') return l.mode === 'save' ? '保存先を選択' : baseName(l.path) || 'フォルダ';
  if (l.type === 'app') return l.title || l.name || l.cmd || 'アプリ'; // 取り込んだ窓の題名(Chromium なら今のページ名)を優先
  if (l.type === 'home') return 'ホーム';
  if (l.type === 'field') return baseName(l.path) || 'フィールド';
  return '設定';
}
const leafIcon = (l) => (l.type === 'web' ? 'globe' : l.type === 'files' ? 'folder' : l.type === 'app' ? 'window' : l.type === 'home' ? 'home' : l.type === 'field' ? 'field' : 'settings');

// アプリのペインのアイコン(.desktop の Icon。実ブラウザのペインは Chromium のアイコン)。まだ読めていなければ汎用のアイコンを出し、読めたら描き直す
function appLeafIcon(l) {
  if (l.iconUrl) return h('img', { class: 'fav', src: l.iconUrl, onerror: (e) => e.target.replaceWith(ico('window')) });
  const list = S.appList || [];
  const name = l.icon || (l.webUrl ? list.find((a) => /chrom/i.test(a.exec))?.icon : list.find((a) => a.exec === l.cmd)?.icon) || '';
  if (name && !l.iconTried) {
    l.iconTried = true;
    appIconUrl(name).then((u) => { if (u) { l.iconUrl = u; renderSidebar(); if (l.ui?.icon?.isConnected) l.ui.icon.replaceWith(appLeafIcon(l)); } });
  }
  return ico('window');
}

function renderSidebar() {
  const bl = $('#bm-list');
  bl.replaceChildren();
  if (!S.bookmarks.length) bl.append(h('div', { class: 'empty' }, 'ペインの ☆ で追加'));
  for (const b of S.bookmarks) {
    const open = S.tabs.some((t) => t.bookmarkId === b.id);
    bl.append(h('div', {
      class: 'item' + (activeTab()?.bookmarkId === b.id ? ' active' : ''),
      title: b.target,
      onclick: () => openBookmark(b),
      oncontextmenu: (e) => {
        e.preventDefault();
        popup([
          { label: '名前を変更', icon: 'text', run: () => renameBookmark(b) },
          { label: '削除', icon: 'close', danger: true, run: () => { S.bookmarks = S.bookmarks.filter((x) => x !== b); saveBookmarks(); renderSidebar(); } },
        ], e.clientX, e.clientY);
      },
    }, ico(b.kind === 'web' ? 'globe' : b.kind === 'app' ? 'window' : 'folder'), h('span', { class: 'label' }, b.title || b.target), open ? h('span', { class: 'badge' }, '●') : null));
  }

  const tl = $('#tab-list');
  tl.replaceChildren();
  for (const tab of S.tabs) {
    const l = activeLeaf(tab);
    const n = leaves(tab.root).length;
    const icon = l.type === 'web' && l.favicon ? h('img', { class: 'fav', src: l.favicon, onerror: (e) => e.target.replaceWith(ico('globe')) })
      : l.type === 'app' ? appLeafIcon(l) : ico(leafIcon(l));
    tl.append(h('div', {
      class: 'item' + (tab.id === S.active ? ' active' : ''),
      onclick: () => showTab(tab),
      onauxclick: (e) => { if (e.button === 1) closeTab(tab); },
      oncontextmenu: (e) => {
        e.preventDefault();
        const others = S.screens.filter((x) => x.key !== WIN_KEY);
        popup([
          ...others.map((x) => ({ label: `${x.label}へ移す`, icon: 'window', run: () => moveTab(tab, x.key) })),
          others.length ? { sep: true } : null,
          { label: '閉じる', icon: 'close', danger: true, run: () => closeTab(tab) },
        ].filter(Boolean), e.clientX, e.clientY);
      },
    }, icon, h('span', { class: 'label' }, tabTitle(tab)), n > 1 ? h('span', { class: 'badge' }, '+' + (n - 1)) : null,
    h('button', { class: 'ibtn x', title: '閉じる', onclick: (e) => { e.stopPropagation(); closeTab(tab); } }, ico('close'))));
  }
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
  const tab = { id: nid('t'), root: leaf, activePane: leaf.id, bookmarkId: o.bookmarkId || null };
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
}

function closeTab(tab) {
  leaves(tab.root).forEach(destroyLeaf);
  const i = S.tabs.indexOf(tab);
  S.tabs.splice(i, 1);
  if (!S.tabs.length) { openTab(C.behavior.startKind); return; }
  if (S.active === tab.id) S.active = S.tabs[Math.min(i, S.tabs.length - 1)].id;
  render();
}

// タブを別の画面の窓へ移す。ペインは壊さずに渡す(Webページ・取り込んだアプリは main.js が移った先の窓へ付け替える)。
// o.quiet: 残りのタブが無くなっても新しいタブを開かない(画面を外して全部移すとき)
const LIVE_ONLY = new Set(['el', 'ui', 'slot', 'vl', 'apps', 'entries', 'view', 'sel', 'ta', 'fv']); // 画面の部品など、移った先で作り直すもの
function moveTab(tab, to, o = {}) {
  const data = JSON.parse(JSON.stringify({ bookmarkId: tab.bookmarkId, activePane: tab.activePane, root: tab.root }, (k, v) => (LIVE_ONLY.has(k) ? undefined : v)));
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
  const tab = { id: nid('t'), root: data.root, activePane: ids.has(data.activePane) ? data.activePane : leaves(data.root)[0].id, bookmarkId: data.bookmarkId || null };
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

// ------------------------------------------------------------------ bookmarks
const bmToJson = (b) => (b.kind === 'app'
  ? { id: b.id, kind: 'app', title: b.title, cmd: b.cmd || '', cls: b.cls || '', webUrl: b.webUrl || '' }
  : { id: b.id, kind: b.kind, title: b.title, [b.kind === 'web' ? 'url' : 'path']: b.target });
function loadBookmarks() {
  S.bookmarks = [];
  S.foreignBm = [];
  for (const b of C.bookmarks || []) {
    if (b.kind === 'app' && (b.cmd || b.webUrl)) S.bookmarks.push({ id: b.id, kind: 'app', title: b.title || '', target: b.webUrl || b.cmd, cmd: b.cmd || '', cls: b.cls || '', webUrl: b.webUrl || '' });
    else if (b.kind === 'web' || b.kind === 'files') S.bookmarks.push({ id: b.id, kind: b.kind, title: b.title || '', target: b.url || b.path || '' });
    else S.foreignBm.push(b); // Rust版のアプリ取り込みブックマークなどは保持だけする
  }
}
const saveBookmarks = () => api.config.set('bookmarks', [...S.bookmarks.map(bmToJson), ...S.foreignBm]);

function addBookmark(leaf) {
  if (leaf.type === 'app') {
    const target = leaf.webUrl || leaf.cmd;
    if (!target) return;
    if (S.bookmarks.some((b) => b.kind === 'app' && b.target === target)) { toast('登録済みです'); return; }
    S.bookmarks.push({ id: 'b' + Date.now().toString(36), kind: 'app', title: leaf.name || target, target, cmd: leaf.webUrl ? '' : leaf.cmd, cls: leaf.webUrl ? '' : leaf.cls, webUrl: leaf.webUrl || '' });
    saveBookmarks();
    renderSidebar();
    toast('ブックマークに追加しました');
    return;
  }
  const kind = leaf.type === 'web' ? 'web' : 'files';
  const target = kind === 'web' ? leaf.url : leaf.path;
  if (!target) return;
  if (S.bookmarks.some((b) => b.kind === kind && b.target === target)) { toast('登録済みです'); return; }
  S.bookmarks.push({ id: 'b' + Date.now().toString(36), kind, title: (kind === 'web' ? leaf.title || hostOf(target) : baseName(target)) || target, target });
  saveBookmarks();
  renderSidebar();
  toast('ブックマークに追加しました');
}

function openBookmark(b) {
  const tab = S.tabs.find((t) => t.bookmarkId === b.id);
  if (tab) { showTab(tab); return; } // 1ブックマーク = 最大1タブ
  if (b.kind === 'app') {
    const t = openTab('app', { cmd: b.cmd, cls: b.cls, name: b.title, webUrl: b.webUrl, bookmarkId: b.id });
    startApp(t.root, b.cmd, b.cls, b.title);
    return;
  }
  openTab(b.kind, { url: b.target, path: b.target, bookmarkId: b.id });
}

async function renameBookmark(b) {
  const name = await askText('ブックマーク名', b.title);
  if (name != null && name.trim()) { b.title = name.trim(); saveBookmarks(); renderSidebar(); }
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
      h('div', { class: 'row' }, h('button', { class: 'textbtn', onclick: () => done(null) }, 'キャンセル'), h('button', { class: 'textbtn primary', onclick: () => done(input.value) }, 'OK')));
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
    class: 'pane' + (tab.activePane === leaf.id ? ' active' : '') + (leaf.loading ? ' loading' : '') + (leaf.mode === 'save' ? ' saving' : ''),
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
      { label: 'フォルダ', icon: 'folder', run: () => splitLeaf(tab, leaf, dir, 'files') },
      { label: 'フィールド', icon: 'field', run: () => splitLeaf(tab, leaf, dir, 'field') },
      S.appSupported ? { label: 'アプリ', icon: 'window', run: () => splitLeaf(tab, leaf, dir, 'app') } : null,
      IS_SUB ? null : { label: '設定', icon: 'settings', run: () => splitLeaf(tab, leaf, dir, 'settings') },
    ].filter(Boolean)));
    return b;
  };
  const realBrowser = leaf.type === 'web' && S.appSupported
    ? btn('window', '本物のChromiumで開く（Googleなどログインできないサイト用）', () => toRealBrowser(tab, leaf)) : null;
  return [
    realBrowser,
    splitBtn('split-h', '左右に分割', 'row'),
    splitBtn('split-v', '上下に分割', 'col'),
    leaf.type !== 'settings' && leaf.type !== 'home' && (leaf.type !== 'app' || leaf.running) && leaf.mode !== 'save' ? btn('bookmark', 'ブックマークに追加', () => addBookmark(leaf)) : null,
    btn('close', '閉じる', () => closeLeaf(tab, leaf)),
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
  const back = btn('back', '戻る (Alt+←)', () => webCmd(leaf, 'back'));
  const fwd = btn('forward', '進む (Alt+→)', () => webCmd(leaf, 'forward'));
  const rel = btn('reload', '再読込 (F5)', () => webCmd(leaf, leaf.loading ? 'stop' : 'reload'));
  const addr = h('input', {
    type: 'text', class: 'addr', spellcheck: 'false', placeholder: 'URL または検索ワード', value: leaf.url,
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
  } else toast('起動できませんでした');
}

function buildApp(tab, leaf, head, body) {
  if (leaf.webUrl && !leaf.running) { head.append(ico('window'), h('span', { class: 'ptitle' }, '起動中…'), ...commonButtons(tab, leaf)); return; }
  const icon = appLeafIcon(leaf);
  const title = h('span', { class: 'ptitle', title: leaf.name }, leaf.running ? leaf.title || leaf.name : 'アプリを取り込む');
  leaf.ui = { icon, title };
  head.append(icon, title, ...commonButtons(tab, leaf));
  if (leaf.running) {
    const slot = h('div', { class: 'web-slot app-slot' });
    body.append(slot);
    leaf.slot = slot;
    return;
  }
  const cmdIn = h('input', {
    type: 'text', class: 'addr', spellcheck: 'false', placeholder: '起動コマンド(例: kate, dolphin)',
    value: leaf.cmd, style: { margin: '0', height: '28px' },
    onkeydown: (e) => { if (e.key === 'Enter') startApp(leaf, cmdIn.value, leaf.cls); },
  });
  const search = h('input', {
    type: 'text', class: 'addr', spellcheck: 'false', placeholder: 'インストール済みアプリを検索', value: leaf.filter,
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
    last && last.cmd ? h('div', { class: 'app-bar' }, h('button', { class: 'textbtn', onclick: () => startApp(leaf, last.cmd, last.cls, last.name, last.icon) }, `前回のアプリ「${last.name}」を起動`)) : null,
    h('div', { class: 'app-bar' }, cmdIn, h('button', { class: 'textbtn', onclick: () => startApp(leaf, cmdIn.value, leaf.cls) }, '起動')),
    h('div', { class: 'app-hint' }, 'ダブルクリックで起動。起動したウィンドウはこのペインの位置に重ねて表示されます。'),
    search, list));
  if (leaf.apps) paint();
  else api.app.list().then((l) => { leaf.apps = l; paint(); });
}

// フォルダペインは files.js

// ------------------------------------------------------------------ settings pane
function buildSettings(tab, leaf, head, body) {
  head.append(ico('settings'), h('span', { class: 'ptitle' }, '設定'), ...commonButtons(tab, leaf));
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
  const note = h('span', { class: 'hint' }, '確認中…');
  wrap.append(h('h3', {}, 'デスクトップ'), h('div', { class: 'frm' }, h('label', {}, 'Kubuntu(plasmashell)のデスクトップに戻る'), back, note));
  api.session.state().then((s) => {
    sessionShell = !!s?.shell;
    back.disabled = !sessionShell;
    note.textContent = sessionShell ? 'PRELUDE を終了します。次回ログインも Kubuntu になります' : 'PRELUDE がデスクトップシェルとして動いている時だけ使えます(shell-mode.sh on --now)';
  });
  // アップデート(GitHub の一番新しい版へ)。更新できるのはシェルとして動いているときだけ
  const upNote = h('span', { class: 'hint' }, '確認中…');
  const upCheck = h('button', { class: 'textbtn' }, '確認');
  const upRun = h('button', { class: 'textbtn primary', disabled: true, title: '新しい版を準備して、PRELUDE を再起動します(取り込んだアプリは閉じます)' }, '更新して再起動');
  const checkUpdate = async () => {
    upCheck.disabled = true;
    upNote.textContent = '確認中…';
    const r = await api.update.check();
    upCheck.disabled = false;
    upRun.disabled = !(r.shell && r.available);
    upNote.textContent = r.error ? `今の版 ${r.current}(${r.error})`
      : r.available ? `今の版 ${r.current} → 新しい版 ${r.latest} があります` + (r.shell ? '' : '(シェルとして動いている時だけ更新できます)')
        : `今の版 ${r.current}(最新です)`;
  };
  upCheck.onclick = checkUpdate;
  upRun.onclick = async () => {
    upRun.disabled = upCheck.disabled = true;
    upNote.textContent = '新しい版を準備しています…(終わると再起動します)';
    const r = await runUpdate();
    if (!r?.updated) checkUpdate();
  };
  wrap.append(h('h3', {}, 'アップデート'), h('div', { class: 'frm' }, h('label', {}, 'PRELUDE の版'), h('span', { style: { display: 'flex', gap: '8px' } }, upCheck, upRun), upNote));
  checkUpdate();
  wrap.append(h('div', { style: { marginTop: '24px' } },
    h('button', { class: 'textbtn danger', onclick: async () => { await api.config.reset(); render(); } }, '初期値に戻す')));
  body.append(wrap);
}

// ------------------------------------------------------------------ persistence (workspace)
let saveTimer = 0;
function scheduleSave() {
  clearTimeout(saveTimer);
  saveTimer = setTimeout(() => {
    const ser = (n) => (n.k === 'split'
      ? { k: 'split', dir: n.dir, ratio: n.ratio, a: ser(n.a), b: ser(n.b) }
      : { k: 'leaf', type: n.type, url: n.url, path: n.path, cmd: n.cmd, cls: n.cls, name: n.name, icon: n.icon, webUrl: n.webUrl, sort: n.type === 'files' ? n.sort : undefined });
    api.ws.save({ active: Math.max(0, S.tabs.findIndex((t) => t.id === S.active)), tabs: S.tabs.map((t) => ({ bookmarkId: t.bookmarkId, root: ser(t.root) })) });
  }, 600);
}

function restore(ws) {
  const deser = (n) => (n.k === 'split'
    ? { k: 'split', dir: n.dir === 'col' ? 'col' : 'row', ratio: clamp(Number(n.ratio) || 0.5, 0.1, 0.9), a: deser(n.a), b: deser(n.b) }
    : makeLeaf(['web', 'files', 'settings', 'app', 'home', 'field'].includes(n.type) ? n.type : 'files', { url: n.url, path: n.path, cmd: n.cmd, cls: n.cls, name: n.name, icon: n.icon, webUrl: n.webUrl, sort: n.sort }));
  for (const t of ws.tabs) {
    try {
      const root = deser(t.root);
      S.tabs.push({ id: nid('t'), root, activePane: leaves(root)[0].id, bookmarkId: t.bookmarkId || null });
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
  else if (action === 'fullscreen') api.win.cmd('toggleFullscreen');
  else if (action === 'focusAddress') focusAddress();
  else if (action === 'launcher') openLauncher();
  else if (cur?.type === 'web' && ['reload', 'back', 'forward'].includes(action)) webCmd(cur, action);
  else if (cur?.type === 'files' && action === 'reload') reloadDir(cur);
  else if (cur?.type === 'files' && (action === 'back' || action === 'forward')) histGo(cur, action === 'back' ? -1 : 1);
});

api.web.onState((s) => {
  const f = findLeaf(s.paneId);
  if (!f) return;
  Object.assign(f.leaf, { url: s.url, title: s.title, loading: s.loading, canBack: s.canBack, canFwd: s.canFwd });
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
  toast(popup ? 'Google でのログインは Chromium で行います。このページを Chromium で開き直しました'
    : site ? 'Chromium で開きました(ログインは Chromium と共有されます。対象のサイトは設定で変更できます)'
    : 'Google のログインは Chromium で行います(このタブは以後 Chromium で表示されます)');
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
api.dl.onSaved(({ path }) => toast('保存しました: ' + path));
api.dl.onError(({ message }) => toast('保存に失敗しました: ' + message));

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
  if (!hadWindow) toast(`「${f.leaf.name || f.leaf.cmd}」のウィンドウが見つかりませんでした`);
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
  clearTimeout(saveTimer);
  api.ws.save({ active: 0, tabs: [] }); // 次にこの画面をつないだときは新しいタブから始める
  api.tab.evacuated();
});
api.win.onScreens((list) => { S.screens = list || []; });

api.config.onChanged((values) => {
  const hidden = C.behavior.showHidden;
  C = values;
  if (hidden !== C.behavior.showHidden) reloadAllFiles();
  applyTheme();
  loadBookmarks();
  renderSidebar();
  syncViews();
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
  loadBookmarks();
  initFiles();
  S.appSupported = await api.app.supported();

  $('#tb-sidebar').append(ico('menu'));
  $('#tb-fullscreen').append(ico('fullscreen'));
  $('#tb-max').append(ico('maximize'));
  $('#new-tab').append(ico('plus'));
  $('#open-settings').append(ico('settings'), '設定');
  document.body.classList.toggle('sub', IS_SUB);
  if (IS_SUB) $('#tb-title').textContent = `PRELUDE ${WIN_KEY.replace(/^sub(\d+)$/, (_m, n) => `画面${Number(n) + 1}`)}`;
  api.win.screens().then((list) => { S.screens = list || []; });
  $('#tb-sidebar').onclick = () => api.config.set('behavior.sidebarVisible', !C.behavior.sidebarVisible);
  $('#tb-fullscreen').onclick = () => api.win.cmd('toggleFullscreen');
  $('#tb-max').onclick = () => api.win.cmd('toggleMaximize');
  $('#new-tab').onclick = (e) => popupAt(e.currentTarget, [
    { header: '新しいタブ' },
    { label: 'ホーム', icon: 'home', run: () => openTab('home') },
    { label: 'Web', icon: 'globe', run: () => openTab('web') },
    { label: 'フォルダ', icon: 'folder', run: () => openTab('files') },
    { label: 'フィールド', icon: 'field', run: () => openTab('field') },
    S.appSupported ? { label: 'アプリ', icon: 'window', run: () => openTab('app') } : null,
    IS_SUB ? null : { label: '設定', icon: 'settings', run: () => openSettings() },
  ].filter(Boolean));
  $('#open-settings').onclick = openSettings;
  initBar();
  loadSessionState();
  // 起動して少ししてから、新しい版があるか確かめて知らせる(ネットワークの準備を待つ)
  if (!IS_SUB) setTimeout(async () => {
    const r = await api.update.check();
    if (r.shell && r.available) toast(`新しい版 ${r.latest} があります(設定の「アップデート」か、ランチャーの「PRELUDE を更新」で更新できます)`, 8000);
  }, 20000);
  // アプリのペインのアイコンを引くため、起動時にアプリの一覧を読んでおく(読めたらタブを描き直す)
  if (!S.appList) api.app.list().then((l) => { S.appList = l; renderSidebar(); });

  new ResizeObserver(() => syncViews()).observe($('#workspace'));
  window.addEventListener('keydown', (e) => { if (e.key === 'Escape') { closePopup(); closeLauncher(); closeQuick(); closeNotifications(); } });

  if (ws && Array.isArray(ws.tabs)) restore(ws);
  if (!S.tabs.length) openTab(C.behavior.startKind);
  else render();
  // 前回取り込んでいたアプリを再起動する
  if (S.appSupported) for (const t of S.tabs) for (const l of leaves(t.root)) if (l.type === 'app' && (l.cmd || l.webUrl)) startApp(l, l.cmd, l.cls, l.name);
}

window.__prelude = { S, openTab, splitLeaf, activeTab, activeLeaf, leaves, moveTab }; // デバッグ用
boot();
