'use strict';
// オーバーレイ窓(透明・枠なし・最前面)。通知のトーストなど、Webビューや取り込みアプリの上に重ねて出したいものを表示する。
// Wayland ではクライアントが自分の位置を決められないので、KWinスクリプト(prelude-overlay)が
// 窓のタイトル "PRELUDE-overlay:<種類>" を見て、画面の隅へ置いて最前面にする。
const { BrowserWindow, ipcMain } = require('electron');
const fs = require('fs');
const os = require('os');
const path = require('path');
const { exec, instanceTag } = require('./util');

const NAME = 'prelude-overlay' + instanceTag;
const supported = process.platform === 'linux' && /kde/i.test(process.env.XDG_CURRENT_DESKTOP || '');
const KINDS = { notify: { width: 380 }, osd: { width: 280 } };
const wins = new Map(); // kind -> BrowserWindow
let scriptFile = null;

function script() {
  return `
var EL_PID = ${process.pid};
function mainWin() {
  var best = null, area = 0;
  workspace.windowList().forEach(function (w) {
    if (w.pid !== EL_PID || !w.normalWindow || /^PRELUDE-overlay:/.test(w.caption || '')) return;
    var a = w.frameGeometry.width * w.frameGeometry.height;
    if (a > area) { best = w; area = a; }
  });
  return best;
}
function place(w) {
  if (w.pid !== EL_PID) return;
  var m = /^PRELUDE-overlay:(\\w+)/.exec(w.caption || '');
  if (!m) return;
  w.noBorder = true; w.keepAbove = true;
  w.skipTaskbar = true; w.skipPager = true; w.skipSwitcher = true;
  var mw = mainWin();
  var r = (mw && mw.output ? mw.output : w.output).geometry;
  var g = w.frameGeometry, t = null;
  if (m[1] === 'notify') t = { x: r.x + r.width - g.width - 12, y: r.y + 44, width: g.width, height: g.height };
  if (m[1] === 'osd') t = { x: r.x + Math.round((r.width - g.width) / 2), y: r.y + r.height - g.height - 72, width: g.width, height: g.height };
  if (t && (g.x !== t.x || g.y !== t.y)) w.frameGeometry = t;
  workspace.raiseWindow(w);
}
function watch(w) {
  place(w);
  if (w.frameGeometryChanged) w.frameGeometryChanged.connect(function () { place(w); });
  if (w.captionChanged) w.captionChanged.connect(function () { place(w); });
}
workspace.windowAdded.connect(watch);
workspace.windowList().forEach(watch);
`;
}

const kwin = (obj, method, ...args) => exec('gdbus', ['call', '--session', '--dest', 'org.kde.KWin', '--object-path', obj, '--method', method, ...args]);

async function loadScript() {
  if (!supported) return;
  if (!scriptFile) scriptFile = path.join(fs.mkdtempSync(path.join(os.tmpdir(), 'prelude-overlay-')), 'overlay.js');
  await kwin('/Scripting', 'org.kde.kwin.Scripting.unloadScript', NAME);
  fs.writeFileSync(scriptFile, script());
  const out = await kwin('/Scripting', 'org.kde.kwin.Scripting.loadScript', scriptFile, NAME);
  const id = out.ok && /(-?\d+)/.exec(out.out)?.[1];
  // 読み込んだ番号の Script{id}.run は使わない。KWin は番号を「今の本数」で振るので、途中のスクリプトを外したあとに読み込むと
  // 既にある別のスクリプトと番号が重なり、run がそちらに届いて新しいスクリプトが動かないことがある。start は未実行のものを全部動かす
  if (id != null && Number(id) >= 0) await kwin('/Scripting', 'org.kde.kwin.Scripting.start');
}

function create(kind) {
  const win = new BrowserWindow({
    width: KINDS[kind].width, height: 10, show: false, frame: false, transparent: true, hasShadow: false,
    focusable: false, skipTaskbar: true, resizable: true, alwaysOnTop: true, title: `PRELUDE-overlay:${kind}`,
    webPreferences: { preload: path.join(__dirname, '..', 'preload-overlay.js'), contextIsolation: true, nodeIntegration: false, sandbox: true },
  });
  win.on('page-title-updated', (e) => e.preventDefault()); // タイトルは KWin が窓を見分ける目印なので変えさせない
  win.loadFile(path.join(__dirname, '..', 'src', 'overlay.html'), { query: { kind } });
  win.on('closed', () => wins.delete(kind));
  wins.set(kind, win);
  return win;
}

function get(kind) {
  return wins.get(kind) || create(kind);
}

// ページが内容の高さを知らせてきたら、その大きさで表示する(0 なら隠す)
function init() {
  ipcMain.on('overlay:size', (e, kind, height) => {
    const win = wins.get(kind);
    if (!win || win.isDestroyed() || win.webContents !== e.sender) return;
    const h = Math.min(Math.max(0, Math.round(Number(height) || 0)), 900);
    if (h === 0) { win.hide(); return; }
    win.setSize(KINDS[kind].width, h);
    if (!win.isVisible()) win.showInactive();
  });
  loadScript();
}

const send = (kind, channel, payload) => {
  if (!supported) return;
  const win = get(kind);
  if (win.isDestroyed()) return;
  const go = () => { if (!win.isDestroyed()) win.webContents.send(channel, payload); };
  // 作った直後はページがまだ読み込み中で、送っても取りこぼす
  if (win.webContents.isLoading()) win.webContents.once('did-finish-load', go); else go();
};

const shutdown = () => { if (supported) kwin('/Scripting', 'org.kde.kwin.Scripting.unloadScript', NAME); };

module.exports = { supported, init, send, shutdown };
