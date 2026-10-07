'use strict';
// アプリの取り込み(Linux / KDE Plasma 6 の Wayland 専用)。
// Waylandでは他アプリのウィンドウを子にできないので、KWinスクリプトで
// 「ペインの位置・大きさに外部アプリのウィンドウを重ねて配置」します(枠なし・最前面)。
// スクリプトは状態(各ペインの矩形・PID)を埋め込んだ常駐スクリプトとして都度読み直します。
const { spawn, execFile } = require('child_process');
const fs = require('fs');
const os = require('os');
const path = require('path');

const SCRIPT_NAME = 'prelude-embed';
const supported = process.platform === 'linux' && /kde/i.test(process.env.XDG_CURRENT_DESKTOP || '');

const panes = new Map(); // paneId -> { proc, cmd, cls, rect }
let suspended = false;
// PRELUDE自身を「疑似全画面」にするための状態。本物のKWin全画面だと、アクティブ中は取り込んだアプリより必ず上に出てしまう
const shell = { fs: false, restore: null };
let notify = () => {};
let scriptFile = null;
let reloading = false;
let again = false;
let timer = null;

const gdbus = (objPath, method, ...args) => new Promise((resolve) => {
  execFile('gdbus', ['call', '--session', '--dest', 'org.kde.KWin', '--object-path', objPath, '--method', method, ...args],
    (err, out) => {
      if (process.env.PRELUDE_DEBUG) fs.appendFileSync('/tmp/prelude-embed.log', `${new Date().toISOString()} ${method.split('.').pop()} ${args.map((x) => path.basename(String(x))).join(' ')} => ${err ? 'ERR ' + err.message : out.trim()}\n`);
      resolve(err ? null : out);
    });
});

// pid の子孫を /proc から集める(sh -c やラッパー経由で起動されたアプリのウィンドウも拾うため)
function descendants(root) {
  const kids = new Map();
  try {
    for (const d of fs.readdirSync('/proc')) {
      if (!/^\d+$/.test(d)) continue;
      try {
        const st = fs.readFileSync(`/proc/${d}/stat`, 'utf8');
        const ppid = Number(st.slice(st.lastIndexOf(')') + 2).split(' ')[1]);
        if (!kids.has(ppid)) kids.set(ppid, []);
        kids.get(ppid).push(Number(d));
      } catch {}
    }
  } catch {}
  const out = [root];
  for (let i = 0; i < out.length; i++) out.push(...(kids.get(out[i]) || []));
  return out;
}

const uptime = () => Number(fs.readFileSync('/proc/uptime', 'utf8').split(' ')[0]);
const TICKS = 100; // CLK_TCK(Linuxの標準値)

// kate のように自分で fork して親から切り離されるアプリは、子孫をたどっても見つからない。
// そこで「起動後に始まったプロセスで、コマンド名と一致するもの」も同じアプリとして扱う。
function freshPids(e) {
  const base = String(e.cmd).trim().split(/\s+/)[0].replace(/['"]/g, '').split('/').pop().toLowerCase();
  const out = [];
  if (!base) return out;
  try {
    for (const d of fs.readdirSync('/proc')) {
      if (!/^\d+$/.test(d)) continue;
      try {
        const st = fs.readFileSync(`/proc/${d}/stat`, 'utf8');
        const comm = st.slice(st.indexOf('(') + 1, st.lastIndexOf(')')).toLowerCase();
        const start = Number(st.slice(st.lastIndexOf(')') + 2).split(' ')[19]) / TICKS;
        if (start >= e.launchedAt - 0.5 && comm && (base.startsWith(comm) || comm.startsWith(base))) out.push(Number(d));
      } catch {}
    }
  } catch {}
  return out;
}
const pidsOf = (e) => [...new Set([...(e.proc?.pid ? descendants(e.proc.pid) : []), ...freshPids(e)])];

function buildScript() {
  order = [...panes.keys()];
  // 最小化から戻したウィンドウは、アクティブにしないと描画されず空白のままになる(KWinの挙動)。
  // 再表示した直後の短い間だけ activate させる。
  for (const e of panes.values()) {
    const now = !!e.rect && e.rect.width > 0 && e.rect.height > 0 && !suspended;
    if (now && !e.shown) {
      e.activate = true;
      setTimeout(() => { e.activate = false; }, 700);
      setTimeout(() => schedule(0), 300);
    }
    e.shown = now;
  }
  const state = {
    suspended,
    shell: { fs: shell.fs, restore: shell.restore },
    panes: [...panes.values()].map((p) => ({
      pids: pidsOf(p),
      cls: (p.cls || '').toLowerCase(),
      rect: p.rect && p.rect.width > 0 && p.rect.height > 0 ? p.rect : null,
      activate: !!p.activate,
    })),
  };
  return `
var STATE = ${JSON.stringify(state)};
var EL_PID = ${process.pid};
function electron() {
  var best = null, area = 0;
  workspace.windowList().forEach(function (w) {
    if (w.pid !== EL_PID || !w.normalWindow) return;
    var a = w.frameGeometry.width * w.frameGeometry.height;
    if (a > area) { best = w; area = a; }
  });
  return best;
}
function paneOf(w) {
  if (w.pid === EL_PID || !w.normalWindow) return null;
  var c = (w.resourceClass || '').toLowerCase(), n = (w.resourceName || '').toLowerCase();
  for (var i = 0; i < STATE.panes.length; i++) {
    var p = STATE.panes[i];
    if (p.pids.indexOf(w.pid) >= 0) return p;
    if (p.cls && (c.indexOf(p.cls) >= 0 || n.indexOf(p.cls) >= 0)) return p;
  }
  return null;
}
function apply(w) {
  var p = paneOf(w);
  if (!p) return;
  var el = electron();
  if (!el || el.minimized || STATE.suspended || !p.rect) { w.minimized = true; return; }
  var g = el.clientGeometry;
  w.minimized = false;
  w.noBorder = true;
  w.keepAbove = true;
  w.skipTaskbar = true; w.skipPager = true; w.skipSwitcher = true;
  var geo = { x: Math.round(g.x + p.rect.x), y: Math.round(g.y + p.rect.y), width: Math.round(p.rect.width), height: Math.round(p.rect.height) };
  w.frameGeometry = geo;
  if (p.activate) workspace.activeWindow = w;
}
function applyAll() { workspace.windowList().forEach(apply); }
workspace.windowAdded.connect(function (w) { w.windowShown && w.windowShown.connect(function () { apply(w); }); apply(w); if (w.pid === EL_PID) applyShell(); });
function applyShell() {
  var el = electron();
  if (!el || (!STATE.shell.fs && !STATE.shell.restore)) return;
  var r = el.output ? el.output.geometry : workspace.virtualScreenGeometry;
  var g = el.frameGeometry, t;
  if (STATE.shell.fs) {
    el.noBorder = true;
    el.keepAbove = true; // パネルより上に出す(取り込んだアプリも keepAbove なので同じ層で並ぶ)
    t = { x: r.x, y: r.y, width: r.width, height: r.height };
  } else {
    el.keepAbove = false;
    var s = STATE.shell.restore;
    t = { x: r.x + Math.round((r.width - s.width) / 2), y: r.y + Math.round((r.height - s.height) / 2), width: s.width, height: s.height };
  }
  if (g.x !== t.x || g.y !== t.y || g.width !== t.width || g.height !== t.height) el.frameGeometry = t;
}
// PRELUDEがアクティブになると同じ層の最前面に来るので、取り込んだアプリを前に出し直す
function raiseEmbedded() {
  workspace.windowList().forEach(function (w) {
    var p = paneOf(w);
    if (p && p.rect && !w.minimized) workspace.raiseWindow(w);
  });
}
// 各ペインのウィンドウ数をNode側へ知らせる(ウィンドウが閉じたら取り込みを終えるため)。
// 宛先のサービスは無く、dbus-monitor が通信を横取りして読む
function report() {
  var counts = STATE.panes.map(function () { return 0; });
  workspace.windowList().forEach(function (w) {
    var p = paneOf(w);
    if (p) counts[STATE.panes.indexOf(p)]++;
  });
  callDBus('org.prelude.embed', '/', 'org.prelude.embed', 'windows', JSON.stringify(counts));
}
function applyEverything() { applyShell(); applyAll(); raiseEmbedded(); report(); }
workspace.windowRemoved.connect(function () { report(); });
workspace.windowActivated.connect(function (w) { if (w && w.pid === EL_PID) raiseEmbedded(); });
var el0 = electron();
if (el0) {
  el0.frameGeometryChanged.connect(applyEverything);
  el0.minimizedChanged.connect(applyEverything);
}
applyEverything();
`;
}

async function reload() {
  if (!supported) return;
  if (reloading) { again = true; return; }
  reloading = true;
  try {
    if (!scriptFile) scriptFile = path.join(fs.mkdtempSync(path.join(os.tmpdir(), 'prelude-embed-')), 'embed.js');
    await gdbus('/Scripting', 'org.kde.kwin.Scripting.unloadScript', SCRIPT_NAME);
    if (!panes.size && !shell.fs && !shell.restore) return;
    fs.writeFileSync(scriptFile, buildScript());
    const out = await gdbus('/Scripting', 'org.kde.kwin.Scripting.loadScript', scriptFile, SCRIPT_NAME);
    const id = out && /(\d+)/.exec(out)?.[1];
    if (id != null) await gdbus(`/Scripting/Script${id}`, 'org.kde.kwin.Script.run');
  } finally {
    reloading = false;
    if (again) { again = false; schedule(); }
  }
}

// KWinスクリプトからの報告(ウィンドウ数)を受け取る。dbus-monitor が無い環境では従来のPID監視にフォールバックする
let monitorOk = false;
let order = []; // 直近のスクリプトでのペインの並び(報告の添字に対応)
function startMonitor() {
  if (!supported || monitorOk) return;
  let mon;
  try { mon = spawn('dbus-monitor', ['--session', "type='method_call',interface='org.prelude.embed'"], { stdio: ['ignore', 'pipe', 'ignore'] }); } catch { return; }
  mon.on('error', () => { monitorOk = false; });
  mon.on('exit', () => { monitorOk = false; });
  monitorOk = true;
  let buf = '', next = false;
  mon.stdout.on('data', (d) => {
    buf += d;
    const lines = buf.split('\n');
    buf = lines.pop();
    for (const ln of lines) {
      if (/member=windows/.test(ln)) { next = true; continue; }
      const m = next && /string "(\[.*\])"/.exec(ln);
      if (!m) continue;
      next = false;
      let counts;
      try { counts = JSON.parse(m[1]); } catch { continue; }
      counts.forEach((n, i) => {
        const id = order[i];
        const e = id && panes.get(id);
        if (!e) return;
        if (n > 0) { e.hadWindow = true; e.gone = 0; return; }
        if (!e.hadWindow) return;
        // 一瞬の0(ウィンドウの作り直しなど)を避けるため、少し待ってもう一度報告させ、続けて0なら終了とみなす
        if (++e.gone === 1) setTimeout(() => schedule(0), 800);
        else if (e.gone === 2) notify('exited', id);
      });
    }
  });
  process.on('exit', () => { try { mon.kill(); } catch {} });
}

function schedule(ms = 40) {
  clearTimeout(timer);
  timer = setTimeout(() => { timer = null; reload(); }, ms);
}

function launch(paneId, cmd, cls) {
  if (!supported || panes.has(paneId) || !String(cmd || '').trim()) return false;
  startMonitor();
  const proc = spawn('/bin/sh', ['-c', cmd], { detached: true, stdio: 'ignore' });
  const e = { proc, cmd, cls: cls || '', rect: null, launchedAt: uptime(), t0: Date.now(), hadWindow: false, gone: 0 };
  panes.set(paneId, e);
  proc.on('error', () => {});
  proc.on('exit', () => {
    // 起動用の sh が終わっても、アプリ本体が生きていれば取り込み続ける。本体も居なくなったらペインを空に戻す
    let dead = 0;
    const t = setInterval(() => {
      if (panes.get(paneId) !== e) { clearInterval(t); return; }
      if (monitorOk && e.hadWindow) return; // ウィンドウの有無は報告で判断する
      if (Date.now() - e.t0 < 20000) return; // 既存プロセスへの相乗り起動などでウィンドウが遅れて出る分を待つ
      dead = pidsOf(e).filter((p) => p !== proc.pid).length ? 0 : dead + 1;
      if (dead >= 3) { clearInterval(t); notify('exited', paneId); }
    }, 1500);
  });
  // 起動直後はウィンドウが遅れて現れるので、30秒間は1秒おきに PID を取り直して配置し直す
  let n = 0;
  const warm = setInterval(() => { if (panes.get(paneId) !== e || ++n > 30) clearInterval(warm); else schedule(0); }, 1000);
  schedule();
  return true;
}

function setBounds(paneId, rect) {
  const e = panes.get(paneId);
  if (!e) return;
  e.rect = rect;
  schedule();
}

// PRELUDE自身の疑似全画面。on=false のときは normal={width,height} の大きさに戻す
function setShell(on, normal) {
  if (!supported) return;
  shell.fs = !!on;
  shell.restore = on ? null : normal || { width: 1400, height: 900 };
  let n = 0;
  const warm = setInterval(() => { if (++n > 10) clearInterval(warm); else schedule(0); }, 1000);
  if (!on) setTimeout(() => { shell.restore = null; schedule(0); }, 2500);
  schedule(0);
}

function setSuspended(v) {
  if (suspended === !!v) return;
  suspended = !!v;
  schedule(0);
}

function close(paneId) {
  const e = panes.get(paneId);
  if (!e) return;
  panes.delete(paneId);
  for (const pid of pidsOf(e).reverse()) { try { process.kill(pid, 'SIGTERM'); } catch {} }
  schedule();
}

function shutdown() {
  [...panes.keys()].forEach((id) => close(id));
  if (supported) gdbus('/Scripting', 'org.kde.kwin.Scripting.unloadScript', SCRIPT_NAME);
}

// .desktop ファイルからインストール済みアプリの一覧を作る
function listApps() {
  const dirs = [
    '/usr/share/applications', '/usr/local/share/applications',
    path.join(os.homedir(), '.local/share/applications'),
    '/var/lib/flatpak/exports/share/applications',
    path.join(os.homedir(), '.local/share/flatpak/exports/share/applications'),
  ];
  const map = new Map();
  for (const dir of dirs) {
    let files = [];
    try { files = fs.readdirSync(dir).filter((f) => f.endsWith('.desktop')); } catch { continue; }
    for (const f of files) {
      try {
        const sec = fs.readFileSync(path.join(dir, f), 'utf8').split(/\n\[/)[0];
        const get = (k) => new RegExp(`^${k}=(.*)$`, 'm').exec(sec)?.[1]?.trim();
        if (get('Type') !== 'Application' || get('NoDisplay') === 'true' || get('Hidden') === 'true') continue;
        const exec = (get('Exec') || '').replace(/%[a-zA-Z]/g, '').replace(/\s+/g, ' ').trim();
        if (!exec) continue;
        map.set(f, { name: get('Name') || f, exec, cls: (get('StartupWMClass') || '').toLowerCase() });
      } catch {}
    }
  }
  return [...map.values()].sort((a, b) => a.name.localeCompare(b.name));
}

// 実ブラウザ(Chromium系)をアプリモードで開くコマンド。ペインごとにウィンドウクラスを付けて、取り込み時に見分ける
function browserCmd(url, cls) {
  const bin = ['/snap/bin/chromium', '/usr/bin/chromium', '/usr/bin/chromium-browser', '/usr/bin/google-chrome-stable', '/usr/bin/google-chrome']
    .find((p) => fs.existsSync(p)) || 'chromium';
  // snap版は ~/snap/chromium/common 以外の隠しディレクトリに書けないので、そこに専用プロファイルを置く
  const profile = bin.startsWith('/snap/') ? path.join(os.homedir(), 'snap/chromium/common/prelude-profile') : path.join(os.homedir(), '.config/PRELUDE-electron/browser-profile');
  const q = (s) => `'${String(s).replace(/'/g, `'\\''`)}'`;
  // 既に同じプロファイルのChromiumが動いていると、新しいプロセスは作られず既存プロセスにウィンドウだけ増える。
  // PIDでは見つからないので、アプリモードのウィンドウクラス(chrome-<host>__<path>-Default)でも照合できるようにしておく
  let winClass = '';
  try { const u = new URL(url); winClass = `chrome-${u.host}__${u.pathname.replace(/^\//, '').replace(/\//g, '_')}-Default`.toLowerCase(); } catch {}
  return {
    cmd: `${q(bin)} --app=${q(url)} --user-data-dir=${q(profile)} --class=${q(cls)} --no-first-run --no-default-browser-check`,
    cls: winClass,
  };
}

module.exports = {
  browserCmd,
  supported, setShell, launch, setBounds, setSuspended, close, shutdown, listApps,
  onEvent: (cb) => { notify = cb; },
};
