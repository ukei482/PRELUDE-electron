'use strict';
// 再生中のメディア(MPRIS)。ブラウザや音楽アプリが公開する org.mpris.MediaPlayer2.* を監視し、
// 「再生中 > 最後に動きのあったもの」を現在のプレーヤーとして画面へ送る。
const bus = require('./bus');

const PREFIX = 'org.mpris.MediaPlayer2.';
const OBJ = '/org/mpris/MediaPlayer2';
const PLAYER = 'org.mpris.MediaPlayer2.Player';

const players = new Map(); // バス名 -> { name, status, title, artist, touched, iface }
let send = () => {};
let lastJson = '';

const val = (v) => (v && typeof v === 'object' && 'value' in v ? v.value : v);

function current() {
  const list = [...players.values()].filter((p) => p.status !== 'Stopped' || p.title);
  list.sort((a, b) => ((b.status === 'Playing') - (a.status === 'Playing')) || (b.touched - a.touched));
  return list[0] || null;
}

function publish() {
  const p = current();
  const s = p ? { name: p.name, status: p.status, title: p.title, artist: p.artist } : null;
  const json = JSON.stringify(s);
  if (json === lastJson) return;
  lastJson = json;
  send('sys:media', s);
}

function apply(p, props) {
  if (props.PlaybackStatus) p.status = val(props.PlaybackStatus);
  if (props.Metadata) {
    const md = val(props.Metadata) || {};
    p.title = String(val(md['xesam:title']) || '');
    const ar = val(md['xesam:artist']);
    p.artist = Array.isArray(ar) ? ar.join(', ') : String(ar || '');
  }
  p.touched = Date.now();
  publish();
}

async function add(name) {
  const b = bus.get();
  if (!b || players.has(name)) return;
  try {
    const obj = await b.getProxyObject(name, OBJ);
    const props = obj.getInterface('org.freedesktop.DBus.Properties');
    const iface = obj.getInterface(PLAYER);
    const p = { name, status: 'Stopped', title: '', artist: '', touched: Date.now(), iface };
    players.set(name, p);
    props.on('PropertiesChanged', (i, changed) => { if (i === PLAYER) apply(p, changed); });
    apply(p, await props.GetAll(PLAYER));
  } catch { players.delete(name); }
}

function remove(name) {
  if (players.delete(name)) publish();
}

async function init({ ipcMain, send: s }) {
  send = s;
  ipcMain.handle('sys:media', () => (lastJson ? JSON.parse(lastJson) : null));
  ipcMain.handle('media:cmd', async (_e, cmd) => {
    const p = current();
    const m = { playpause: 'PlayPause', next: 'Next', prev: 'Previous' }[cmd];
    if (!p || !m) return;
    try { await p.iface[m](); } catch {}
  });
  const b = bus.get();
  if (!b) return;
  try {
    const dobj = await b.getProxyObject('org.freedesktop.DBus', '/org/freedesktop/DBus');
    const dbusIface = dobj.getInterface('org.freedesktop.DBus');
    dbusIface.on('NameOwnerChanged', (name, oldOwner, newOwner) => {
      if (!name.startsWith(PREFIX)) return;
      if (newOwner) add(name); else remove(name);
    });
    for (const name of await dbusIface.ListNames()) if (name.startsWith(PREFIX)) add(name);
  } catch (e) { console.error('media init failed', e.message); }
}

module.exports = { init };
