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

let seq = 0;
const nid = (p) => `${p}${++seq}`;
const clamp = (v, lo, hi) => Math.min(hi, Math.max(lo, v));
const baseName = (p) => String(p || '').replace(/[\\/]+$/, '').split(/[\\/]/).pop() || String(p || '');
const hostOf = (u) => { try { return new URL(u).hostname; } catch { return ''; } };
const fmtSize = (n) => (n < 1024 ? n + ' B' : n < 1048576 ? (n / 1024).toFixed(1) + ' KB' : n < 1073741824 ? (n / 1048576).toFixed(1) + ' MB' : (n / 1073741824).toFixed(2) + ' GB');
const getPath = (o, p) => p.split('.').reduce((v, k) => (v == null ? undefined : v[k]), o);

// ------------------------------------------------------------------ state
let C = null; // config values
let SCHEMA = [];
const S = { tabs: [], active: null, bookmarks: [], foreignBm: [] };

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
      k: 'leaf', id, type, path: o.path ?? C.behavior.startPath ?? '', entries: [], parent: null, err: '', sel: -1, scrollTop: 0,
      mode: o.mode || 'browse', dlId: o.dlId, filename: o.filename || '', dlMsg: '',
    };
  }
  if (type === 'app') return { k: 'leaf', id, type, cmd: o.cmd || '', cls: o.cls || '', name: o.name || '', webUrl: o.webUrl || '', running: false, apps: null, filter: '' };
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
  if (l.type === 'app') return l.name || l.cmd || 'アプリ';
  return '設定';
}
const leafIcon = (l) => (l.type === 'web' ? 'globe' : l.type === 'files' ? 'folder' : l.type === 'app' ? 'window' : 'settings');

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
    const icon = l.type === 'web' && l.favicon ? h('img', { class: 'fav', src: l.favicon, onerror: (e) => e.target.replaceWith(ico('globe')) }) : ico(leafIcon(l));
    tl.append(h('div', {
      class: 'item' + (tab.id === S.active ? ' active' : ''),
      onclick: () => showTab(tab),
      onauxclick: (e) => { if (e.button === 1) closeTab(tab); },
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

function openTab(kind, o = {}) {
  if (kind === 'app' && !S.appSupported) kind = 'files';
  const leaf = makeLeaf(kind, o);
  const tab = { id: nid('t'), root: leaf, activePane: leaf.id, bookmarkId: o.bookmarkId || null };
  S.tabs.push(tab);
  S.active = tab.id;
  render();
  return tab;
}

function destroyLeaf(l) {
  if (l.type === 'web' && l.created) api.web.destroy(l.id);
  if (l.type === 'app' && l.running) api.app.close(l.id);
  if (l.mode === 'save' && l.dlId && !l.dlHandled) { l.dlHandled = true; api.dl.cancel(l.dlId); }
}

function closeTab(tab) {
  leaves(tab.root).forEach(destroyLeaf);
  const i = S.tabs.indexOf(tab);
  S.tabs.splice(i, 1);
  if (!S.tabs.length) { openTab(C.behavior.startKind); return; }
  if (S.active === tab.id) S.active = S.tabs[Math.min(i, S.tabs.length - 1)].id;
  render();
}

function splitLeaf(tab, leaf, dir, kind, o = {}) {
  if (kind === 'files' && o.path == null) o.path = leaf.type === 'files' && leaf.mode !== 'save' ? leaf.path : C.behavior.startPath;
  const nl = makeLeaf(kind, o);
  replaceNode(tab, leaf, { k: 'split', dir, ratio: 0.5, a: leaf, b: nl });
  tab.activePane = nl.id;
  S.active = tab.id;
  render();
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
  pop = null;
  suspend(-1);
}
function popup(items, x, y) {
  closePopup();
  const bd = h('div', { class: 'backdrop', onpointerdown: closePopup, oncontextmenu: (e) => { e.preventDefault(); closePopup(); } });
  const m = h('div', { class: 'menu' }, items.map((it) => (it.header ? h('div', { class: 'mh' }, it.header)
    : it.sep ? h('div', { class: 'sep' })
    : h('div', { class: 'mi' + (it.danger ? ' danger' : ''), style: it.danger ? { color: 'var(--danger)' } : null, onclick: () => { closePopup(); it.run(); } }, it.icon ? ico(it.icon) : null, it.label))));
  overlay.append(bd, m);
  m.style.left = clamp(x, 4, innerWidth - m.offsetWidth - 4) + 'px';
  m.style.top = clamp(y, 4, innerHeight - m.offsetHeight - 4) + 'px';
  pop = { bd, m };
  suspend(+1);
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
function toast(msg) {
  const t = $('#toast');
  t.textContent = msg;
  t.style.display = 'block';
  clearTimeout(toastTimer);
  toastTimer = setTimeout(() => { t.style.display = 'none'; }, 2600);
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
  else buildSettings(tab, leaf, head, body);
  return pane;
}

function commonButtons(tab, leaf) {
  const splitBtn = (icon, title, dir) => {
    const b = btn(icon, title, () => popupAt(b, [
      { header: title },
      { label: 'Web', icon: 'globe', run: () => splitLeaf(tab, leaf, dir, 'web') },
      { label: 'フォルダ', icon: 'folder', run: () => splitLeaf(tab, leaf, dir, 'files') },
      S.appSupported ? { label: 'アプリ', icon: 'window', run: () => splitLeaf(tab, leaf, dir, 'app') } : null,
      { label: '設定', icon: 'settings', run: () => splitLeaf(tab, leaf, dir, 'settings') },
    ].filter(Boolean)));
    return b;
  };
  const realBrowser = leaf.type === 'web' && S.appSupported
    ? btn('window', '本物のChromiumで開く（Googleなどログインできないサイト用）', () => toRealBrowser(tab, leaf)) : null;
  return [
    realBrowser,
    splitBtn('split-h', '左右に分割', 'row'),
    splitBtn('split-v', '上下に分割', 'col'),
    leaf.type !== 'settings' && (leaf.type !== 'app' || leaf.running) && leaf.mode !== 'save' ? btn('bookmark', 'ブックマークに追加', () => addBookmark(leaf)) : null,
    btn('close', '閉じる', () => closeLeaf(tab, leaf)),
  ].filter(Boolean);
}

// ------------------------------------------------------------------ real browser pane
// ElectronのWebビューは、Googleなどに「サポートされていないブラウザ」と判定されてログインできないことがある。
// そのときはペインを本物のChromiumのウィンドウ(アプリモード)に置き換える。プロファイルは専用で、ログインは保持される。
function toRealBrowser(tab, leaf) {
  const url = leaf.url || C.behavior.homepage;
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
async function startApp(leaf, cmd, cls = '', name = '') {
  if (leaf.webUrl) { // 実ブラウザ(Chromium)ペイン: ウィンドウの照合用クラスはペインごとに作る
    ({ cmd, cls } = await api.app.browserCmd(leaf.webUrl, 'prelude-' + leaf.id));
    name = hostOf(leaf.webUrl) || 'Chromium';
  }
  if (!cmd.trim()) return;
  if (await api.app.launch(leaf.id, cmd, cls)) {
    Object.assign(leaf, { cmd, cls, name: name || cmd.split(/\s+/)[0].split('/').pop(), running: true, dormant: false });
    if (!leaf.webUrl) { try { localStorage.setItem('lastApp', JSON.stringify({ cmd, cls, name: leaf.name })); } catch {} }
    render();
  } else toast('起動できませんでした');
}

function buildApp(tab, leaf, head, body) {
  if (leaf.webUrl && !leaf.running) { head.append(ico('window'), h('span', { class: 'ptitle' }, '起動中…'), ...commonButtons(tab, leaf)); return; }
  head.append(ico('window'), h('span', { class: 'ptitle' }, leaf.running ? leaf.name : 'アプリを取り込む'), ...commonButtons(tab, leaf));
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
      list.append(h('div', { class: 'app-row', title: a.exec, ondblclick: () => startApp(leaf, a.exec, a.cls, a.name), onclick: () => { cmdIn.value = a.exec; leaf.cmd = a.exec; leaf.cls = a.cls; } }, a.name));
    }
  };
  let last = null;
  try { last = JSON.parse(localStorage.getItem('lastApp') || 'null'); } catch {}
  body.append(h('div', { class: 'app-launcher' },
    last && last.cmd ? h('div', { class: 'app-bar' }, h('button', { class: 'textbtn', onclick: () => startApp(leaf, last.cmd, last.cls, last.name) }, `前回のアプリ「${last.name}」を起動`)) : null,
    h('div', { class: 'app-bar' }, cmdIn, h('button', { class: 'textbtn', onclick: () => startApp(leaf, cmdIn.value, leaf.cls) }, '起動')),
    h('div', { class: 'app-hint' }, 'ダブルクリックで起動。起動したウィンドウはこのペインの位置に重ねて表示されます。'),
    search, list));
  if (leaf.apps) paint();
  else api.app.list().then((l) => { leaf.apps = l; paint(); });
}

// ------------------------------------------------------------------ folder pane
const ROW = 28;
const EXT = {
  image: 'png jpg jpeg gif webp svg bmp ico', archive: 'zip 7z rar tar gz xz bz2',
  code: 'js ts jsx tsx rs py c cpp h hpp java go html css json toml yaml yml sh bat ps1', text: 'txt md log csv',
};
function iconForEntry(en) {
  if (en.isDir) return 'folder';
  const ext = en.name.split('.').pop().toLowerCase();
  for (const [name, list] of Object.entries(EXT)) if (list.split(' ').includes(ext)) return name;
  return 'file';
}

async function loadDir(leaf, p) {
  const r = await api.fs.list(p ?? leaf.path, C.behavior.showHidden);
  if (r.ok) { Object.assign(leaf, { path: r.path, parent: r.parent, entries: r.entries, err: '', sel: -1 }); } else leaf.err = r.error;
  leaf.scrollTop = 0;
  if (leaf.vl) leaf.vl.list.scrollTop = 0;
  if (leaf.ui?.addr) leaf.ui.addr.value = leaf.path;
  if (leaf.ui?.up) leaf.ui.up.disabled = !leaf.parent;
  paintRows(leaf);
  updateSaveBar(leaf);
  renderSidebar();
  scheduleSave();
}

function paintRows(leaf) {
  const v = leaf.vl;
  if (!v || !v.list.isConnected) return;
  v.err.textContent = leaf.err;
  v.err.style.display = leaf.err ? 'block' : 'none';
  const n = leaf.entries.length;
  v.spacer.style.height = n * ROW + 'px';
  const top = v.list.scrollTop;
  const first = Math.max(0, Math.floor(top / ROW) - 6);
  const last = Math.min(n, Math.ceil((top + v.list.clientHeight) / ROW) + 6);
  v.rows.style.transform = `translateY(${first * ROW}px)`;
  v.rows.replaceChildren(...leaf.entries.slice(first, last).map((en, i) => {
    const idx = first + i;
    return h('div', {
      class: 'frow' + (idx === leaf.sel ? ' sel' : ''),
      onclick: () => { leaf.sel = idx; paintRows(leaf); },
      ondblclick: () => (en.isDir ? loadDir(leaf, en.full) : leaf.mode === 'save' ? null : api.fs.open(en.full)),
    }, ico(iconForEntry(en)), h('span', { class: 'name', title: en.name }, en.name),
    h('span', { class: 'meta' }, en.isDir ? '' : fmtSize(en.size)),
    h('span', { class: 'date' }, en.mtime ? new Date(en.mtime).toLocaleString('ja-JP', { dateStyle: 'short', timeStyle: 'short' }) : ''));
  }));
}

function buildFiles(tab, leaf, head, body) {
  const up = btn('up', '上へ', () => leaf.parent && loadDir(leaf, leaf.parent));
  const home = btn('home', 'ホーム', async () => loadDir(leaf, await api.fs.home()));
  const drv = btn('drive', 'ドライブ・場所', async () => {
    const list = await api.fs.drives();
    popupAt(drv, list.map((d) => ({ label: d.label, icon: 'drive', run: () => loadDir(leaf, d.path) })));
  });
  const addr = h('input', {
    type: 'text', class: 'addr', spellcheck: 'false', value: leaf.path, placeholder: 'パス',
    onfocus: (e) => e.target.select(),
    onkeydown: (e) => { if (e.key === 'Enter') loadDir(leaf, addr.value); },
  });
  leaf.ui = { up, addr };
  head.append(up, home, drv, addr, ...commonButtons(tab, leaf));
  up.disabled = !leaf.parent;

  const list = h('div', { class: 'vlist' });
  const spacer = h('div', { class: 'vspacer' });
  const rows = h('div', { class: 'vrows' });
  const err = h('div', { class: 'ferr', style: { display: 'none' } });
  list.append(spacer, rows);
  body.append(err, list);
  leaf.vl = { list, spacer, rows, err };
  list.addEventListener('scroll', () => { leaf.scrollTop = list.scrollTop; paintRows(leaf); });
  new ResizeObserver(() => paintRows(leaf)).observe(list);

  if (leaf.mode === 'save') {
    const msg = h('span', { class: 'msg' });
    const bar = h('div', { class: 'savebar' }, msg,
      h('button', { class: 'textbtn primary', onclick: () => { leaf.dlHandled = true; api.dl.choose(leaf.dlId, leaf.path); closeLeaf(tab, leaf); } }, 'ここに保存'),
      h('button', { class: 'textbtn', onclick: () => closeLeaf(tab, leaf) }, 'キャンセル'));
    leaf.ui.saveMsg = msg;
    body.append(bar);
  }

  if (leaf.loaded) {
    list.scrollTop = leaf.scrollTop;
    paintRows(leaf);
    updateSaveBar(leaf);
  } else {
    leaf.loaded = true;
    loadDir(leaf);
  }
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
  leaf.ui.saveMsg.textContent = `「${leaf.filename}」の保存先を選んでください${st}`;
}

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
      : { k: 'leaf', type: n.type, url: n.url, path: n.path, cmd: n.cmd, cls: n.cls, name: n.name, webUrl: n.webUrl });
    api.ws.save({ active: Math.max(0, S.tabs.findIndex((t) => t.id === S.active)), tabs: S.tabs.map((t) => ({ bookmarkId: t.bookmarkId, root: ser(t.root) })) });
  }, 600);
}

function restore(ws) {
  const deser = (n) => (n.k === 'split'
    ? { k: 'split', dir: n.dir === 'col' ? 'col' : 'row', ratio: clamp(Number(n.ratio) || 0.5, 0.1, 0.9), a: deser(n.a), b: deser(n.b) }
    : makeLeaf(['web', 'files', 'settings', 'app'].includes(n.type) ? n.type : 'files', { url: n.url, path: n.path, cmd: n.cmd, cls: n.cls, name: n.name, webUrl: n.webUrl }));
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
api.web.onOpenRequest(({ fromPaneId, url }) => {
  const f = findLeaf(fromPaneId);
  if (f) splitLeaf(f.tab, f.leaf, 'row', 'web', { url });
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

api.app.onExited(({ paneId }) => {
  const f = findLeaf(paneId);
  if (!f || !f.leaf.running) return;
  api.app.close(paneId);
  f.leaf.running = false;
  f.leaf.dormant = !!(f.leaf.cmd || f.leaf.webUrl); // 次にこのタブを開いたとき、同じアプリを起動し直す
  render();
});

api.config.onChanged((values) => {
  C = values;
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
  S.appSupported = await api.app.supported();

  $('#tb-sidebar').append(ico('menu'));
  $('#tb-fullscreen').append(ico('fullscreen'));
  $('#tb-min').append(ico('minimize'));
  $('#tb-max').append(ico('maximize'));
  $('#tb-close').append(ico('close'));
  $('#new-tab').append(ico('plus'));
  $('#open-settings').append(ico('settings'), '設定');
  $('#tb-sidebar').onclick = () => api.config.set('behavior.sidebarVisible', !C.behavior.sidebarVisible);
  $('#tb-fullscreen').onclick = () => api.win.cmd('toggleFullscreen');
  $('#tb-min').onclick = () => api.win.cmd('minimize');
  $('#tb-max').onclick = () => api.win.cmd('toggleMaximize');
  $('#tb-close').onclick = () => api.win.cmd('close');
  $('#new-tab').onclick = (e) => popupAt(e.currentTarget, [
    { header: '新しいタブ' },
    { label: 'Web', icon: 'globe', run: () => openTab('web') },
    { label: 'フォルダ', icon: 'folder', run: () => openTab('files') },
    S.appSupported ? { label: 'アプリ', icon: 'window', run: () => openTab('app') } : null,
    { label: '設定', icon: 'settings', run: () => openSettings() },
  ].filter(Boolean));
  $('#open-settings').onclick = openSettings;
  initBar();

  new ResizeObserver(() => syncViews()).observe($('#workspace'));
  window.addEventListener('keydown', (e) => { if (e.key === 'Escape') { closePopup(); closeLauncher(); closeQuick(); } });

  if (ws && Array.isArray(ws.tabs)) restore(ws);
  if (!S.tabs.length) openTab(C.behavior.startKind);
  else render();
  // 前回取り込んでいたアプリを再起動する
  if (S.appSupported) for (const t of S.tabs) for (const l of leaves(t.root)) if (l.type === 'app' && (l.cmd || l.webUrl)) startApp(l, l.cmd, l.cls, l.name);
}

window.__prelude = { S, openTab, splitLeaf, activeTab, activeLeaf, leaves }; // デバッグ用
boot();
