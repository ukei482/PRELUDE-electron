'use strict';
// 通知サーバ(org.freedesktop.Notifications)。plasmashell の代わりに通知を受け取り、
// オーバーレイのトースト表示・履歴・おやすみモードを管理する。
// 名前は取り合わない: plasmashell が持っている間は待機し、止まったら自動で引き継ぐ(D-Bus の待ち行列)。
const dbus = require('dbus-next');
const fs = require('fs');
const bus = require('./bus');
const icons = require('./icons');

// テスト時だけ別名にできる。別プロファイルの PRELUDE(デバッグ用)も別名にする(本物の名前を待つと、シェルが止まったときに通知を横取りする)
const NAME = process.env.PRELUDE_NOTIFY_NAME || (require('./util').instanceTag ? 'org.prelude.debug.Notifications' : 'org.freedesktop.Notifications');
const OBJ = '/org/freedesktop/Notifications';
const HISTORY_MAX = 100;
const DEFAULT_TIMEOUT = 6000;
const MAX_TOASTS = 5;
const clip = (s, n) => String(s || '').slice(0, n);

const toasts = new Map(); // id -> { entry, timer }
let history = [];
let unread = 0;
let dnd = false;
let seq = 0;
let iface = null;
let owner = false;
let hooks = { toasts: () => {}, state: () => {} };
let dataFile = null;
let saveTimer = null;

// ---------------------------------------------------------------- 状態
function loadHistory() {
  try {
    const d = JSON.parse(fs.readFileSync(dataFile, 'utf8'));
    if (Array.isArray(d.history)) history = d.history.slice(0, HISTORY_MAX);
    dnd = !!d.dnd;
    seq = history.reduce((m, h) => Math.max(m, h.id || 0), 0);
  } catch {}
}
function saveHistory() {
  clearTimeout(saveTimer);
  saveTimer = setTimeout(() => { try { fs.writeFile(dataFile, JSON.stringify({ history, dnd }), () => {}); } catch {} }, 500);
}
const summary = () => ({ history, unread, dnd, serving: owner });
function push() {
  hooks.toasts([...toasts.values()].map((t) => t.entry));
  hooks.state(summary());
}

// ---------------------------------------------------------------- 通知の受信と終了
function notify({ app, replaces, icon, title, body, actions, hints, timeout }) {
  const hint = (k) => (hints && hints[k] ? hints[k].value : undefined);
  const urgency = Number(hint('urgency') ?? 1);
  const id = replaces > 0 ? replaces : ++seq;
  if (id > seq) seq = id;
  const acts = [];
  for (let i = 0; i + 1 < actions.length; i += 2) acts.push({ key: clip(actions[i], 100), label: clip(actions[i + 1], 100) });
  const entry = {
    id, app: clip(app, 100), summary: clip(title, 300), body: clip(body, 2000), urgency,
    icon: icons.toDataUrl(hint('image-path') || icon) || icons.toDataUrl(hint('desktop-entry')) || null,
    actions: acts, time: Date.now(),
  };
  // 履歴(アイコンは重いので保存しない)
  history = [{ ...entry, icon: null, actions: [] }, ...history.filter((h) => h.id !== id)].slice(0, HISTORY_MAX);
  unread++;
  saveHistory();

  const old = toasts.get(id);
  if (old) clearTimeout(old.timer);
  toasts.delete(id);
  if (!(dnd && urgency < 2)) { // おやすみモード中は緊急(critical)以外をトーストにしない
    // 緊急は消えない。それ以外は指定された時間(未指定なら既定)
    const ms = urgency >= 2 || timeout === 0 ? 0 : timeout > 0 ? timeout : DEFAULT_TIMEOUT;
    const t = { entry, timer: ms ? setTimeout(() => close(id, 1), ms) : null };
    toasts.set(id, t);
    while (toasts.size > MAX_TOASTS) { const first = toasts.keys().next().value; close(first, 4); }
  }
  push();
  return id;
}

function close(id, reason) {
  const t = toasts.get(id);
  if (t) { clearTimeout(t.timer); toasts.delete(id); }
  if (t && iface) { try { iface.NotificationClosed(id, reason); } catch {} }
  push();
}

function invoke(id, key) {
  if (!toasts.has(id)) return;
  if (iface) { try { iface.ActionInvoked(id, String(key)); } catch {} }
  close(id, 2);
}

// ---------------------------------------------------------------- D-Bus
class Notifications extends dbus.interface.Interface {
  GetCapabilities() { return ['body', 'actions', 'icon-static']; }
  Notify(app, replaces, icon, title, body, actions, hints, timeout) { return notify({ app, replaces, icon, title, body, actions, hints, timeout }); }
  CloseNotification(id) { close(id, 3); }
  GetServerInformation() { return ['PRELUDE', 'prelude', '0.1', '1.2']; }
  NotificationClosed(id, reason) { return [id, reason]; }
  ActionInvoked(id, key) { return [id, key]; }
}
Notifications.configureMembers({
  methods: {
    GetCapabilities: { outSignature: 'as' },
    Notify: { inSignature: 'susssasa{sv}i', outSignature: 'u' },
    CloseNotification: { inSignature: 'u' },
    GetServerInformation: { outSignature: 'ssss' },
  },
  signals: { NotificationClosed: { signature: 'uu' }, ActionInvoked: { signature: 'us' } },
});

async function init({ ipcMain, file, onToasts, onState }) {
  dataFile = file;
  hooks = { toasts: onToasts, state: onState };
  loadHistory();

  // 画面側(本体の通知センター / オーバーレイ)からの操作
  ipcMain.handle('notify:state', () => summary());
  ipcMain.handle('notify:toasts', () => [...toasts.values()].map((t) => t.entry));
  ipcMain.on('notify:seen', () => { unread = 0; push(); });
  ipcMain.on('notify:dnd', (_e, on) => { dnd = !!on; saveHistory(); push(); });
  ipcMain.on('notify:remove', (_e, id) => { history = history.filter((h) => h.id !== id); saveHistory(); push(); });
  ipcMain.on('notify:clear', () => { history = []; unread = 0; saveHistory(); push(); });
  ipcMain.on('toast:action', (_e, id, key) => invoke(Number(id), key));
  ipcMain.on('toast:dismiss', (_e, id) => close(Number(id), 2));

  const b = bus.get();
  if (!b) return;
  try {
    iface = new Notifications('org.freedesktop.Notifications');
    b.export(OBJ, iface);
    const r = await b.requestName(NAME, 0); // 置き換えず、空くのを待つ
    owner = r === dbus.RequestNameReply.PRIMARY_OWNER || r === dbus.RequestNameReply.ALREADY_OWNER;
    b.on('NameAcquired', () => {});
    // 名前を引き継いだ(plasmashell が止まった)ら状態を更新する
    const dobj = await b.getProxyObject('org.freedesktop.DBus', '/org/freedesktop/DBus');
    dobj.getInterface('org.freedesktop.DBus').on('NameOwnerChanged', async (name, _old, now) => {
      if (name !== NAME) return;
      const me = await dobj.getInterface('org.freedesktop.DBus').GetNameOwner(NAME).catch(() => '');
      const was = owner;
      owner = !!me && me === b.name;
      if (owner !== was) hooks.state(summary());
      void now;
    });
    hooks.state(summary());
  } catch (e) { console.error('notification server failed:', e.message); }
}

module.exports = { init };
