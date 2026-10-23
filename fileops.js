'use strict';
// ファイラ(フォルダペイン)のファイル操作。画面側からは fs:* の IPC だけで呼ぶ。
// 一覧は2段階: fs:list で名前と種類だけをすぐ返し、サイズ・日付は fs:stats で後から少しずつ埋める
// (数千件のフォルダや外付け・ネットワークのドライブでも、まず一覧が出る)。
// OS に近い部分はこのファイルに閉じ込めておく(将来 Rust などに置き換えるときはここだけ差し替える)。

const fs = require('fs');
const fsp = fs.promises;
const os = require('os');
const path = require('path');
const { spawn } = require('child_process');
const { pathToFileURL, fileURLToPath } = require('url');
const { shell, clipboard, ClipboardItem, nativeImage } = require('electron');
const { cleanShell } = require('./sys/util');

const isWin = process.platform === 'win32';
const BAD_NAME = isWin ? /[\\/:*?"<>|]/ : /\//;

let config = null;
// 知らせは、頼んできた窓(webContents)へ返す(画面ごとに窓がある)
const sendWc = (wc, channel, payload) => { if (wc && !wc.isDestroyed()) wc.send(channel, payload); };

function validName(name) {
  const n = String(name ?? '').trim();
  if (!n || n === '.' || n === '..' || BAD_NAME.test(n) || n.includes('\0')) return null;
  return n;
}

const exists = (p) => fsp.lstat(p).then(() => true, () => false);

// 同じ名前があれば「名前 (1).拡張子」…にする。フォルダは拡張子を分けない
async function freePath(dir, name, isDir) {
  const ext = isDir ? '' : path.extname(name);
  const base = ext ? name.slice(0, -ext.length) : name;
  let p = path.join(dir, name);
  for (let i = 1; await exists(p); i++) p = path.join(dir, `${base} (${i})${ext}`);
  return p;
}

// a の中(または a 自身)に b があるか
const inside = (a, b) => { const r = path.relative(a, b); return r === '' || (!r.startsWith('..') && !path.isAbsolute(r)); };

// ---------------------------------------------------------------- 一覧
async function list(dir, showHidden) {
  try {
    const abs = path.resolve(dir || os.homedir());
    const dirents = await fsp.readdir(abs, { withFileTypes: true });
    const entries = [];
    const links = [];
    for (const d of dirents) {
      if (!showHidden && d.name.startsWith('.')) continue;
      const en = { name: d.name, isDir: d.isDirectory(), link: d.isSymbolicLink() };
      if (en.link) links.push(en);
      entries.push(en);
    }
    // シンボリックリンクだけは、指す先がフォルダかどうかをここで調べる(並べ順に関わるため)
    await Promise.all(links.map(async (en) => { try { en.isDir = (await fsp.stat(path.join(abs, en.name))).isDirectory(); } catch {} }));
    const parent = path.dirname(abs);
    return { ok: true, path: abs, parent: parent === abs ? null : parent, entries };
  } catch (e) {
    return { ok: false, error: e.message, path: dir };
  }
}

// names のサイズ・更新日時。[size, mtimeMs] の配列(読めないものは null)
async function stats(dir, names) {
  if (!Array.isArray(names)) return [];
  const out = new Array(names.length).fill(null);
  let i = 0;
  const worker = async () => {
    while (i < names.length) {
      const k = i++;
      try { const st = await fsp.stat(path.join(dir, String(names[k]))); out[k] = [st.isDirectory() ? -1 : st.size, st.mtimeMs]; } catch {}
    }
  };
  await Promise.all(Array.from({ length: Math.min(16, names.length) }, worker));
  return out;
}

// ---------------------------------------------------------------- 変更の監視
const watchers = new Map(); // paneId -> { w, dir, timer, wc }
const tracked = new WeakSet(); // 閉じたら監視をやめる窓
function unwatch(id) {
  const x = watchers.get(id);
  if (!x) return;
  clearTimeout(x.timer);
  try { x.w.close(); } catch {}
  watchers.delete(id);
}
function watch(id, dir, wc) {
  if (wc && !tracked.has(wc)) {
    tracked.add(wc);
    wc.once('destroyed', () => { for (const [k, x] of watchers) if (x.wc === wc) unwatch(k); });
  }
  const cur = watchers.get(id);
  if (cur && cur.dir === dir) { cur.wc = wc; return; } // タブが別の画面の窓へ移ったときは、知らせる先だけ変える
  unwatch(id);
  try {
    const x = { dir, timer: 0, w: null, wc };
    x.w = fs.watch(dir, { persistent: false }, () => {
      clearTimeout(x.timer);
      x.timer = setTimeout(() => sendWc(x.wc, 'fs:changed', { id, dir }), 200); // まとめて1回だけ知らせる
    });
    x.w.on('error', () => unwatch(id));
    watchers.set(id, x);
  } catch {} // inotify の上限・権限などで監視できないフォルダは、手動の再読込(F5)だけになる
}

// ---------------------------------------------------------------- 操作
async function mkdir(dir, name) {
  const n = validName(name);
  if (!n) return { ok: false, error: '使えない名前です' };
  try {
    const p = await freePath(dir, n, true);
    await fsp.mkdir(p);
    return { ok: true, name: path.basename(p) };
  } catch (e) { return { ok: false, error: e.message }; }
}

async function rename(full, name) {
  const n = validName(name);
  if (!n) return { ok: false, error: '使えない名前です' };
  const dest = path.join(path.dirname(full), n);
  if (dest === full) return { ok: true, name: n };
  // 大文字小文字だけの変更(Windows)は同じファイルを指すので、存在チェックをしない
  if (dest.toLowerCase() !== full.toLowerCase() && await exists(dest)) return { ok: false, error: `「${n}」はすでにあります` };
  try { await fsp.rename(full, dest); return { ok: true, name: n }; } catch (e) { return { ok: false, error: e.message }; }
}

async function remove(paths, permanent) {
  const errors = [];
  for (const p of paths) {
    try {
      if (permanent) await fsp.rm(p, { recursive: true, force: true });
      else await shell.trashItem(p);
    } catch (e) { errors.push(`${path.basename(p)}: ${e.message}`); }
  }
  return { ok: !errors.length, errors };
}

let jobSeq = 0;
// コピー/移動。同名があれば「(1)」を付けて並べる(上書きはしない)
async function transfer(mode, paths, destDir, wc) {
  const send = (ch, p) => sendWc(wc, ch, p);
  const job = ++jobSeq;
  const errors = [];
  const names = [];
  const total = paths.length;
  for (let k = 0; k < total; k++) {
    const src = paths[k];
    const name = path.basename(src);
    send('fs:progress', { job, mode, done: k, total, name, destDir });
    try {
      const st = await fsp.lstat(src);
      const isDir = st.isDirectory();
      if (isDir && inside(src, destDir)) throw new Error('フォルダをその中へは' + (mode === 'move' ? '移動' : 'コピー') + 'できません');
      if (mode === 'move' && path.dirname(src) === path.resolve(destDir)) { names.push(name); continue; } // 同じ場所への移動は何もしない
      const dest = await freePath(destDir, name, isDir);
      if (mode === 'move') {
        try { await fsp.rename(src, dest); } catch (e) {
          if (e.code !== 'EXDEV') throw e;
          // 別のドライブへの移動は、コピーしてから消す
          await fsp.cp(src, dest, { recursive: true, errorOnExist: true, force: false, preserveTimestamps: true, verbatimSymlinks: true });
          await fsp.rm(src, { recursive: true, force: true });
        }
      } else {
        await fsp.cp(src, dest, { recursive: true, errorOnExist: true, force: false, preserveTimestamps: true, verbatimSymlinks: true });
      }
      names.push(path.basename(dest));
    } catch (e) { errors.push(`${name}: ${e.message}`); }
  }
  send('fs:progress', { job, mode, done: total, total, finished: true, destDir });
  return { ok: !errors.length, errors, names };
}

// ---------------------------------------------------------------- クリップボード
// 1回の書き込みで text/uri-list(ファイルとして)・text/plain(パスの文字列)・切り取りの印(KDE の x-kde-cutselection)を並べる。
// Dolphin などとの間でそのまま貼り付けられる
const KDE_CUT = 'electron application/osclipboard;format="application/x-kde-cutselection"';
let lastClip = null; // { paths, cut } 自分で書いた内容(切り取りかどうかを確実に知るため)
const sameSet = (a, b) => a.length === b.length && a.every((x) => b.includes(x));

async function clipWrite(paths, cut) {
  lastClip = { paths: [...paths], cut: !!cut };
  const data = { 'text/plain': paths.join(os.EOL), 'text/uri-list': paths.map((p) => pathToFileURL(p).href).join('\r\n') };
  if (cut && !isWin) data[KDE_CUT] = '1';
  await clipboard.write([new ClipboardItem(data)]);
}

async function clipRead() {
  const toPath = (s) => { s = s.trim(); if (!s || s.startsWith('#')) return null; try { return s.startsWith('file:') ? fileURLToPath(s) : path.isAbsolute(s) ? s : null; } catch { return null; } };
  const get = async (it, t) => { if (!it.types.includes(t)) return ''; try { const b = await it.getType(t); return typeof b === 'string' ? b : await b.text(); } catch { return ''; } };
  let paths = [];
  let kdeCut = false;
  for (const it of await clipboard.read()) {
    let got = (await get(it, 'text/uri-list')).split(/\r?\n/).map(toPath).filter(Boolean);
    if (!got.length) got = (await get(it, 'text/plain')).split(/\r?\n/).map(toPath).filter(Boolean);
    paths.push(...got);
    if ((await get(it, KDE_CUT)).trim() === '1') kdeCut = true;
  }
  paths = paths.filter((p) => fs.existsSync(p));
  const cut = lastClip && sameSet(lastClip.paths, paths) ? lastClip.cut : kdeCut;
  return { paths, cut };
}

// 切り取り→貼り付けが終わったら、同じものをもう一度貼れないようにクリップボードを空にする
function clipConsumed(paths) {
  if (lastClip && lastClip.cut && sameSet(lastClip.paths, paths)) { lastClip = null; clipboard.clear(); }
}

// ---------------------------------------------------------------- 端末・プロパティ
const quote = (s) => (isWin ? `"${s}"` : `'${String(s).replace(/'/g, `'\\''`)}'`);
const terminalCmd = (dir) => String(config.get('behavior.terminal') || (isWin ? 'wt.exe -d %d' : 'konsole --workdir %d')).replace(/%d/g, quote(dir));

function openTerminal(dir) {
  try {
    const cmd = terminalCmd(dir);
    const p = isWin ? spawn(cmd, { shell: true, detached: true, stdio: 'ignore', cwd: dir }) : spawn(...cleanShell(cmd), { detached: true, stdio: 'ignore', cwd: dir });
    p.on('error', () => {});
    p.unref();
    return true;
  } catch { return false; }
}

const modeStr = (m) => [0o400, 0o200, 0o100, 0o40, 0o20, 0o10, 0o4, 0o2, 0o1].map((b, i) => (m & b ? 'rwx'[i % 3] : '-')).join('');

async function props(paths) {
  const items = [];
  for (const p of paths) {
    try {
      const l = await fsp.lstat(p);
      const st = l.isSymbolicLink() ? await fsp.stat(p).catch(() => l) : l;
      const it = { full: p, name: path.basename(p) || p, isDir: st.isDirectory(), size: st.size, mtime: st.mtimeMs, btime: st.birthtimeMs, mode: modeStr(st.mode), uid: st.uid };
      if (l.isSymbolicLink()) it.link = await fsp.readlink(p).catch(() => '');
      if (it.isDir) it.count = (await fsp.readdir(p).catch(() => [])).length;
      items.push(it);
    } catch (e) { items.push({ full: p, name: path.basename(p), error: e.message }); }
  }
  return items;
}

// フォルダの中身の合計サイズ。時間がかかりすぎるときは途中までの値を返す
async function dirSize(paths, limitMs = 4000) {
  const until = Date.now() + limitMs;
  let size = 0, files = 0, dirs = 0, partial = false;
  const walk = async (p) => {
    if (Date.now() > until) { partial = true; return; }
    let st;
    try { st = await fsp.lstat(p); } catch { return; }
    if (!st.isDirectory()) { size += st.size; files++; return; }
    dirs++;
    let names = [];
    try { names = await fsp.readdir(p); } catch { return; }
    for (const n of names) await walk(path.join(p, n));
  };
  for (const p of paths) await walk(p);
  return { size, files, dirs, partial };
}

// ---------------------------------------------------------------- ブックマークのファイル(.prelude)
// 中身の形は画面側(src/bmfile.js)が決める。ここでは .prelude のファイルだけを読み書きする
const BM_EXT = '.prelude';
const BM_MAX = 256 * 1024;
const isBm = (p) => path.isAbsolute(p) && p.toLowerCase().endsWith(BM_EXT);

// dir の中に新しく作る(同じ名前があれば「名前 (1).prelude」)。dir が無ければ作る
async function bmSave(dir, name, text) {
  const n = validName(name);
  if (!path.isAbsolute(dir) || !n || !n.toLowerCase().endsWith(BM_EXT)) return { error: '保存先が正しくありません' };
  try {
    await fsp.mkdir(dir, { recursive: true });
    const p = await freePath(dir, n, false);
    await fsp.writeFile(p, String(text), { flag: 'wx' });
    return { path: p };
  } catch (e) { return { error: e.message }; }
}
async function bmMkdir(dir) {
  if (!path.isAbsolute(dir)) return { error: '保存先が正しくありません' };
  try { await fsp.mkdir(dir, { recursive: true }); return { path: dir }; } catch (e) { return { error: e.message }; }
}
async function bmWrite(full, text) {
  if (!isBm(full)) return { error: 'ブックマークのファイルではありません' };
  try { await fsp.writeFile(full, String(text)); return { path: full }; } catch (e) { return { error: e.message }; }
}
async function bmRead(full) {
  if (!isBm(full)) return { error: 'ブックマークのファイルではありません' };
  try {
    if ((await fsp.stat(full)).size > BM_MAX) return { error: 'ファイルが大きすぎます' };
    return { text: await fsp.readFile(full, 'utf8') };
  } catch (e) { return { error: e.message }; }
}

// ---------------------------------------------------------------- IPC
const strs = (a) => (Array.isArray(a) ? a.filter((x) => typeof x === 'string' && x) : []);

function init(o) {
  ({ config } = o);
  const { ipcMain } = o;
  ipcMain.handle('fs:list', (_e, dir, showHidden) => list(dir, showHidden));
  ipcMain.handle('fs:stats', (_e, dir, names) => stats(String(dir), names));
  ipcMain.on('fs:watch', (e, id, dir) => watch(String(id), String(dir), e.sender));
  ipcMain.on('fs:unwatch', (_e, id) => unwatch(String(id)));
  ipcMain.handle('fs:open', (_e, p) => shell.openPath(String(p)));
  ipcMain.handle('fs:open-with', (_e, p) => require('./sys/apps').forFile(String(p))); // 既定のアプリ(ペインに取り込んで開く用)
  ipcMain.handle('fs:home', () => os.homedir());
  ipcMain.handle('fs:bm-save', (_e, dir, name, text) => bmSave(String(dir), name, text));
  ipcMain.handle('fs:bm-mkdir', (_e, dir) => bmMkdir(String(dir)));
  ipcMain.handle('fs:bm-write', (_e, full, text) => bmWrite(String(full), text));
  ipcMain.handle('fs:bm-read', (_e, full) => bmRead(String(full)));
  ipcMain.handle('fs:mkdir', (_e, dir, name) => mkdir(String(dir), name));
  ipcMain.handle('fs:rename', (_e, full, name) => rename(String(full), name));
  ipcMain.handle('fs:remove', (_e, paths, permanent) => remove(strs(paths), !!permanent));
  ipcMain.handle('fs:transfer', async (e, mode, paths, destDir) => {
    const r = await transfer(mode === 'move' ? 'move' : 'copy', strs(paths), String(destDir), e.sender);
    if (mode === 'move') clipConsumed(strs(paths));
    return r;
  });
  ipcMain.handle('fs:clip-write', (_e, paths, cut) => clipWrite(strs(paths), cut));
  ipcMain.handle('fs:clip-read', () => clipRead());
  ipcMain.handle('fs:copy-text', (_e, s) => clipboard.writeText(String(s)));
  ipcMain.handle('fs:terminal', (_e, dir) => openTerminal(String(dir)));
  ipcMain.handle('fs:terminal-cmd', (_e, dir) => terminalCmd(String(dir)));
  ipcMain.handle('fs:props', (_e, paths) => props(strs(paths)));
  ipcMain.handle('fs:dirsize', (_e, paths) => dirSize(strs(paths)));
  // ファイルを OS のドラッグとして渡す(Claude・エディタ・ブラウザなど、他のアプリに落とせる)。icon は画面側で描いた札(data URL)
  ipcMain.on('fs:drag', (e, paths, icon) => {
    const files = strs(paths).filter((p) => fs.existsSync(p));
    if (!files.length) return;
    let img = typeof icon === 'string' ? nativeImage.createFromDataURL(icon) : null;
    if (!img || img.isEmpty()) img = nativeImage.createFromBuffer(Buffer.from('iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg==', 'base64'));
    e.sender.startDrag({ file: files[0], files, icon: img });
  });
  ipcMain.handle('fs:drives', async () => {
    const out = [{ label: 'ホーム', path: os.homedir() }];
    if (isWin) {
      for (let c = 65; c <= 90; c++) {
        const p = String.fromCharCode(c) + ':\\';
        try { await fsp.access(p); out.push({ label: p, path: p }); } catch {}
      }
    } else {
      out.push({ label: '/', path: '/' });
      for (const base of ['/mnt', '/media', path.join('/run/media', os.userInfo().username), path.join('/media', os.userInfo().username)]) {
        try {
          for (const d of await fsp.readdir(base, { withFileTypes: true })) {
            if (d.isDirectory()) out.push({ label: path.join(base, d.name), path: path.join(base, d.name) });
          }
        } catch {}
      }
    }
    return out;
  });
}

const shutdown = () => { for (const id of [...watchers.keys()]) unwatch(id); };

module.exports = { init, shutdown };
