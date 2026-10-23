'use strict';
const { app, BrowserWindow, WebContentsView, Menu, ipcMain, shell, session, screen } = require('electron');
const path = require('path');
const fs = require('fs');
const { spawn } = require('child_process');

app.setName('PRELUDE-electron');

// SaaS側に「Electron」と見なされてブロックされないよう、普通のChromeのUAにする
const platToken =
  process.platform === 'win32' ? 'Windows NT 10.0; Win64; x64'
  : process.platform === 'darwin' ? 'Macintosh; Intel Mac OS X 10_15_7'
  : 'X11; Linux x86_64';
app.userAgentFallback = `Mozilla/5.0 (${platToken}) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/${process.versions.chrome} Safari/537.36`;

// Googleのログインなどは、Client Hints(sec-ch-ua / navigator.userAgentData)に「Google Chrome」が無い
// Electronを「サポートされていないブラウザ」として弾く。UAだけでなく Client Hints も本物のChromeに合わせる。
const chromeFull = process.versions.chrome;
const chromeMajor = chromeFull.split('.')[0];
const hintPlatform = process.platform === 'win32' ? 'Windows' : process.platform === 'darwin' ? 'macOS' : 'Linux';
async function spoofChrome(wc) {
  try {
    if (!wc.debugger.isAttached()) wc.debugger.attach('1.3');
    const list = (v) => [
      { brand: 'Not?A_Brand', version: '24' },
      { brand: 'Chromium', version: v },
      { brand: 'Google Chrome', version: v },
    ];
    await wc.debugger.sendCommand('Emulation.setUserAgentOverride', {
      userAgent: app.userAgentFallback,
      acceptLanguage: 'ja,en-US,en',
      userAgentMetadata: {
        brands: list(chromeMajor),
        fullVersionList: list(chromeFull).map((b) => (b.brand === 'Not?A_Brand' ? { ...b, version: '24.0.0.0' } : b)),
        platform: hintPlatform,
        platformVersion: process.platform === 'win32' ? '10.0.0' : process.platform === 'darwin' ? '10.15.7' : '6.0.0',
        architecture: 'x86',
        model: '',
        mobile: false,
        bitness: '64',
        wow64: false,
      },
    });
    // 通常のChromeには window.chrome.{app,csi,loadTimes} がある
    await wc.debugger.sendCommand('Page.addScriptToEvaluateOnNewDocument', {
      source: `(() => { const c = window.chrome || (window.chrome = {});
        if (!c.app) c.app = { isInstalled: false, InstallState: { DISABLED: 'disabled', INSTALLED: 'installed', NOT_INSTALLED: 'not_installed' }, RunningState: { CANNOT_RUN: 'cannot_run', READY_TO_RUN: 'ready_to_run', RUNNING: 'running' } };
        if (!c.csi) c.csi = () => ({ onloadT: Date.now(), startE: Date.now(), pageT: performance.now(), tran: 15 });
        if (!c.loadTimes) c.loadTimes = () => ({ requestTime: Date.now() / 1000, startLoadTime: Date.now() / 1000, commitLoadTime: Date.now() / 1000, finishDocumentLoadTime: 0, finishLoadTime: 0, firstPaintTime: 0, firstPaintAfterLoadTime: 0, navigationType: 'Other', wasFetchedViaSpdy: true, wasNpnNegotiated: true, npnNegotiatedProtocol: 'h2', wasAlternateProtocolAvailable: false, connectionInfo: 'h2' });
      })();`,
    });
  } catch {}
}

if (!app.requestSingleInstanceLock()) {
  app.quit();
  process.exit(0);
}

// 捕まらなかった例外。Electron の既定ではモーダルのエラーダイアログが出て、閉じるまでメインの処理が止まる
// (シェルとして動いているとき、それで終了も切り替えもできなくなる)。ダイアログは出さずに記録し、終了処理中ならそのまま終わる
let quitting = false;
process.on('uncaughtException', (e) => {
  console.error('uncaught exception', e);
  qlog(`uncaught exception: ${e?.stack || e}`);
  if (quitting) process.exit(0);
});

// 終了処理の各段階の時刻(PRELUDE_DEBUG のときだけ。固まった場所を後から追えるよう同期で書く)
const qlog = (msg) => { if (process.env.PRELUDE_DEBUG) try { require('fs').appendFileSync('/tmp/prelude-quit.log', `${new Date().toISOString()} ${msg}\n`); } catch {} };
// systemd(prelude-shell.service)などからの終了要求は、通常の終了として扱う(異常終了扱いだと plasmashell への自動復旧が走ってしまう)
for (const sig of ['SIGTERM', 'SIGINT']) process.on(sig, () => { qlog(sig); app.quit(); });

const { Config, SCHEMA } = require('./config');
const appembed = require('./appembed');
const bus = require('./sys/bus');
const status = require('./sys/status');
const quick = require('./sys/quick');
const sessionCtl = require('./sys/session');
const updater = require('./sys/update');
let shellMode = false; // prelude-shell.service として動いているか(起動後に判定)
sessionCtl.isShell().then((v) => { shellMode = v; });
const media = require('./sys/media');
const keys = require('./sys/keys');
const { instanceTag, cleanShell } = require('./sys/util');
const notify = require('./sys/notify');
const iconTheme = require('./sys/icons');
const overlay = require('./sys/overlay');
const osd = require('./sys/osd');
const tray = require('./sys/tray');
const fileops = require('./fileops');

let config;
let win = null; // 主画面の窓
// 画面ごとの窓。key は 'main'(主画面)と 'sub1', 'sub2'…(2枚目以降の画面。docs/shell-design.md 4.12)
const shells = new Map(); // key -> { key, win, closing, finish }
const suspended = new Set(); // Webビューを全部隠している窓の key(メニュー表示中・境界ドラッグ中)
const views = new Map(); // paneId -> { id, view, wc, rect }
const owners = new Map(); // paneId -> そのペインがある窓の key(Web・アプリのペイン。タブを別の画面へ移すと変わる)
let lastKey = 'main'; // 最後にフォーカスがあった窓(グローバルショートカットのランチャーを開く先)

const shellWin = (key) => { const w = shells.get(key)?.win; return w && !w.isDestroyed() ? w : null; };
const keyOf = (wc) => { for (const [k, s] of shells) if (s.win && !s.win.isDestroyed() && s.win.webContents === wc) return k; return null; };
const sendTo = (key, channel, payload) => { const w = shellWin(key); if (w) w.webContents.send(channel, payload); };
const broadcast = (channel, payload) => { for (const k of shells.keys()) sendTo(k, channel, payload); };
// ペインに関する知らせは、そのペインがある窓へ
const sendPane = (paneId, channel, payload) => sendTo(owners.get(paneId) || 'main', channel, payload);

// ---------------------------------------------------------------- URL
function normalizeInput(raw) {
  const s = String(raw || '').trim();
  if (!s) return config.get('behavior.homepage');
  if (/^(https?|file|about):/i.test(s)) return s;
  if (/^localhost(:\d+)?([/?#]|$)/i.test(s) || /^\d{1,3}(\.\d{1,3}){3}(:\d+)?([/?#]|$)/.test(s)) return 'http://' + s;
  if (!/\s/.test(s) && /^[^\s/]+\.[a-z]{2,}(:\d+)?([/?#].*)?$/i.test(s)) return 'https://' + s;
  return String(config.get('behavior.searchEngine')).replace('%s', encodeURIComponent(s));
}

// ---------------------------------------------------------------- shortcuts
function parseAccel(s) {
  const parts = String(s || '').split('+').map((x) => x.trim()).filter(Boolean);
  if (!parts.length) return null;
  const key = parts.pop().toLowerCase();
  const mods = parts.map((m) => m.toLowerCase());
  return {
    key,
    ctrl: mods.includes('ctrl') || mods.includes('control'),
    shift: mods.includes('shift'),
    alt: mods.includes('alt'),
    meta: mods.includes('meta') || mods.includes('cmd'),
  };
}
const accelMatches = (a, input) =>
  !!a && input.key.toLowerCase() === a.key &&
  !!input.control === a.ctrl && !!input.shift === a.shift && !!input.alt === a.alt && !!input.meta === a.meta;

function bindings() {
  return [
    ['newTab', config.get('shortcuts.newTab')],
    ['closeTab', config.get('shortcuts.closeTab')],
    ['reopenTab', config.get('shortcuts.reopenTab')],
    ['fullscreen', config.get('shortcuts.fullscreen')],
    ['launcher', config.get('shortcuts.launcher')],
    ['focusAddress', 'Ctrl+L'],
    ['reload', 'F5'],
    ['reload', 'Ctrl+R'],
    ['back', 'Alt+ArrowLeft'],
    ['forward', 'Alt+ArrowRight'],
    ['devtools', 'F12'],
    ...[1, 2, 3, 4, 5, 6, 7, 8, 9].map((n) => [`goto:${n}`, `Ctrl+${n}`]), // その番号のタブへ
  ].map(([action, accel]) => [action, parseAccel(accel)]);
}

function hookInput(wc, paneId = null, key = 'main') {
  wc.on('before-input-event', (event, input) => {
    if (input.type !== 'keyDown') return;
    // ズーム(Webビューのみ)
    if (paneId && input.control && !input.alt) {
      if (input.key === '=' || input.key === '+') { event.preventDefault(); wc.setZoomLevel(wc.getZoomLevel() + 0.5); return; }
      if (input.key === '-') { event.preventDefault(); wc.setZoomLevel(wc.getZoomLevel() - 0.5); return; }
      if (input.key === '0') { event.preventDefault(); wc.setZoomLevel(0); return; }
    }
    for (const [action, accel] of bindings()) {
      if (!accelMatches(accel, input)) continue;
      event.preventDefault();
      if (action === 'devtools') { wc.toggleDevTools(); return; }
      const to = paneId ? owners.get(paneId) || 'main' : key;
      if (action === 'focusAddress' || action === 'launcher') shellWin(to)?.webContents.focus(); // ネイティブビューからシェルへフォーカスを戻す
      sendTo(to, 'shortcut', { action, paneId });
      return;
    }
  });
}

// ---------------------------------------------------------------- window
// 画面ごとの窓を作る。中身はどれも同じ index.html で、2枚目以降の画面(role=sub)は設定・全画面などのボタンを省く。
// Wayland ではクライアントが位置を決められないので、2枚目以降の窓は題名 "PRELUDE:sub<n>" を目印に KWin スクリプト(appembed.js)が
// 主画面以外の画面へ置く
function createShellWindow(key) {
  const sub = key !== 'main';
  const w = new BrowserWindow({
    width: sub ? 1200 : 1400,
    height: sub ? 800 : 900,
    minWidth: 640,
    minHeight: 400,
    frame: false, // タイトルバーは無い(☰ は左上のペインの見出しに置く)
    show: false,
    title: sub ? `PRELUDE:${key}` : 'PRELUDE',
    backgroundColor: config.get('appearance.bg'),
    webPreferences: {
      preload: path.join(__dirname, 'preload.js'),
      contextIsolation: true,
      nodeIntegration: false,
      sandbox: true,
    },
  });
  const s = { key, win: w, closing: false, finish: null };
  shells.set(key, s);
  w.loadFile(path.join(__dirname, 'src', 'index.html'), { query: { role: sub ? 'sub' : 'main', key } });
  hookInput(w.webContents, null, key);
  w.on('focus', () => { lastKey = key; });
  if (!sub) return w;
  w.on('page-title-updated', (e) => e.preventDefault()); // 題名は KWin が窓を見分ける目印なので変えさせない
  if (!appembed.supported) placeSub(w, key); // KDE 以外(X11・Windows)は自分で位置を決められる
  w.once('ready-to-show', () => {
    w.showInactive(); // 主画面のフォーカスを奪わない
    if (!appembed.supported && isFullscreen()) w.setFullScreen(true);
  });
  // 2枚目以降の窓は閉じさせない(画面を外したときだけ、タブを主画面へ移してから閉じる)
  w.on('close', (e) => { if (!quitting && !s.closing) e.preventDefault(); });
  w.on('minimize', () => w.restore());
  w.on('closed', () => dropShell(key));
  return w;
}

function createWindow() {
  win = createShellWindow('main');
  win.once('ready-to-show', () => {
    win.show();
    if (config.get('behavior.startFullscreen')) setFullscreen(true);
    syncDisplays();
  });
  // シェルとして動いている間は、閉じる・最小化を受け付けない(Alt+F4 などで閉じると、画面に何も無くなる)。戻るときは「Kubuntuに戻る」
  win.on('close', (e) => { if (shellMode && !closingApps) e.preventDefault(); });
  win.on('minimize', () => { if (shellMode) win.restore(); });
  const pushState = () => broadcast('win:state', winState());
  ['maximize', 'unmaximize', 'enter-full-screen', 'leave-full-screen'].forEach((e) => win.on(e, pushState));
  win.on('resize', () => {
    if (pseudoFs || win.isMaximized() || win.isFullScreen()) return;
    const [width, height] = win.getSize();
    normalSize = { width, height };
  });
  // 主画面の窓を閉じたら終了する(2枚目以降の窓が残っていると window-all-closed が来ない)
  win.on('closed', () => { win = null; dropShell('main'); app.quit(); });
}

// ---------------------------------------------------------------- displays
// 画面の数。PRELUDE_FAKE_DISPLAYS=2 で、1画面の環境でも2枚目の窓を試せる(デバッグ用。その窓は主画面に重なる)
const displayCount = () => Number(process.env.PRELUDE_FAKE_DISPLAYS) || screen.getAllDisplays().length;
const SUB_RE = /^sub(\d+)$/;
const subIndex = (key) => Number(SUB_RE.exec(key)?.[1] || 0);
const screenLabel = (key) => (key === 'main' ? '主画面' : `画面${subIndex(key) + 1}`);
const liveShells = () => [...shells.values()].filter((s) => !s.closing && shellWin(s.key));

// 画面の数に合わせて、2枚目以降の窓を作る・閉じる
function syncDisplays() {
  if (!win || quitting) return;
  const want = config.get('behavior.subDisplays') ? Math.max(0, displayCount() - 1) : 0;
  for (let i = 1; i <= want; i++) if (!shells.has('sub' + i)) createShellWindow('sub' + i);
  for (const s of [...shells.values()]) if (s.key !== 'main' && subIndex(s.key) > want) evacuate(s);
  pushScreens();
}
let displayTimer = null;
const displaysChanged = () => { clearTimeout(displayTimer); displayTimer = setTimeout(syncDisplays, 1500); }; // 抜き差しの直後は数が揺れるので少し待つ

function pushScreens() {
  appembed.setSubs(liveShells().map((s) => s.key).filter((k) => k !== 'main'));
  broadcast('win:screens', liveShells().map((s) => ({ key: s.key, label: screenLabel(s.key) })));
}

// KDE 以外: 主画面以外の n 番目の画面(左から順)の中央に置く
function placeSub(w, key) {
  const primary = screen.getPrimaryDisplay();
  const others = screen.getAllDisplays().filter((d) => d.id !== primary.id).sort((a, b) => a.bounds.x - b.bounds.x || a.bounds.y - b.bounds.y);
  const d = others[subIndex(key) - 1];
  if (!d) return;
  const [width, height] = w.getSize();
  const r = d.workArea;
  w.setBounds({ x: r.x + Math.round((r.width - width) / 2), y: r.y + Math.round((r.height - height) / 2), width, height });
}

// 画面を外した: その窓のタブを(Webページ・取り込んだアプリは閉じずに)主画面へ移してから窓を閉じる。
// 画面側が移し終えたら tab:evacuated が来る。来なければ3秒で閉じる(残ったペインは dropShell が片付ける)
function evacuate(s) {
  if (s.closing) return;
  s.closing = true;
  const timer = setTimeout(() => s.finish(), 3000);
  s.finish = () => { clearTimeout(timer); if (!s.win.isDestroyed()) s.win.destroy(); };
  sendTo(s.key, 'tab:evacuate', 'main');
}

// 窓が無くなった: そこに残っていたペインを片付ける
function dropShell(key) {
  if (!shells.has(key)) return;
  shells.delete(key);
  tabNums.delete(key);
  suspended.delete(key);
  for (const [id, k] of [...owners]) {
    if (k !== key) continue;
    owners.delete(id);
    const e = views.get(id);
    if (e) { views.delete(id); try { e.wc.close(); } catch {} }
    if (!quitting && key !== 'main') appembed.close(id); // 終了時は before-quit の closeAll が穏やかに閉じる
  }
  if (!quitting) pushScreens();
}

// ペインの木(画面側のタブの root)に含まれるペインの id
const paneIdsOf = (n) => (!n ? [] : n.k === 'split' ? [...paneIdsOf(n.a), ...paneIdsOf(n.b)] : [String(n.id)]);

// タブを別の窓へ移す。Webビューは付け替え、取り込んだアプリは置き場所の窓を変えるだけなので、ページもアプリもそのまま続く
ipcMain.on('tab:move', (e, to, data) => {
  const from = keyOf(e.sender);
  if (!from || !data?.root) return;
  const target = shells.get(to);
  if (!target || target.closing || !shellWin(to)) { sendTo(from, 'tab:adopt', data); return; } // 移せなければ元の窓へ戻す
  for (const id of paneIdsOf(data.root)) {
    if (owners.get(id) !== from) continue;
    owners.set(id, to);
    const v = views.get(id);
    if (v) {
      try { shellWin(from)?.contentView.removeChildView(v.view); } catch {}
      shellWin(to).contentView.addChildView(v.view);
      v.rect = null;
      applyView(v);
    }
    appembed.setOwner(id, to);
  }
  sendTo(to, 'tab:adopt', data);
});
ipcMain.on('tab:evacuated', (e) => shells.get(keyOf(e.sender))?.finish?.());

// タブの番号(名札)。並べ替えても変わらず、全部の画面の窓で重ならないように、ここで振る。
// 開いた時に空いている一番小さい番号(want が空いていればそれ)を渡す。各窓は使っている番号の一覧を tab:nums で知らせる
const tabNums = new Map(); // 窓の key -> Set<番号>
const numOwner = (n) => { for (const [k, s] of tabNums) if (s.has(n) && shellWin(k)) return k; return null; };
ipcMain.on('tab:alloc-num', (e, want) => {
  const key = keyOf(e.sender);
  let n = Number(want) > 0 && !numOwner(Number(want)) ? Number(want) : 1;
  if (n !== Number(want)) while (numOwner(n)) n++;
  if (key) (tabNums.get(key) || tabNums.set(key, new Set()).get(key)).add(n);
  e.returnValue = n;
});
ipcMain.on('tab:nums', (e, list) => { const key = keyOf(e.sender); if (key) tabNums.set(key, new Set(list)); e.returnValue = true; });
ipcMain.on('tab:where', (e, n) => { const k = numOwner(Number(n)); e.returnValue = k ? { key: k, label: screenLabel(k) } : null; });
// 番号で呼ぶ(ランチャー・Ctrl+数字・prelude.sh tab)。"3" はタブ3(持っている窓を前に出す)、"b3" はブックマーク3(今の窓で開く)
function gotoNum(s, from = lastKey) {
  const m = /^(b?)(\d+)$/i.exec(String(s).trim());
  if (!m) return;
  const key = m[1] ? (shellWin(from) ? from : 'main') : numOwner(Number(m[2]));
  if (!key) { sendTo(shellWin(from) ? from : 'main', 'tab:goto', { num: Number(m[2]), missing: true }); return; }
  const w = shellWin(key);
  if (w) { if (w.isMinimized()) w.restore(); w.focus(); }
  sendTo(key, 'tab:goto', { bookmark: !!m[1], num: Number(m[2]) });
}
ipcMain.on('tab:goto', (e, s) => gotoNum(s, keyOf(e.sender)));
ipcMain.handle('win:screens', () => liveShells().map((s) => ({ key: s.key, label: screenLabel(s.key) })));

// KDE(Wayland)では本物の全画面にすると取り込んだアプリが後ろに隠れるので、KWinスクリプトで画面いっぱいに広げる疑似全画面にする
let pseudoFs = false;
let normalSize = { width: 1400, height: 900 }; // 疑似全画面を解除したときに戻す大きさ(通常時の大きさを覚えておく)
const isFullscreen = () => pseudoFs || !!win?.isFullScreen();
function setFullscreen(on) {
  if (!win) return;
  if (appembed.supported) {
    if (on === pseudoFs) return;
    pseudoFs = on;
    appembed.setShell(on, normalSize);
    broadcast('win:state', winState());
  } else {
    win.setFullScreen(on);
    for (const s of liveShells()) if (s.key !== 'main') s.win.setFullScreen(on);
  }
}
const winState = () => ({ maximized: !!win?.isMaximized(), fullscreen: isFullscreen() });

ipcMain.on('win:cmd', (_e, cmd) => {
  if (!win) return;
  if (cmd === 'toggleMaximize') (win.isMaximized() ? win.unmaximize() : win.maximize());
  else if (cmd === 'toggleFullscreen') setFullscreen(!isFullscreen());
});
ipcMain.handle('win:state', () => winState());

// ---------------------------------------------------------------- config / workspace / icons
ipcMain.handle('config:get', () => ({ values: config.data, schema: SCHEMA }));
ipcMain.handle('config:set', (_e, p, value) => {
  config.set(p, value);
  broadcast('config:changed', config.data);
  if (p === 'behavior.subDisplays') syncDisplays();
});
ipcMain.handle('config:reset', () => { config.reset(); broadcast('config:changed', config.data); syncDisplays(); });

// タブ・分割の配置は窓ごとに保存する(主画面は従来どおり workspace.json)
const wsFile = (key) => path.join(app.getPath('userData'), key && key !== 'main' ? `workspace-${key}.json` : 'workspace.json');
ipcMain.handle('ws:load', (e) => { try { return JSON.parse(fs.readFileSync(wsFile(keyOf(e.sender)), 'utf8')); } catch { return null; } });
ipcMain.on('ws:save', (e, data) => {
  const file = wsFile(keyOf(e.sender));
  try { fs.mkdirSync(path.dirname(file), { recursive: true }); fs.writeFile(file, JSON.stringify(data), () => {}); } catch {}
});

// ホーム画面の壁紙(設定のパスの画像を data URL で渡す。画面側のCSPは file: を読めない)
const IMG_MIME = { '.png': 'image/png', '.jpg': 'image/jpeg', '.jpeg': 'image/jpeg', '.webp': 'image/webp', '.gif': 'image/gif', '.svg': 'image/svg+xml' };
ipcMain.handle('wallpaper:get', async () => {
  const file = String(config.get('appearance.wallpaper') || '').trim();
  const mime = IMG_MIME[path.extname(file).toLowerCase()];
  if (!file || !mime) return { path: file, url: null };
  try {
    const st = await fs.promises.stat(file);
    if (!st.isFile() || st.size > 15 * 1024 * 1024) return { path: file, url: null };
    return { path: file, url: `data:${mime};base64,${(await fs.promises.readFile(file)).toString('base64')}` };
  } catch { return { path: file, url: null }; }
});
// アプリのアイコン(アイコンテーマの名前 → data URL)
ipcMain.handle('icon:get', (_e, name) => iconTheme.toDataUrl(String(name || '')));

ipcMain.handle('icons:get', () => {
  const dir = path.join(__dirname, 'assets', 'icons');
  const out = {};
  for (const f of fs.readdirSync(dir)) if (f.endsWith('.svg')) out[f.slice(0, -4)] = fs.readFileSync(path.join(dir, f), 'utf8');
  return out;
});

// ---------------------------------------------------------------- web views
const navBack = (wc) => (wc.navigationHistory ? wc.navigationHistory.canGoBack() : wc.canGoBack());
const navFwd = (wc) => (wc.navigationHistory ? wc.navigationHistory.canGoForward() : wc.canGoForward());

function applyView(e) {
  if (e.rect) {
    e.view.setBounds({
      x: Math.round(e.rect.x), y: Math.round(e.rect.y),
      width: Math.max(0, Math.round(e.rect.width)), height: Math.max(0, Math.round(e.rect.height)),
    });
  }
  e.view.setVisible(!!e.rect && e.rect.width > 0 && e.rect.height > 0 && !suspended.has(owners.get(e.id)));
}

// Google のログイン画面。Electron(埋め込みブラウザ)は「このブラウザは安全でない可能性があります」と弾かれ、UA などの偽装でも安定して通らない。
// そこで、このページに進もうとしたペインは本物の Chromium のペイン(ログインはそのプロファイルに保存される)に切り替える
const GOOGLE_SIGNIN = /^\/(v3\/signin|signin|ServiceLogin|AddSession|InteractiveLogin|AccountChooser|o\/oauth2|CheckCookie)/i;
function isGoogleSignin(u) {
  try { const x = new URL(u); return x.hostname === 'accounts.google.com' && GOOGLE_SIGNIN.test(x.pathname); } catch { return false; }
}
// 設定の「最初から Chromium で開くサイト」に当たるか(そのホスト自身かサブドメイン)。Chromium のプロファイルはログイン済みなので、開いた時からログインした状態になる
function isChromiumSite(u) {
  let host;
  try { host = new URL(u).hostname.toLowerCase(); } catch { return false; }
  return String(config.get('behavior.chromiumSites') || '').toLowerCase().split(/[\s,]+/).filter(Boolean)
    .some((s) => host === s || host.endsWith('.' + s));
}

// ペイン(またはそのポップアップ)が Google のログインへ進むのを止め、画面側にペインの切り替えを頼む。
// ポップアップ(他サイトの「Googleでログイン」)は、元のページごと Chromium で開き直す(ログイン結果を元のページに返せないため)
function guardGoogleSignin(wc, paneId, popup) {
  let handled = false; // 1回の移動で will-navigate と did-start-navigation の両方が来るので、1度だけ処理する
  const check = (e, url, _inPlace, isMain) => {
    const u = e?.url ?? url;
    // ポップアップは Google のログインだけを扱う(サイト単位の振り分けは、タブとして開いたものだけ)
    const site = !popup && isChromiumSite(u);
    if (handled || !(e?.isMainFrame ?? isMain) || !(isGoogleSignin(u) || site) || !appembed.supported || !appembed.hasBrowser()) return;
    handled = true;
    e.preventDefault?.();
    const opener = views.get(paneId)?.wc;
    const target = popup ? (opener && !opener.isDestroyed() ? opener.getURL() : u) : u;
    // 画面側はこのペイン(ポップアップなら開いた元のペイン)を壊して Chromium に置き換える。
    // 移動の途中のポップアップと、その開き元を同時に壊すと Electron が落ちる(SIGTRAP)ので、
    // 先にポップアップを閉じ、閉じ終わってから切り替えを頼む。イベントの中では何も壊さない
    setImmediate(() => {
      if (!popup) { if (!wc.isDestroyed()) wc.stop(); sendPane(paneId, 'web:to-chromium', { paneId, url: target, popup: false, site }); return; }
      const ask = () => sendPane(paneId, 'web:to-chromium', { paneId, url: target, popup: true });
      if (wc.isDestroyed()) { ask(); return; }
      wc.once('destroyed', () => setImmediate(ask));
      wc.close();
    });
  };
  wc.on('will-navigate', check);
  wc.on('will-redirect', check);
  wc.on('did-start-navigation', check); // loadURL(アドレス欄・ブックマーク)は will-navigate を通らない
}

// サイトの単位(登録ドメインのおおよそ)。mail.google.com と accounts.google.com は同じ google.com、example.co.jp は example.co.jp
function siteOf(u) {
  let host;
  try { host = new URL(u).hostname.toLowerCase(); } catch { return ''; }
  const p = host.split('.');
  const n = p.length >= 3 && p[p.length - 1].length === 2 && p[p.length - 2].length <= 3 ? 3 : 2;
  return p.slice(-n).join('.');
}

// ブックマーク(ピン留め)のタブ: 別のサイトへ移ろうとしたら、タブはそのサイトのまま残し、移り先は設定に従って新しいタブか分割で開く(Arc と同じ考え方)。
// Google のログイン・Chromium で開くサイトは、そちらの切り替えに任せる
function guardPinned(wc, paneId) {
  wc.on('will-navigate', (e) => {
    const entry = views.get(paneId);
    if (!entry?.pin || e.defaultPrevented || !e.isMainFrame || config.get('tabs.pinnedOtherSite') === 'stay') return;
    const site = siteOf(e.url);
    if (!site || site === entry.pin || isGoogleSignin(e.url) || isChromiumSite(e.url)) return;
    e.preventDefault();
    sendPane(paneId, 'web:open-request', { fromPaneId: paneId, url: e.url, disposition: 'pinned' });
  });
}

function createWeb(paneId, input, key) {
  const w = shellWin(key);
  if (!w || views.has(paneId)) return;
  const view = new WebContentsView({
    webPreferences: { contextIsolation: true, nodeIntegration: false, sandbox: true, preload: path.join(__dirname, 'preload-web.js') },
  });
  const wc = view.webContents;
  const entry = { id: paneId, view, wc, rect: null };
  views.set(paneId, entry);
  owners.set(paneId, key);
  w.contentView.addChildView(view);
  view.setVisible(false);
  hookInput(wc, paneId);

  const push = () => sendPane(paneId, 'web:state', {
    paneId, url: wc.getURL(), title: wc.getTitle(), loading: wc.isLoading(),
    canBack: navBack(wc), canFwd: navFwd(wc), audible: wc.isCurrentlyAudible(), muted: wc.isAudioMuted(),
  });
  ['did-start-loading', 'did-stop-loading', 'did-navigate', 'did-navigate-in-page', 'page-title-updated', 'dom-ready', 'audio-state-changed']
    .forEach((ev) => wc.on(ev, () => push()));
  wc.on('page-favicon-updated', (_e, icons) => sendPane(paneId, 'web:favicon', { paneId, icon: icons[0] || '' }));
  wc.on('focus', () => { lastKey = owners.get(paneId) || 'main'; sendPane(paneId, 'web:focused', { paneId }); });
  wc.on('did-fail-load', (_e, code, desc, url, isMain) => {
    if (!isMain || code === -3) return; // -3 = ユーザー操作による中断
    const html = `<meta charset=utf-8><body style="font:14px sans-serif;padding:40px;color:#444"><h2>ページを表示できません</h2><p>${String(url).replace(/</g, '&lt;')}</p><p>${desc} (${code})</p>`;
    wc.loadURL('data:text/html;charset=utf-8,' + encodeURIComponent(html)).catch(() => {});
  });

  // target=_blank などの新規タブ要求は、画面側が設定(タブ → 別のタブで開くリンク)に従って新しいタブか分割で開く。
  // window.open(features付き)のポップアップ(OAuthログイン等)は opener を保つため本物のウィンドウで開く。
  guardGoogleSignin(wc, paneId, false);
  guardPinned(wc, paneId);
  wc.setWindowOpenHandler(({ url, disposition, features }) => {
    if (disposition === 'new-window' && features && isGoogleSignin(url) && appembed.supported && appembed.hasBrowser()) {
      sendPane(paneId, 'web:to-chromium', { paneId, url: wc.getURL(), popup: true });
      return { action: 'deny' };
    }
    if (disposition === 'new-window' && features) {
      return { action: 'allow', overrideBrowserWindowOptions: { autoHideMenuBar: true } };
    }
    sendPane(paneId, 'web:open-request', { fromPaneId: paneId, url, disposition }); // background-tab(中クリック・Ctrl+クリック)は裏で開く
    return { action: 'deny' };
  });

  wc.on('did-create-window', (child) => { spoofChrome(child.webContents); guardGoogleSignin(child.webContents, paneId, true); });
  // 先に空ページを読んで描画プロセスを作り、Client Hintsを上書きしてから本当のURLへ進む
  wc.loadURL('about:blank').catch(() => {})
    .then(() => spoofChrome(wc))
    .then(() => { if (!wc.isDestroyed()) wc.loadURL(normalizeInput(input)).catch(() => {}); });
}

ipcMain.on('web:create', (e, paneId, url) => createWeb(paneId, url, keyOf(e.sender)));
ipcMain.on('web:bounds', (_e, paneId, rect) => {
  const e = views.get(paneId);
  if (!e) return;
  e.rect = rect;
  applyView(e);
});
ipcMain.on('web:suspend', (e, v) => {
  const key = keyOf(e.sender);
  if (!key) return;
  if (v) suspended.add(key); else suspended.delete(key);
  views.forEach((x) => { if (owners.get(x.id) === key) applyView(x); });
  appembed.setSuspended(key, !!v);
});
ipcMain.on('web:cmd', (_e, paneId, cmd, arg) => {
  const e = views.get(paneId);
  if (!e) return;
  const wc = e.wc;
  if (cmd === 'navigate') wc.loadURL(normalizeInput(arg)).catch(() => {});
  else if (cmd === 'back') { if (navBack(wc)) (wc.navigationHistory ? wc.navigationHistory.goBack() : wc.goBack()); }
  else if (cmd === 'forward') { if (navFwd(wc)) (wc.navigationHistory ? wc.navigationHistory.goForward() : wc.goForward()); }
  else if (cmd === 'reload') wc.reload();
  else if (cmd === 'stop') wc.stop();
  else if (cmd === 'mute') { wc.setAudioMuted(!!arg); wc.emit('audio-state-changed'); } // 状態を画面へ送り直す
  else if (cmd === 'focus') wc.focus();
  else if (cmd === 'pin') e.pin = arg ? siteOf(String(arg)) : ''; // ブックマークのタブなら、そのサイト
});
// Web ペインの中の Alt+クリック(preload-web.js)。送ってきたペインを探して、画面側に開き方を任せる
ipcMain.on('web:link', (ev, url, how) => {
  if (typeof url !== 'string' || !/^https?:/i.test(url) || how !== 'alt') return;
  for (const [paneId, e] of views) if (e.wc === ev.sender) sendPane(paneId, 'web:open-request', { fromPaneId: paneId, url, disposition: 'alt' });
});
ipcMain.on('web:destroy', (_e, paneId) => {
  const e = views.get(paneId);
  if (!e) return;
  views.delete(paneId);
  try { shellWin(owners.get(paneId))?.contentView.removeChildView(e.view); } catch {}
  owners.delete(paneId);
  try { e.wc.close(); } catch {}
});

// ---------------------------------------------------------------- app embed
appembed.onEvent((ev, paneId, extra) => sendPane(paneId, 'app:' + ev, { paneId, ...extra }));
ipcMain.handle('app:supported', () => appembed.supported);
ipcMain.handle('app:browser-cmd', (_e, url, cls) => appembed.browserCmd(url, cls));
ipcMain.handle('app:list', () => appembed.listApps());
ipcMain.handle('app:launch', async (e, paneId, cmd, cls) => {
  const key = keyOf(e.sender) || 'main';
  const ok = await appembed.launch(paneId, cmd, cls, key);
  if (ok) owners.set(paneId, key);
  return ok;
});
ipcMain.on('app:bounds', (_e, paneId, rect) => appembed.setBounds(paneId, rect));
ipcMain.on('app:close', (_e, paneId) => { appembed.close(paneId); if (!views.has(paneId)) owners.delete(paneId); });
// 取り込まずに普通のウィンドウとして起動する(設定「アプリを開く方法」が「別ウィンドウ」のとき)
ipcMain.handle('app:spawn', (_e, cmd) => {
  if (typeof cmd !== 'string' || !cmd.trim()) return false;
  try {
    const p = spawn(...cleanShell(cmd), { detached: true, stdio: 'ignore', env: appembed.childEnv(), cwd: require('os').homedir() });
    p.on('error', () => {});
    p.unref();
    return true;
  } catch { return false; }
});

// ---------------------------------------------------------------- file system
// ファイラの操作は fileops.js(fs:* の IPC)
fileops.init({ ipcMain, config: { get: (p) => config.get(p) } }); // config は起動時(whenReady)に作られる

// ---------------------------------------------------------------- downloads
// ダウンロード自体は一時フォルダへ保存を始め、フォルダペインで選ばれた場所へ移動する。
const downloads = new Map();
let dlSeq = 0;
const safeName = (n) => String(n).replace(/[\\/:*?"<>|]/g, '_');

function uniquePath(dir, name) {
  const ext = path.extname(name);
  const base = path.basename(name, ext);
  let p = path.join(dir, name);
  for (let i = 1; fs.existsSync(p); i++) p = path.join(dir, `${base} (${i})${ext}`);
  return p;
}

function finalizeDownload(d) {
  if (!d.done) return;
  if (d.state !== 'completed') {
    fs.rm(d.tmpPath, { force: true }, () => {});
    downloads.delete(d.id);
    return;
  }
  if (!d.dir || d.moved) return;
  d.moved = true;
  try {
    fs.mkdirSync(d.dir, { recursive: true });
    const dest = uniquePath(d.dir, d.filename);
    try { fs.renameSync(d.tmpPath, dest); } catch { fs.copyFileSync(d.tmpPath, dest); fs.rmSync(d.tmpPath, { force: true }); }
    sendTo(d.key, 'dl:saved', { id: d.id, path: dest });
  } catch (e) {
    sendTo(d.key, 'dl:error', { id: d.id, message: e.message });
  }
  downloads.delete(d.id);
}

function setupDownloads() {
  session.defaultSession.on('will-download', (_e, item, wc) => {
    const id = ++dlSeq;
    const tmpDir = path.join(app.getPath('temp'), 'prelude-dl');
    fs.mkdirSync(tmpDir, { recursive: true });
    const filename = item.getFilename();
    const tmpPath = path.join(tmpDir, `${id}-${safeName(filename)}`);
    item.setSavePath(tmpPath);
    const d = { id, item, tmpPath, filename, dir: null, done: false, state: '', moved: false };
    downloads.set(id, d);
    let fromPaneId = null;
    for (const [pid, v] of views) if (v.wc === wc) fromPaneId = pid;
    d.key = (fromPaneId && owners.get(fromPaneId)) || lastKey; // 保存先を選ぶペインは、ダウンロードを始めたペインの窓に出す
    sendTo(d.key, 'dl:start', {
      id, filename, total: item.getTotalBytes(), fromPaneId,
      defaultDir: config.get('behavior.downloadDir') || app.getPath('downloads'),
    });
    // 保存先のペインはタブごと別の窓へ移ることがあるので、進み具合は全部の窓へ
    item.on('updated', () => broadcast('dl:progress', { id, received: item.getReceivedBytes(), total: item.getTotalBytes() }));
    item.once('done', (_ev, state) => { d.done = true; d.state = state; broadcast('dl:done', { id, state }); finalizeDownload(d); });
  });
}
ipcMain.handle('dl:choose', (_e, id, dir) => { const d = downloads.get(id); if (d) { d.dir = dir; finalizeDownload(d); } });
ipcMain.handle('dl:cancel', (_e, id) => {
  const d = downloads.get(id);
  if (!d) return;
  if (d.done) { d.state = 'cancelled'; finalizeDownload(d); } else d.item.cancel();
});

// ---------------------------------------------------------------- lifecycle
app.whenReady().then(async () => {
  config = new Config();
  if (process.platform === 'linux') await bus.init(); // D-Bus が使えなくても続行(シェル機能なしで動く)
  Menu.setApplicationMenu(null);
  setupDownloads();
  // 通信側のClient Hintsヘッダも、全リクエストで本物のChromeと同じブランド構成にそろえる(読み込み開始との競合を避ける)
  const hintBrands = `"Not?A_Brand";v="24", "Chromium";v="${chromeMajor}", "Google Chrome";v="${chromeMajor}"`;
  const hintFull = `"Not?A_Brand";v="24.0.0.0", "Chromium";v="${chromeFull}", "Google Chrome";v="${chromeFull}"`;
  session.defaultSession.webRequest.onBeforeSendHeaders((details, cb) => {
    const h = details.requestHeaders;
    for (const k of Object.keys(h)) {
      const lk = k.toLowerCase();
      if (lk === 'sec-ch-ua') h[k] = hintBrands;
      else if (lk === 'sec-ch-ua-full-version-list') h[k] = hintFull;
    }
    cb({ requestHeaders: h });
  });
  const sys = status.init({ send: broadcast, ipcMain, onVolumeChange: (v) => osd.show('volume', v.percent, v.muted) });
  quick.init({ ipcMain, refreshStatus: sys?.refresh, onSelfChange: () => osd.quiet() });
  sessionCtl.init({ ipcMain });
  updater.init({ ipcMain });
  if (process.platform === 'linux') {
    media.init({ ipcMain, send: broadcast });
    // どのアプリが前面でも効くショートカット(KWinスクリプト経由)。
    // 音量キーはここでは扱わない。kded6 の audioshortcutsservice が plasmashell 無しでも処理する(docs/shell-design.md 6.5)
    // 別プロファイルの PRELUDE(デバッグ用)は、シェルの PRELUDE からグローバルショートカットを奪わない
    if (!instanceTag) keys.load({
      launcher: { key: config.get('shortcuts.globalLauncher'), raise: true },
      brightnessReset: { key: 'Meta+Shift+B', raise: false }, // 緊急用: 画面が暗すぎるとき
    });
    bus.onReport('goto', (s) => gotoNum(s)); // prelude.sh tab 3 / tab b3
    bus.onReport('shortcut', (name) => {
      if (name === 'launcher') { shellWin(lastKey)?.webContents.focus(); sendTo(shellWin(lastKey) ? lastKey : 'main', 'shortcut', { action: 'launcher', paneId: null }); }
      else if (name === 'brightnessReset') osd.resetBrightness();
    });
    osd.init();
    tray.init({ ipcMain, onItems: (list) => broadcast('tray:items', list) });
    // 通知サーバ。トーストは別窓(オーバーレイ)に、履歴と未読数は本体の通知センターに送る
    overlay.init();
    ipcMain.handle('overlay:theme', () => config.data.appearance);
    notify.init({
      ipcMain, file: path.join(app.getPath('userData'), 'notifications.json'),
      onToasts: (list) => overlay.send('notify', 'notify:toasts', list),
      onState: (s) => broadcast('notify:state', s),
    });
  }
  createWindow();
  screen.on('display-added', displaysChanged);
  screen.on('display-removed', displaysChanged);
  app.on('second-instance', () => { if (win) { if (win.isMinimized()) win.restore(); win.focus(); } });
});
app.on('window-all-closed', () => app.quit());
// 終了前に、取り込んだアプリを穏やかに閉じる(待つのは最大8秒)。終わってからもう一度 quit する
let closingApps = false;
app.on('before-quit', (e) => {
  qlog(`before-quit closingApps=${closingApps}`);
  // 終了処理が固まっても、systemd に SIGKILL される(=失敗扱い)前に自分で正常終了する。TimeoutStopSec(20秒)より短く。
  // (SIGTERM は Electron が自分で受けて quit するため、下の process.on('SIGTERM') には来ないことがある。ここで必ず仕掛ける)
  if (!quitting) setTimeout(() => { qlog('fallback exit'); process.exit(0); }, 14000).unref();
  quitting = true;
  if (closingApps) return;
  closingApps = true;
  e.preventDefault();
  appembed.closeAll(8000).catch(() => {}).finally(() => { qlog('closeAll done'); app.quit(); });
});
app.on('will-quit', () => { qlog('will-quit'); fileops.shutdown(); appembed.shutdown(); keys.shutdown(); overlay.shutdown(); bus.shutdown(); qlog('will-quit done'); });
app.on('quit', () => qlog('quit'));
process.on('exit', () => qlog('process exit'));
