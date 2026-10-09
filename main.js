'use strict';
const { app, BrowserWindow, WebContentsView, Menu, ipcMain, shell, session } = require('electron');
const path = require('path');
const fs = require('fs');
const os = require('os');
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
let shellMode = false; // prelude-shell.service として動いているか(起動後に判定)
sessionCtl.isShell().then((v) => { shellMode = v; });
const media = require('./sys/media');
const keys = require('./sys/keys');
const notify = require('./sys/notify');
const iconTheme = require('./sys/icons');
const overlay = require('./sys/overlay');
const osd = require('./sys/osd');
const tray = require('./sys/tray');

let config;
let win = null;
let suspended = false; // true の間、Webビューを全部隠す(メニュー表示中・境界ドラッグ中)
const views = new Map(); // paneId -> { view, wc, rect }

const send = (channel, payload) => {
  if (win && !win.isDestroyed()) win.webContents.send(channel, payload);
};

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
    ['fullscreen', config.get('shortcuts.fullscreen')],
    ['launcher', config.get('shortcuts.launcher')],
    ['focusAddress', 'Ctrl+L'],
    ['reload', 'F5'],
    ['reload', 'Ctrl+R'],
    ['back', 'Alt+ArrowLeft'],
    ['forward', 'Alt+ArrowRight'],
    ['devtools', 'F12'],
  ].map(([action, accel]) => [action, parseAccel(accel)]);
}

function hookInput(wc, paneId = null) {
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
      if ((action === 'focusAddress' || action === 'launcher') && win) win.webContents.focus(); // ネイティブビューからシェルへフォーカスを戻す
      send('shortcut', { action, paneId });
      return;
    }
  });
}

// ---------------------------------------------------------------- window
function createWindow() {
  win = new BrowserWindow({
    width: 1400,
    height: 900,
    minWidth: 640,
    minHeight: 400,
    frame: false, // タイトルバーは自前で描く
    show: false,
    title: 'PRELUDE',
    backgroundColor: config.get('appearance.bg'),
    webPreferences: {
      preload: path.join(__dirname, 'preload.js'),
      contextIsolation: true,
      nodeIntegration: false,
      sandbox: true,
    },
  });
  win.loadFile(path.join(__dirname, 'src', 'index.html'));
  win.once('ready-to-show', () => {
    win.show();
    if (config.get('behavior.startFullscreen')) setFullscreen(true);
  });
  hookInput(win.webContents);
  // シェルとして動いている間は、閉じる・最小化を受け付けない(Alt+F4 などで閉じると、画面に何も無くなる)。戻るときは「Kubuntuに戻る」
  win.on('close', (e) => { if (shellMode && !closingApps) e.preventDefault(); });
  win.on('minimize', () => { if (shellMode) win.restore(); });
  const pushState = () => send('win:state', winState());
  ['maximize', 'unmaximize', 'enter-full-screen', 'leave-full-screen'].forEach((e) => win.on(e, pushState));
  win.on('resize', () => {
    if (pseudoFs || win.isMaximized() || win.isFullScreen()) return;
    const [width, height] = win.getSize();
    normalSize = { width, height };
  });
  win.on('closed', () => { win = null; views.clear(); });
}
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
    send('win:state', winState());
  } else win.setFullScreen(on);
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
ipcMain.handle('config:set', (_e, p, value) => { config.set(p, value); send('config:changed', config.data); });
ipcMain.handle('config:reset', () => { config.reset(); send('config:changed', config.data); });

const wsFile = () => path.join(app.getPath('userData'), 'workspace.json');
ipcMain.handle('ws:load', () => { try { return JSON.parse(fs.readFileSync(wsFile(), 'utf8')); } catch { return null; } });
ipcMain.on('ws:save', (_e, data) => {
  try { fs.mkdirSync(path.dirname(wsFile()), { recursive: true }); fs.writeFile(wsFile(), JSON.stringify(data), () => {}); } catch {}
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
  e.view.setVisible(!!e.rect && e.rect.width > 0 && e.rect.height > 0 && !suspended);
}

// Google のログイン画面。Electron(埋め込みブラウザ)は「このブラウザは安全でない可能性があります」と弾かれ、UA などの偽装でも安定して通らない。
// そこで、このページに進もうとしたペインは本物の Chromium のペイン(ログインはそのプロファイルに保存される)に切り替える
const GOOGLE_SIGNIN = /^\/(v3\/signin|signin|ServiceLogin|AddSession|InteractiveLogin|AccountChooser|o\/oauth2|CheckCookie)/i;
function isGoogleSignin(u) {
  try { const x = new URL(u); return x.hostname === 'accounts.google.com' && GOOGLE_SIGNIN.test(x.pathname); } catch { return false; }
}
// ペイン(またはそのポップアップ)が Google のログインへ進むのを止め、画面側にペインの切り替えを頼む。
// ポップアップ(他サイトの「Googleでログイン」)は、元のページごと Chromium で開き直す(ログイン結果を元のページに返せないため)
function guardGoogleSignin(wc, paneId, popup) {
  const check = (e, url, _inPlace, isMain) => {
    const u = e?.url ?? url;
    if (!(e?.isMainFrame ?? isMain) || !isGoogleSignin(u) || !appembed.supported || !appembed.hasBrowser()) return;
    e.preventDefault?.();
    wc.stop();
    const opener = views.get(paneId)?.wc;
    send('web:to-chromium', { paneId, url: popup ? (opener && !opener.isDestroyed() ? opener.getURL() : u) : u, popup: !!popup });
    if (popup) setImmediate(() => { try { wc.close(); } catch {} });
  };
  wc.on('will-navigate', check);
  wc.on('will-redirect', check);
  wc.on('did-start-navigation', check); // loadURL(アドレス欄・ブックマーク)は will-navigate を通らない
}

function createWeb(paneId, input) {
  if (!win || views.has(paneId)) return;
  const view = new WebContentsView({
    webPreferences: { contextIsolation: true, nodeIntegration: false, sandbox: true },
  });
  const wc = view.webContents;
  const entry = { view, wc, rect: null };
  views.set(paneId, entry);
  win.contentView.addChildView(view);
  view.setVisible(false);
  hookInput(wc, paneId);

  const push = () => send('web:state', {
    paneId, url: wc.getURL(), title: wc.getTitle(), loading: wc.isLoading(),
    canBack: navBack(wc), canFwd: navFwd(wc),
  });
  ['did-start-loading', 'did-stop-loading', 'did-navigate', 'did-navigate-in-page', 'page-title-updated', 'dom-ready']
    .forEach((ev) => wc.on(ev, () => push()));
  wc.on('page-favicon-updated', (_e, icons) => send('web:favicon', { paneId, icon: icons[0] || '' }));
  wc.on('focus', () => send('web:focused', { paneId }));
  wc.on('did-fail-load', (_e, code, desc, url, isMain) => {
    if (!isMain || code === -3) return; // -3 = ユーザー操作による中断
    const html = `<meta charset=utf-8><body style="font:14px sans-serif;padding:40px;color:#444"><h2>ページを表示できません</h2><p>${String(url).replace(/</g, '&lt;')}</p><p>${desc} (${code})</p>`;
    wc.loadURL('data:text/html;charset=utf-8,' + encodeURIComponent(html)).catch(() => {});
  });

  // target=_blank などの新規タブ要求は分割ペインで開く。
  // window.open(features付き)のポップアップ(OAuthログイン等)は opener を保つため本物のウィンドウで開く。
  guardGoogleSignin(wc, paneId, false);
  wc.setWindowOpenHandler(({ url, disposition, features }) => {
    if (disposition === 'new-window' && features && isGoogleSignin(url) && appembed.supported && appembed.hasBrowser()) {
      send('web:to-chromium', { paneId, url: wc.getURL(), popup: true });
      return { action: 'deny' };
    }
    if (disposition === 'new-window' && features) {
      return { action: 'allow', overrideBrowserWindowOptions: { autoHideMenuBar: true } };
    }
    send('web:open-request', { fromPaneId: paneId, url });
    return { action: 'deny' };
  });

  wc.on('did-create-window', (child) => { spoofChrome(child.webContents); guardGoogleSignin(child.webContents, paneId, true); });
  // 先に空ページを読んで描画プロセスを作り、Client Hintsを上書きしてから本当のURLへ進む
  wc.loadURL('about:blank').catch(() => {})
    .then(() => spoofChrome(wc))
    .then(() => { if (!wc.isDestroyed()) wc.loadURL(normalizeInput(input)).catch(() => {}); });
}

ipcMain.on('web:create', (_e, paneId, url) => createWeb(paneId, url));
ipcMain.on('web:bounds', (_e, paneId, rect) => {
  const e = views.get(paneId);
  if (!e) return;
  e.rect = rect;
  applyView(e);
});
ipcMain.on('web:suspend', (_e, v) => { suspended = !!v; views.forEach(applyView); appembed.setSuspended(suspended); });
ipcMain.on('web:cmd', (_e, paneId, cmd, arg) => {
  const e = views.get(paneId);
  if (!e) return;
  const wc = e.wc;
  if (cmd === 'navigate') wc.loadURL(normalizeInput(arg)).catch(() => {});
  else if (cmd === 'back') { if (navBack(wc)) (wc.navigationHistory ? wc.navigationHistory.goBack() : wc.goBack()); }
  else if (cmd === 'forward') { if (navFwd(wc)) (wc.navigationHistory ? wc.navigationHistory.goForward() : wc.goForward()); }
  else if (cmd === 'reload') wc.reload();
  else if (cmd === 'stop') wc.stop();
  else if (cmd === 'focus') wc.focus();
});
ipcMain.on('web:destroy', (_e, paneId) => {
  const e = views.get(paneId);
  if (!e) return;
  views.delete(paneId);
  try { win?.contentView.removeChildView(e.view); } catch {}
  try { e.wc.close(); } catch {}
});

// ---------------------------------------------------------------- app embed
appembed.onEvent((ev, paneId, extra) => send('app:' + ev, { paneId, ...extra }));
ipcMain.handle('app:supported', () => appembed.supported);
ipcMain.handle('app:browser-cmd', (_e, url, cls) => appembed.browserCmd(url, cls));
ipcMain.handle('app:list', () => appembed.listApps());
ipcMain.handle('app:launch', (_e, paneId, cmd, cls) => appembed.launch(paneId, cmd, cls));
ipcMain.on('app:bounds', (_e, paneId, rect) => appembed.setBounds(paneId, rect));
ipcMain.on('app:close', (_e, paneId) => appembed.close(paneId));
// 取り込まずに普通のウィンドウとして起動する(設定「アプリを開く方法」が「別ウィンドウ」のとき)
ipcMain.handle('app:spawn', (_e, cmd) => {
  if (typeof cmd !== 'string' || !cmd.trim()) return false;
  try {
    const p = spawn('/bin/sh', ['-c', cmd], { detached: true, stdio: 'ignore', env: appembed.childEnv(), cwd: require('os').homedir() });
    p.on('error', () => {});
    p.unref();
    return true;
  } catch { return false; }
});

// ---------------------------------------------------------------- file system
ipcMain.handle('fs:list', async (_e, dir, showHidden) => {
  try {
    const abs = path.resolve(dir || os.homedir());
    const dirents = await fs.promises.readdir(abs, { withFileTypes: true });
    const entries = await Promise.all(
      dirents.filter((d) => showHidden || !d.name.startsWith('.')).map(async (d) => {
        const full = path.join(abs, d.name);
        let size = 0, mtime = 0, isDir = d.isDirectory();
        try {
          const st = await fs.promises.stat(full);
          size = st.size; mtime = st.mtimeMs; isDir = st.isDirectory();
        } catch {}
        return { name: d.name, full, isDir, size, mtime };
      }),
    );
    entries.sort((a, b) => (b.isDir - a.isDir) || a.name.localeCompare(b.name, undefined, { numeric: true, sensitivity: 'base' }));
    const parent = path.dirname(abs);
    return { ok: true, path: abs, parent: parent === abs ? null : parent, entries };
  } catch (e) {
    return { ok: false, error: e.message, path: dir };
  }
});
ipcMain.handle('fs:open', (_e, p) => shell.openPath(p));
ipcMain.handle('fs:home', () => os.homedir());
ipcMain.handle('fs:drives', async () => {
  const out = [{ label: 'ホーム', path: os.homedir() }];
  if (process.platform === 'win32') {
    for (let c = 65; c <= 90; c++) {
      const p = String.fromCharCode(c) + ':\\';
      try { await fs.promises.access(p); out.push({ label: p, path: p }); } catch {}
    }
  } else {
    out.push({ label: '/', path: '/' });
    for (const base of ['/mnt', '/media', path.join('/run/media', os.userInfo().username), path.join('/media', os.userInfo().username)]) {
      try {
        for (const d of await fs.promises.readdir(base, { withFileTypes: true })) {
          if (d.isDirectory()) out.push({ label: path.join(base, d.name), path: path.join(base, d.name) });
        }
      } catch {}
    }
  }
  return out;
});

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
    send('dl:saved', { id: d.id, path: dest });
  } catch (e) {
    send('dl:error', { id: d.id, message: e.message });
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
    send('dl:start', {
      id, filename, total: item.getTotalBytes(), fromPaneId,
      defaultDir: config.get('behavior.downloadDir') || app.getPath('downloads'),
    });
    item.on('updated', () => send('dl:progress', { id, received: item.getReceivedBytes(), total: item.getTotalBytes() }));
    item.once('done', (_ev, state) => { d.done = true; d.state = state; send('dl:done', { id, state }); finalizeDownload(d); });
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
  const sys = status.init({ send, ipcMain, onVolumeChange: (v) => osd.show('volume', v.percent, v.muted) });
  quick.init({ ipcMain, refreshStatus: sys?.refresh, onSelfChange: () => osd.quiet() });
  sessionCtl.init({ ipcMain });
  if (process.platform === 'linux') {
    media.init({ ipcMain, send });
    // どのアプリが前面でも効くショートカット(KWinスクリプト経由)。
    // 音量キーは kglobalaccel 上で kmix(plasmashell)の持ち物のままなので PRELUDE は受けられない(docs/shell-design.md 参照)
    keys.load({
      launcher: { key: config.get('shortcuts.globalLauncher'), raise: true },
      brightnessReset: { key: 'Meta+Shift+B', raise: false }, // 緊急用: 画面が暗すぎるとき
    });
    bus.onReport('shortcut', (name) => {
      if (name === 'launcher') send('shortcut', { action: 'launcher', paneId: null });
      else if (name === 'brightnessReset') osd.resetBrightness();
    });
    osd.init();
    tray.init({ ipcMain, onItems: (list) => send('tray:items', list) });
    // 通知サーバ。トーストは別窓(オーバーレイ)に、履歴と未読数は本体の通知センターに送る
    overlay.init();
    ipcMain.handle('overlay:theme', () => config.data.appearance);
    notify.init({
      ipcMain, file: path.join(app.getPath('userData'), 'notifications.json'),
      onToasts: (list) => overlay.send('notify', 'notify:toasts', list),
      onState: (s) => send('notify:state', s),
    });
  }
  createWindow();
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
app.on('will-quit', () => { qlog('will-quit'); appembed.shutdown(); keys.shutdown(); overlay.shutdown(); bus.shutdown(); qlog('will-quit done'); });
app.on('quit', () => qlog('quit'));
process.on('exit', () => qlog('process exit'));
