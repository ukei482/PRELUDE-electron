'use strict';
// アプリの取り込み(Linux / KDE Plasma 6 の Wayland 専用)。
// Waylandでは他アプリのウィンドウを子にできないので、KWinスクリプトで
// 「ペインの位置・大きさに外部アプリのウィンドウを重ねて配置」します(枠なし・最前面)。
// スクリプトは状態(各ペインの矩形・PID)を埋め込んだ常駐スクリプトとして都度読み直します。
const { spawn, execFile } = require('child_process');
const bus = require('./sys/bus');
const apps = require('./sys/apps');
const fs = require('fs');
const os = require('os');
const path = require('path');

const { instanceTag, cleanShell } = require('./sys/util');
const SCRIPT_NAME = 'prelude-embed' + instanceTag; // 別プロファイルの PRELUDE(デバッグ用)はシェルのスクリプトを上書きしない
const supported = process.platform === 'linux' && /kde/i.test(process.env.XDG_CURRENT_DESKTOP || '');

const panes = new Map(); // paneId -> { proc, cmd, keys, rect, wids, owned, old, ... }
let suspended = false;
// PRELUDE自身を「疑似全画面」にするための状態。本物のKWin全画面だと、アクティブ中は取り込んだアプリより必ず上に出てしまう
const shell = { fs: false, restore: null };
let notify = () => {};
let scriptFile = null;
let reloading = false;
let again = false;
let timer = null;

const log = (msg) => { if (process.env.PRELUDE_DEBUG) fs.appendFileSync('/tmp/prelude-embed.log', `${new Date().toISOString()} ${msg}\n`); };
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
// ペインのプロセス: 起動した sh の子孫と、取り込んだウィンドウの持ち主(起動後に始まったものだけ)。
// ウィンドウがまだ無い間は「起動後に始まった同名のプロセス」も候補にする。ただし他のペインのものは除く
// (同じアプリを2つ開くと、先のペインが後のペインのプロセスまで自分のものにしてしまうため)
function pidsOf(e) {
  const out = new Set([...(e.proc?.pid ? descendants(e.proc.pid) : []), ...e.owned]);
  if (!e.wids.length) {
    const others = new Set();
    for (const o of panes.values()) if (o !== e) { o.owned.forEach((p) => others.add(p)); if (o.proc?.pid) descendants(o.proc.pid).forEach((p) => others.add(p)); }
    for (const p of freshPids(e)) if (!others.has(p)) out.add(p);
  }
  return [...out];
}

// ランチャーの起動コマンドしか無くても照合できるように、.desktop の情報を引く(一覧は1分だけ使い回す)
let appCache = null;
function appFor(cmd) {
  if (!appCache || Date.now() - appCache.t > 60000) appCache = { t: Date.now(), list: apps.list() };
  return appCache.list.find((a) => a.exec === String(cmd).trim());
}
// 起動したアプリのウィンドウを見分ける名前(小文字)。.desktop の id(Wayland の app_id と同じことが多い)・StartupWMClass・実行ファイル名。
// 既に動いているプロセスに起動を任せるアプリ(Zed・システム設定・Dolphin など)は PID では見つからないので、これで照合する
const WRAPPERS = new Set(['sh', 'bash', 'env', 'nohup', 'setsid', 'pkexec', 'sudo', 'lxqt-sudo', 'xdg-open', 'gtk-launch', 'kioclient', 'kioclient5', 'flatpak', 'snap', 'dbus-launch', 'x-terminal-emulator']);
function keysOf(cmd, cls) {
  if (/^chrome-/.test(cls || '')) return [cls.toLowerCase()]; // 実ブラウザのペインは、そのペイン専用のクラスだけで照合する
  const keys = new Set();
  if (cls) keys.add(cls.toLowerCase());
  const a = appFor(cmd);
  if (a?.cls) keys.add(a.cls);
  if (a?.id) keys.add(a.id.toLowerCase());
  const base = String(cmd).trim().split(/\s+/)[0].replace(/['"]/g, '').split('/').pop().toLowerCase();
  if (base && !WRAPPERS.has(base)) keys.add(base);
  return [...keys];
}

function buildScript() {
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
    panes: [...panes.entries()].map(([id, p]) => ({
      id,
      pids: pidsOf(p),
      keys: p.keys,
      wids: p.wids,
      old: p.old, // 起動前からあったウィンドウ(名前での照合の対象外)
      adopt: !!p.adopt, // 既存のウィンドウも取り込んでよい(起動が既存のプロセスに引き継がれて終わった)
      rect: p.rect && p.rect.width > 0 && p.rect.height > 0 ? p.rect : null,
      activate: !!p.activate,
    })),
    close: closeWids, // 閉じたペインのウィンドウ(プロセスを殺さずに閉じる)
    closePids: closePids, // 終了の合図を送れなかったプロセス(snap の Chromium など)。そのウィンドウを閉じる
    release: releaseWids, // 閉じたペインが借りていた、起動前からあったウィンドウ(閉じずに普通の窓に戻す)
  };
  closeWids = [];
  releaseWids = [];
  closePids = [];
  return `
var STATE = ${JSON.stringify(state)};
var EL_PID = ${process.pid};
// 報告先(bus.NAME)を自分が持っているときだけ報告する。持っていない(別の PRELUDE がシェルとして動いている)ときに報告すると、
// ペインの番号が重なって、そちらのペインに別のウィンドウが混ざる
var REPORT = ${monitorOk};
function send(kind, data) { if (REPORT) callDBus(${JSON.stringify(bus.NAME)}, '/org/prelude/Shell', 'org.prelude.Shell', 'Report', kind, JSON.stringify(data)); }
function electron(skip) {
  var best = null, area = 0;
  workspace.windowList().forEach(function (w) {
    if (w === skip || w.pid !== EL_PID || !w.normalWindow) return;
    var a = w.frameGeometry.width * w.frameGeometry.height;
    if (a > area) { best = w; area = a; }
  });
  return best;
}
function wid(w) { return String(w.internalId); }
function keyHit(p, w) {
  var c = (w.resourceClass || '').toLowerCase(), n = (w.resourceName || '').toLowerCase(), d = (w.desktopFileName || '').toLowerCase();
  var ds = d.split('.').pop();
  for (var i = 0; i < p.keys.length; i++) {
    var k = p.keys[i];
    if (k && (k === c || k === n || k === d || k === ds)) return true;
  }
  return false;
}
// ウィンドウをペインのものにする。以後はウィンドウID で照合し続ける(Node 側にも知らせて、次のスクリプトに引き継ぐ)
function claim(p, w, how) {
  p.wids.push(wid(w));
  send('claim', { pane: p.id, wid: wid(w), pid: w.pid, how: how });
  return p;
}
// ウィンドウの属するペイン。順に: 取り込み済みのウィンドウID → 起動したプロセスのPID → (まだウィンドウが無いペインだけ)起動後に現れた同じアプリのウィンドウ
function paneOf(w, activated) {
  if (w.pid === EL_PID || !w.normalWindow) return null;
  var id = wid(w), i, p;
  for (i = 0; i < STATE.panes.length; i++) if (STATE.panes[i].wids.indexOf(id) >= 0) return STATE.panes[i];
  for (i = 0; i < STATE.panes.length; i++) if (STATE.panes[i].pids.indexOf(w.pid) >= 0) return claim(STATE.panes[i], w, 'pid');
  for (i = 0; i < STATE.panes.length; i++) {
    p = STATE.panes[i];
    // 既に開いているウィンドウを前に出すだけのアプリ(システム設定など)は、その既存のウィンドウを取り込む
    // (Wayland では前に出す要求が拒否されて「注意を要求」になるだけのことが多いので、アクティブになるのは待たない)
    if (!p.wids.length && p.old && (activated || p.adopt || p.old.indexOf(id) < 0) && keyHit(p, w)) return claim(p, w, p.old.indexOf(id) >= 0 ? 'adopt' : 'key');
  }
  return null;
}
function apply(w) {
  if (orphan) return;
  var p = paneOf(w);
  if (!p) return;
  hookCaption(w);
  var el = electron();
  if (!el || el.minimized || STATE.suspended || !p.rect) { w.minimized = true; return; }
  var g = el.clientGeometry;
  w.minimized = false;
  w.keepAbove = true;
  w.skipTaskbar = true; w.skipPager = true; w.skipSwitcher = true;
  var geo = { x: Math.round(g.x + p.rect.x), y: Math.round(g.y + p.rect.y), width: Math.round(p.rect.width), height: Math.round(p.rect.height) };
  if (w.transient) { placeChild(w, geo); return; }
  w.noBorder = true;
  w.frameGeometry = geo;
  if (p.activate) workspace.activeWindow = w;
}
// 取り込んだアプリの子の窓(設定・保存・確認などのダイアログ)。ペインいっぱいに広げず、枠を付けたまま自分の大きさで、ペインの中に置く。
// ペインからはみ出しているときだけ中央に置き直す(ペインの中で動かした位置は保つ)。ペインより大きければ縮める
function placeChild(w, pane) {
  w.noBorder = false;
  var f = w.frameGeometry;
  var width = Math.min(f.width, pane.width), height = Math.min(f.height, pane.height);
  var inside = f.x >= pane.x && f.y >= pane.y && f.x + f.width <= pane.x + pane.width && f.y + f.height <= pane.y + pane.height;
  if (inside && width === f.width && height === f.height) return;
  w.frameGeometry = { x: Math.round(pane.x + (pane.width - width) / 2), y: Math.round(pane.y + (pane.height - height) / 2), width: width, height: height };
}
function applyAll() { workspace.windowList().forEach(apply); }
// どのペインにも属さない新しいウィンドウ(ログイン用のポップアップ、起動済みの Chromium に引き継がれた窓、アプリのダイアログなど)は、
// 最前面の PRELUDE の後ろに隠れて見えなくなるので、同じ層に上げて前面に出す。追加された時の1回だけ(既存の窓には触らない)
var surfaced = [];
function surface(w) {
  if (orphan || !STATE.shell.fs || w.pid === EL_PID || !(w.normalWindow || w.dialog) || paneOf(w)) return;
  w.keepAbove = true;
  surfaced.push(w);
  workspace.raiseWindow(w);
  workspace.activeWindow = w;
}
// PRELUDE が落ちて(クラッシュ・強制終了)本体のウィンドウが消えたら、取り込んでいた窓を普通の窓に戻す。
// Node 側の release は走らないので、KWin に残っているこのスクリプトが後始末をする
// (放っておくと「最小化・タスクバーにも切り替えにも出ない」窓が残り、そのアプリを開けなくなる)
var orphan = false;
function releaseAll() {
  orphan = true;
  workspace.windowList().forEach(function (w) {
    if (w.pid === EL_PID) return;
    var id = wid(w), mine = false;
    STATE.panes.forEach(function (p) { if (p.wids.indexOf(id) >= 0 || p.pids.indexOf(w.pid) >= 0) mine = true; });
    if (mine) { w.keepAbove = false; w.noBorder = false; w.skipTaskbar = false; w.skipPager = false; w.skipSwitcher = false; w.minimized = false; }
    else if (surfaced.indexOf(w) >= 0) w.keepAbove = false;
  });
}
workspace.windowAdded.connect(function (w) {
  w.windowShown && w.windowShown.connect(function () { apply(w); });
  apply(w);
  if (w.pid === EL_PID) applyShell(); else if (!paneOf(w)) surface(w);
  report();
});
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
// 子の窓(ダイアログ)は本体より前に出すので、本体を先に、子の窓を後に上げる
function raiseEmbedded() {
  [false, true].forEach(function (child) {
    workspace.windowList().forEach(function (w) {
      var p = paneOf(w);
      if (p && p.rect && !w.minimized && !!w.transient === child) workspace.raiseWindow(w);
    });
  });
}
// 各ペインのウィンドウ数をNode側へ知らせる(ウィンドウが閉じたら取り込みを終えるため)。
// 宛先は PRELUDE の D-Bus サービス(sys/bus.js)
// ペインごとの窓の数と題名(最初の窓の題名。Chromium なら今のページ名)
function report() {
  var counts = {}, titles = {}, all = [];
  STATE.panes.forEach(function (p) { counts[p.id] = 0; });
  workspace.windowList().forEach(function (w) {
    all.push(wid(w));
    var p = paneOf(w);
    if (!p) return;
    counts[p.id]++;
    if (!titles[p.id] && w.caption && !w.transient) titles[p.id] = w.caption; // ダイアログの題名ではなく本体の題名
  });
  send('windows', { counts: counts, titles: titles, all: all });
}
// 取り込んだ窓の題名が変わったら知らせ直す(Chromium のページ移動・エディタのファイル切り替えなど)。窓ごとに1度だけつなぐ
var hooked = {};
function hookCaption(w) {
  var id = wid(w);
  if (hooked[id] || !w.captionChanged) return;
  hooked[id] = true;
  w.captionChanged.connect(function () { if (paneOf(w)) report(); });
}
function applyEverything() { applyShell(); applyAll(); raiseEmbedded(); report(); }
workspace.windowRemoved.connect(function (w) {
  if (!orphan && w.pid === EL_PID && w.normalWindow && !electron(w)) { releaseAll(); return; }
  report();
});
workspace.windowActivated.connect(function (w) {
  if (!w || orphan) return;
  if (w.pid === EL_PID) { raiseEmbedded(); return; }
  if (paneOf(w, true)) { apply(w); report(); }
});
workspace.windowList().forEach(function (w) {
  var id = wid(w);
  if (STATE.close.indexOf(id) >= 0 || STATE.closePids.indexOf(w.pid) >= 0) w.closeWindow();
  else if (STATE.release.indexOf(id) >= 0) {
    w.keepAbove = false; w.noBorder = false; w.skipTaskbar = false; w.skipPager = false; w.skipSwitcher = false; w.minimized = false;
  }
});
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
    if (!panes.size && !shell.fs && !shell.restore && !waiters.length && !closeWids.length && !releaseWids.length && !closePids.length) return;
    fs.writeFileSync(scriptFile, buildScript());
    const out = await gdbus('/Scripting', 'org.kde.kwin.Scripting.loadScript', scriptFile, SCRIPT_NAME);
    const id = out && /(-?\d+)/.exec(out)?.[1];
    // 読み込んだ番号の Script{id}.run は使わない。KWin は番号を「今の本数」で振るので、途中のスクリプトを外したあとに読み込むと
    // 既にある別のスクリプトと番号が重なり、run がそちらに届いて新しいスクリプトが動かないことがある。start は未実行のものを全部動かす
    if (id != null && Number(id) >= 0) await gdbus('/Scripting', 'org.kde.kwin.Scripting.start');
  } finally {
    reloading = false;
    if (again) { again = false; schedule(); }
  }
}

// KWinスクリプトからの報告(ウィンドウ数)を受け取る。D-Bus に繋がらない環境では従来のPID監視にフォールバックする
let monitorOk = false;
let known = []; // 直近の報告にあった全ウィンドウのID
let waiters = []; // 次の報告を待っている launch
let closeWids = [];
let closePids = [];
// 終了の合図(SIGTERM)を送る。snap のアプリ(Chromium など)は AppArmor で拒まれる(EACCES・EPERM)ので、そのウィンドウを閉じて終わらせる
function terminate(pid) {
  try { process.kill(pid, 'SIGTERM'); } catch (e) { log(`terminate ${pid}: ${e.code}`); if (e.code === 'EPERM' || e.code === 'EACCES') closePids.push(pid); }
}
let releaseWids = [];

// 取り込み中のウィンドウIDの控え。PRELUDE が後始末できずに終わったとき、次の起動で普通の窓に戻すために使う
// (KWin スクリプト側でも戻すが、スクリプトの読み直しの合間に落ちた場合の保険)。ログアウトで消える場所に置く
const widsFile = path.join(process.env.XDG_RUNTIME_DIR || os.tmpdir(), `prelude-embed-wids${instanceTag}.json`);
function saveWids() {
  const all = [...panes.values()].flatMap((e) => e.wids);
  try { if (all.length) fs.writeFileSync(widsFile, JSON.stringify(all)); else fs.rmSync(widsFile, { force: true }); } catch {}
}
function recoverWids() {
  if (!supported) return;
  let left = [];
  try { left = JSON.parse(fs.readFileSync(widsFile, 'utf8')); } catch { return; }
  try { fs.rmSync(widsFile, { force: true }); } catch {}
  if (!Array.isArray(left) || !left.length) return;
  log(`recover ${left.length} window(s)`);
  releaseWids.push(...left.map(String)); // 前回のアプリは閉じずに返す(保存していない作業があるかもしれない)
  schedule(0);
}
recoverWids();

function startMonitor() {
  if (!supported || monitorOk || !bus.get()) return;
  monitorOk = true;
  bus.onReport('claim', (c) => {
    const e = c && panes.get(c.pane);
    if (!e || e.wids.includes(c.wid)) return;
    e.wids.push(c.wid);
    if (c.how === 'pid') e.owned.add(c.pid); // 起動後に始まったプロセス。閉じるときに終了させてよい
    if (c.how === 'adopt') e.adopted.push(c.wid); // 起動前からあったウィンドウ。閉じるときは閉じずに返す
    saveWids();
    log(`claim ${c.pane} ${c.how} pid=${c.pid}`);
  });
  bus.onReport('windows', (r) => {
    if (!r || !r.counts) return;
    known = r.all || [];
    waiters.splice(0).forEach((f) => f(known));
    Object.entries(r.titles || {}).forEach(([id, t]) => {
      const e = panes.get(id);
      if (e && typeof t === 'string' && e.title !== t) { e.title = t; notify('title', id, { title: t.slice(0, 300) }); }
    });
    Object.entries(r.counts).forEach(([id, n]) => {
      const e = panes.get(id);
      if (!e) return;
      if (n > 0) { e.hadWindow = true; e.gone = 0; return; }
      if (!e.hadWindow) return;
      // 一瞬の0(ウィンドウの作り直しなど)を避けるため、少し待ってもう一度報告させ、続けて0なら終了とみなす
      if (++e.gone === 1) setTimeout(() => schedule(0), 800);
      else if (e.gone === 2) notify('exited', id, { hadWindow: true });
    });
  });
}

function schedule(ms = 40) {
  clearTimeout(timer);
  timer = setTimeout(() => { timer = null; reload(); }, ms);
}

// 今あるウィンドウの一覧(次の報告)を待つ。報告が来なければ直近のもの
function snapshot() {
  return new Promise((resolve) => {
    waiters.push(resolve);
    schedule(0);
    setTimeout(() => { waiters = waiters.filter((f) => f !== resolve); resolve(known); }, 800);
  });
}

// 子プロセスに渡す環境。Electron が自分用に足した変数は外す
// (CHROME_DESKTOP が残ると、Chromium/Electron 製のアプリが自分を PRELUDE だと名乗ってしまう)
function childEnv() {
  const env = { ...process.env };
  delete env.CHROME_DESKTOP;
  delete env.NO_AT_BRIDGE;
  return env;
}

const starting = new Set();
async function launch(paneId, cmd, cls) {
  if (!supported || panes.has(paneId) || starting.has(paneId) || !String(cmd || '').trim()) return false;
  startMonitor();
  starting.add(paneId);
  // 起動前からあるウィンドウを覚えておく(名前で照合するとき、ユーザーが別に開いていた同じアプリの窓を取らないため)
  const old = monitorOk ? [...(await snapshot())] : null;
  starting.delete(paneId);
  if (panes.has(paneId)) return false;
  const proc = spawn(...cleanShell(cmd), { detached: true, stdio: 'ignore', env: childEnv(), cwd: os.homedir() });
  const e = { proc, cmd, keys: keysOf(cmd, cls), wids: [], owned: new Set(), adopted: [], old, rect: null, launchedAt: uptime(), t0: Date.now(), hadWindow: false, gone: 0 };
  log(`launch ${paneId} ${cmd} keys=${e.keys}`);
  panes.set(paneId, e);
  proc.on('error', () => {});
  proc.on('exit', () => {
    // 新しいウィンドウを出さずにすぐ終わった = 既に動いているプロセスに引き継いだ。その既存のウィンドウを取り込む
    // (自分で fork して切り離されるアプリ(kate など)は、起動後に始まったプロセスが残っているので対象外)
    setTimeout(() => {
      if (panes.get(paneId) !== e || e.wids.length || pidsOf(e).some((p) => p !== proc.pid)) return;
      e.adopt = true;
      log(`adopt ${paneId}`);
      schedule(0);
    }, 1500);
    // 起動用の sh が終わっても、アプリ本体が生きていれば取り込み続ける。本体も居なくなったらペインを空に戻す
    let dead = 0;
    const t = setInterval(() => {
      if (panes.get(paneId) !== e) { clearInterval(t); return; }
      if (monitorOk && e.hadWindow) return; // ウィンドウの有無は報告で判断する
      if (Date.now() - e.t0 < 20000) return; // 既存プロセスへの相乗り起動などでウィンドウが遅れて出る分を待つ
      dead = pidsOf(e).filter((p) => p !== proc.pid).length ? 0 : dead + 1;
      if (dead >= 3) { clearInterval(t); log(`exited ${paneId} hadWindow=${e.hadWindow}`); notify('exited', paneId, { hadWindow: e.hadWindow }); }
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
  saveWids();
  // ウィンドウは閉じる操作で閉じ、PRELUDE が起動したプロセスだけを終了させる(他の窓も持つ既存のプロセスは残す)
  release(e);
  for (const pid of ownPids(e).reverse()) terminate(pid);
  schedule();
}

// 終了時: 取り込んだアプリ全部に SIGTERM を送り、閉じるのを最大 ms ミリ秒待つ。残ったものだけ SIGKILL する。
// (systemd の KillMode=mixed では、PRELUDE が先に終わると残りは即 SIGKILL される。保存中のアプリや
//  キーリングを開いたままのアプリを、書き込みの途中で落とさないための猶予)
async function closeAll(ms) {
  const pids = new Set();
  for (const e of panes.values()) for (const p of ownPids(e)) pids.add(p);
  log(`closeAll panes=${panes.size} pids=${[...pids]}`);
  for (const p of pids) terminate(p);
  // 既存のプロセスのウィンドウ(Zed・システム設定など)は、プロセスを残してウィンドウだけ閉じる
  for (const e of panes.values()) release(e);
  await reload();
  const alive = () => [...pids].filter((p) => { try { process.kill(p, 0); return true; } catch { return false; } });
  const end = Date.now() + ms;
  while (alive().length && Date.now() < end) await new Promise((r) => setTimeout(r, 100));
  for (const p of alive()) { try { process.kill(p, 'SIGKILL'); } catch {} }
}

// ペインのウィンドウを手放す。ペインが開いたウィンドウは閉じ、借りていた既存のウィンドウは普通の窓に戻す
function release(e) {
  for (const w of e.wids) (e.adopted.includes(w) ? releaseWids : closeWids).push(w);
}

// 終了させてよいプロセス: 起動した sh の子孫と、起動後に始まった取り込み済みウィンドウの持ち主
const ownPids = (e) => [...new Set([...(e.proc?.pid ? descendants(e.proc.pid) : []), ...e.owned])];

function shutdown() {
  [...panes.keys()].forEach((id) => close(id));
  if (supported) gdbus('/Scripting', 'org.kde.kwin.Scripting.unloadScript', SCRIPT_NAME);
}

const BROWSERS = ['/snap/bin/chromium', '/usr/bin/chromium', '/usr/bin/chromium-browser', '/usr/bin/google-chrome-stable', '/usr/bin/google-chrome'];
const hasBrowser = () => BROWSERS.some((p) => fs.existsSync(p));

// 実ブラウザ(Chromium系)をアプリモードで開くコマンド。ペインごとにウィンドウクラスを付けて、取り込み時に見分ける
function browserCmd(url, cls) {
  const bin = BROWSERS.find((p) => fs.existsSync(p)) || 'chromium';
  // snap版は ~/snap/chromium/common 以外の隠しディレクトリに書けないので、そこに専用プロファイルを置く
  const profile = bin.startsWith('/snap/') ? path.join(os.homedir(), 'snap/chromium/common/prelude-profile') : path.join(os.homedir(), '.config/PRELUDE-electron/browser-profile');
  const q = (s) => `'${String(s).replace(/'/g, `'\\''`)}'`;
  // 既に同じプロファイルのChromiumが動いていると、新しいプロセスは作られず既存プロセスにウィンドウだけ増える。
  // PIDでは見つからないので、アプリモードのウィンドウクラス(chrome-<host>__<path>-Default)でも照合できるようにしておく
  let winClass = '';
  try { const u = new URL(url); winClass = `chrome-${u.hostname}__${u.pathname.replace(/^\//, '').replace(/\//g, '_')}-Default`.toLowerCase(); } catch {}
  return {
    cmd: `${q(bin)} --app=${q(url)} --user-data-dir=${q(profile)} --class=${q(cls)} --no-first-run --no-default-browser-check`,
    cls: winClass,
  };
}

module.exports = {
  closeAll,
  childEnv,
  browserCmd,
  hasBrowser,
  supported, setShell, launch, setBounds, setSuspended, close, shutdown, listApps: apps.list,
  onEvent: (cb) => { notify = cb; },
};
