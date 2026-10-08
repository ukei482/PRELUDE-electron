'use strict';
// システムトレイ(StatusNotifierItem のホスト)。plasmashell の代わりに、トレイのアイコンとメニューをバーへ出す。
// ウォッチャー(org.kde.StatusNotifierWatcher)は kded6 が持っているので、plasmashell が止まっても残る。PRELUDE はホストとして登録するだけ。
// Chromium/Electron 系のアイテムは自己記述(イントロスペクション)を返さないので、プロキシは使わず、生のメッセージで呼ぶ。
const dbus = require('dbus-next');
const { nativeImage } = require('electron');
const bus = require('./bus');
const icons = require('./icons');

const { Message, Variant } = dbus;
const WATCHER = 'org.kde.StatusNotifierWatcher';
const WPATH = '/StatusNotifierWatcher';
const SNI = 'org.kde.StatusNotifierItem';
const MENU = 'com.canonical.dbusmenu';
const MAX_ITEMS = 40;
const MAX_MENU = 300; // 1つのメニューの項目数の上限

const items = new Map(); // サービス文字列 -> { id, name, path, owner, props, icon, menuPath }
let push = () => {};
let timer = null;

const call = (destination, path, iface, member, signature = '', body = []) => {
  const b = bus.get();
  if (!b) return Promise.reject(new Error('no bus'));
  return b.call(new Message({ type: dbus.MessageType.METHOD_CALL, destination, path, interface: iface, member, signature, body })).then((reply) => (reply && reply.body) || []);
};
const addMatch = (rule) => call('org.freedesktop.DBus', '/org/freedesktop/DBus', 'org.freedesktop.DBus', 'AddMatch', 's', [rule]);
const val = (v) => (v && typeof v === 'object' && 'value' in v && 'signature' in v ? v.value : v);

// "busname" or "busname/object/path"(Chromium系は後者)
function parseService(s) {
  const i = s.indexOf('/');
  return i < 0 ? { name: s, path: '/StatusNotifierItem' } : { name: s.slice(0, i), path: s.slice(i) };
}

// ARGB32(ネットワークバイト順)のピクセル列 → PNG の data URL。大きい順に選び、画面側で縮小する
function pixmapToDataUrl(pixmaps) {
  if (!Array.isArray(pixmaps) || !pixmaps.length) return null;
  const best = [...pixmaps].sort((a, b) => b[0] * b[1] - a[0] * a[1])[0];
  const [w, h, raw] = best;
  const px = Buffer.from(raw);
  if (!w || !h || w > 512 || h > 512 || px.length < w * h * 4) return null;
  const bgra = Buffer.alloc(w * h * 4);
  for (let i = 0; i < w * h; i++) { // A R G B → B G R A
    bgra[i * 4] = px[i * 4 + 3];
    bgra[i * 4 + 1] = px[i * 4 + 2];
    bgra[i * 4 + 2] = px[i * 4 + 1];
    bgra[i * 4 + 3] = px[i * 4];
  }
  try { return nativeImage.createFromBitmap(bgra, { width: w, height: h }).toDataURL(); } catch { return null; }
}

function iconFor(p) {
  const status = p.Status;
  const themed = (n) => (n ? icons.toDataUrl(n, p.IconThemePath ? [p.IconThemePath] : []) : null);
  if (status === 'NeedsAttention') {
    return themed(p.AttentionIconName) || pixmapToDataUrl(p.AttentionIconPixmap) || themed(p.IconName) || pixmapToDataUrl(p.IconPixmap);
  }
  return themed(p.IconName) || pixmapToDataUrl(p.IconPixmap);
}

function view(it) {
  const p = it.props;
  const tip = Array.isArray(p.ToolTip) ? p.ToolTip : null;
  return {
    id: it.id,
    title: String(p.Title || (tip && tip[2]) || p.Id || it.name),
    tooltip: String((tip && [tip[2], tip[3]].filter(Boolean).join('\n')) || ''),
    status: String(p.Status || 'Active'),
    icon: it.icon,
    hasMenu: !!it.menuPath,
    itemIsMenu: !!p.ItemIsMenu,
  };
}

function publish() {
  clearTimeout(timer);
  timer = setTimeout(() => push([...items.values()].map(view)), 60);
}

async function fetchProps(it) {
  try {
    const r = await call(it.name, it.path, 'org.freedesktop.DBus.Properties', 'GetAll', 's', [SNI]);
    const props = {};
    for (const [k, v] of Object.entries(r[0] || {})) props[k] = val(v);
    it.props = props;
    it.icon = iconFor(props);
    it.menuPath = typeof props.Menu === 'string' && props.Menu !== '/' ? props.Menu : null;
    publish();
  } catch { /* アイテムがもう居ない。登録解除の通知で消える */ }
}

async function addItem(service) {
  if (items.has(service) || items.size >= MAX_ITEMS) return;
  const { name, path } = parseService(service);
  const it = { id: service, name, path, owner: '', props: {}, icon: null, menuPath: null };
  items.set(service, it);
  try {
    it.owner = (await call('org.freedesktop.DBus', '/org/freedesktop/DBus', 'org.freedesktop.DBus', 'GetNameOwner', 's', [name]))[0];
    await addMatch(`type='signal',sender='${name}',interface='${SNI}'`);
  } catch { /* 取れなくても表示はする(更新の通知だけ受けられない) */ }
  await fetchProps(it);
}

function removeItem(service) {
  if (items.delete(service)) publish();
}

// ---- メニュー(DBusMenu) → 画面へ渡す木
const clean = (s) => String(s || '').replace(/__/g, '\u0000').replace(/_/g, '').replace(/\u0000/g, '_'); // ニーモニックの '_' を除く

function menuNode(raw, budget) {
  const [id, props, kids] = raw;
  const p = {};
  for (const [k, v] of Object.entries(props || {})) p[k] = val(v);
  const node = {
    id: Number(id),
    label: clean(p.label),
    type: p.type === 'separator' ? 'separator' : 'item',
    enabled: p.enabled !== false,
    visible: p.visible !== false,
    toggle: p['toggle-type'] === 'checkmark' || p['toggle-type'] === 'radio' ? Number(p['toggle-state']) === 1 : null,
    children: [],
  };
  for (const k of kids || []) {
    if (budget.n-- <= 0) break;
    const v = val(k);
    if (Array.isArray(v)) node.children.push(menuNode(v, budget));
  }
  return node;
}

async function getMenu(service) {
  const it = items.get(service);
  if (!it || !it.menuPath) return null;
  try { await call(it.name, it.menuPath, MENU, 'AboutToShow', 'i', [0]); } catch { /* 無くてもよい */ }
  let r;
  try { r = await call(it.name, it.menuPath, MENU, 'GetLayout', 'iias', [0, 10, []]); }
  catch { r = await call(it.name, it.menuPath, MENU, 'GetLayout', 'iias', [0, -1, []]); } // 深さ10を受け付けないアプリ用
  const root = menuNode(r[1], { n: MAX_MENU });
  return root.children.filter((c) => c.visible);
}

async function menuClick(service, menuId) {
  const it = items.get(service);
  if (!it || !it.menuPath || !Number.isInteger(menuId)) return;
  try { await call(it.name, it.menuPath, MENU, 'Event', 'isvu', [menuId, 'clicked', new Variant('i', 0), Math.floor(Date.now() / 1000)]); } catch {}
}

async function act(service, member, x, y) {
  const it = items.get(service);
  if (!it) return;
  try { await call(it.name, it.path, SNI, member, 'ii', [Math.round(Number(x)) || 0, Math.round(Number(y)) || 0]); } catch {}
}

async function init({ ipcMain, onItems }) {
  push = onItems;
  ipcMain.handle('tray:items', () => [...items.values()].map(view));
  ipcMain.handle('tray:menu', (_e, id) => getMenu(String(id)).catch(() => null));
  ipcMain.on('tray:menu-click', (_e, id, menuId) => menuClick(String(id), menuId));
  ipcMain.on('tray:activate', (_e, id, x, y) => act(String(id), 'Activate', x, y));
  ipcMain.on('tray:secondary', (_e, id, x, y) => act(String(id), 'SecondaryActivate', x, y));

  const b = bus.get();
  if (!b) return;
  try {
    // 更新の通知とウォッチャーの登録/解除の通知を受ける
    await addMatch(`type='signal',sender='${WATCHER}',interface='${WATCHER}'`);
    b.on('message', (msg) => {
      if (msg.type !== dbus.MessageType.SIGNAL) return;
      if (msg.interface === WATCHER) {
        if (msg.member === 'StatusNotifierItemRegistered') addItem(String(msg.body[0]));
        else if (msg.member === 'StatusNotifierItemUnregistered') removeItem(String(msg.body[0]));
      } else if (msg.interface === SNI) {
        for (const it of items.values()) if (it.owner && it.owner === msg.sender) fetchProps(it);
      }
    });
    // ホストとして登録する(名前を取ってから伝える)
    const host = `org.kde.StatusNotifierHost-${process.pid}`;
    await b.requestName(host, 0);
    await call(WATCHER, WPATH, WATCHER, 'RegisterStatusNotifierHost', 's', [host]);
    // すでに登録されているアイテム
    const r = await call(WATCHER, WPATH, 'org.freedesktop.DBus.Properties', 'Get', 'ss', [WATCHER, 'RegisteredStatusNotifierItems']);
    for (const s of val(r[0]) || []) await addItem(String(s));
  } catch (e) { console.error('tray init failed:', e.message); }
}

module.exports = { init };
