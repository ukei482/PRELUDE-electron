#!/usr/bin/env node
// 橋: 本物のコアのソケットと、ブラウザで開いた試作をつなぐ(本番では Rust の外側がこの役をする)。依存なし。
//   node proto/web/bridge.js [--port 8370]
//   → http://127.0.0.1:8370/sidebar.html(サイドバーだけ)・/stage.html(見た目を試す台)
// ソケットは PRELUDE_CORE_SOCK → $XDG_RUNTIME_DIR/prelude/core.sock → /tmp/prelude-$USER/core.sock(prelude-proto の default_socket_path と同じ)。
// - GET /events: コアの様子(状態・ブックマーク・フォルダ・スペース・レシピ)を Server-Sent Events で。状態が変わるたび
// - POST /cmd: {cmd, args} をコアへ送り、返事({result} か {error})を返す
// - POST /launch: {split?, pane?, url?} で prelude-launcher を起動する(分割のボタン・新しい Web のタブ)
//   POST はどれも x-prelude の頭が要る。ほかの Web ページから命令を送らせないため(頭を付けると、ブラウザは先に確かめの頼みを
//   送るが、橋はそれに許しを返さないので、ほかのページからは届かない)
// - GET /icon?path=<絶対パス>: アイコンの画像(コアの状態の icon。画像の拡張子だけ)
'use strict';
const net = require('node:net');
const { spawn } = require('node:child_process');
const http = require('node:http');
const fs = require('node:fs');
const path = require('node:path');

const PROTOCOL = 1;
const DIR = __dirname;
const port = Number((process.argv.indexOf('--port') > 0 && process.argv[process.argv.indexOf('--port') + 1]) || 8370);

function socketPath() {
  if (process.env.PRELUDE_CORE_SOCK) return process.env.PRELUDE_CORE_SOCK;
  if (process.env.XDG_RUNTIME_DIR) return path.join(process.env.XDG_RUNTIME_DIR, 'prelude', 'core.sock');
  return `/tmp/prelude-${process.env.USER || 'user'}/core.sock`;
}

let snap = { online: false, status: 'コアを探しています…', state: null, spaces: [], bookmarks: [], folders: [], recipes: [] };
const clients = new Set();
const broadcast = () => {
  const line = `data: ${JSON.stringify(snap)}\n\n`;
  for (const res of clients) res.write(line);
};

let conn = null;
let nextId = 1;
const pending = new Map();
function request(cmd, args = {}) {
  return new Promise((resolve) => {
    if (!conn) return resolve({ error: { code: 'unavailable', message: 'コアにつながっていません' } });
    const id = nextId++;
    pending.set(id, resolve);
    conn.write(`${JSON.stringify({ id, cmd, args })}\n`);
  });
}

let lastRev = null;
async function lists(force) {
  const st = snap.state;
  const rev = st ? `${st.space}:${st.bookmarks_rev}` : '';
  if (!force && rev === lastRev) return;
  lastRev = rev;
  const [spaces, bookmarks, folders, recipes] = await Promise.all(['space.list', 'bookmark.list', 'folder.list', 'recipe.list'].map((c) => request(c)));
  snap = { ...snap, spaces: spaces.result || [], bookmarks: bookmarks.result || [], folders: folders.result || [], recipes: recipes.result || [] };
}

function connect() {
  const s = net.createConnection(socketPath());
  let buf = '';
  s.on('connect', async () => {
    conn = s;
    const hello = await request('hello', { protocol: PROTOCOL, name: 'proto-web', subscribe: true });
    if (hello.error) {
      snap = { ...snap, online: false, status: `コアと約束の版が合いません: ${hello.error.message}` };
      return broadcast();
    }
    const st = await request('state.get');
    snap = { ...snap, online: true, status: '', state: st.result };
    await lists(true);
    broadcast();
  });
  s.on('data', async (d) => {
    buf += d;
    let i;
    while ((i = buf.indexOf('\n')) >= 0) {
      const line = buf.slice(0, i);
      buf = buf.slice(i + 1);
      if (!line.trim()) continue;
      let m;
      try {
        m = JSON.parse(line);
      } catch {
        continue;
      }
      if (m.event === 'state') {
        snap = { ...snap, online: true, status: '', state: m.data };
        await lists(false);
        broadcast();
      } else if (m.id != null && pending.has(m.id)) {
        pending.get(m.id)(m.error ? { error: m.error } : { result: m.result });
        pending.delete(m.id);
      }
    }
  });
  const down = (why) => {
    if (conn === s) conn = null;
    for (const r of pending.values()) r({ error: { code: 'unavailable', message: why } });
    pending.clear();
    snap = { ...snap, online: false, status: `コアにつながっていません(${socketPath()})` };
    broadcast();
    setTimeout(connect, 1000);
  };
  s.on('error', (e) => s.destroy(e));
  s.on('close', () => down('コアとの接続が切れました'));
}

// prelude-launcher の引数(分割は今のペインを分ける。左・上(before)はランチャーがまだ持たないので、右・下になる)
function launcherArgs(a) {
  const out = [];
  if (a.split === 'row' || a.split === 'col') out.push('--split', a.split);
  if (Number.isInteger(a.pane) && typeof a.url === 'string' && a.url) out.push('--pane', String(a.pane), '--url', a.url);
  return out;
}
function launch(a) {
  const prog = process.env.PRELUDE_LAUNCHER || 'prelude-launcher';
  try {
    const p = spawn(prog, launcherArgs(a), { detached: true, stdio: 'ignore' });
    p.on('error', (e) => console.error(`prelude proto/web: ${prog}: ${e.message}`));
    p.unref();
    return { result: null };
  } catch (e) {
    return { error: { code: 'spawn', message: `${prog}: ${e.message}` } };
  }
}

const TYPES = { '.html': 'text/html; charset=utf-8', '.js': 'text/javascript; charset=utf-8', '.css': 'text/css; charset=utf-8', '.png': 'image/png', '.svg': 'image/svg+xml', '.jpg': 'image/jpeg', '.jpeg': 'image/jpeg', '.ico': 'image/x-icon', '.webp': 'image/webp' };
const IMAGES = new Set(['.png', '.svg', '.jpg', '.jpeg', '.ico', '.webp']);

const server = http.createServer((req, res) => {
  const u = new URL(req.url, 'http://x');
  if (u.pathname === '/events') {
    res.writeHead(200, { 'content-type': 'text/event-stream', 'cache-control': 'no-cache', connection: 'keep-alive' });
    clients.add(res);
    res.write(`data: ${JSON.stringify(snap)}\n\n`);
    req.on('close', () => clients.delete(res));
    return;
  }
  if (req.method === 'POST' && req.headers['x-prelude'] !== '1') {
    res.writeHead(403, { 'content-type': 'application/json' });
    return res.end(JSON.stringify({ error: { code: 'forbidden', message: 'x-prelude の頭がありません' } }));
  }
  if (u.pathname === '/launch' && req.method === 'POST') {
    let body = '';
    req.on('data', (d) => (body += d));
    req.on('end', () => {
      let r = { result: null };
      try {
        const a = JSON.parse(body || '{}');
        r = launch(a);
      } catch (e) {
        r = { error: { code: 'bad_request', message: String(e.message || e) } };
      }
      res.writeHead(200, { 'content-type': 'application/json' });
      res.end(JSON.stringify(r));
    });
    return;
  }
  if (u.pathname === '/cmd' && req.method === 'POST') {
    let body = '';
    req.on('data', (d) => (body += d));
    req.on('end', async () => {
      let r;
      try {
        const { cmd, args } = JSON.parse(body);
        r = await request(cmd, args || {});
      } catch (e) {
        r = { error: { code: 'bad_request', message: String(e.message || e) } };
      }
      res.writeHead(200, { 'content-type': 'application/json' });
      res.end(JSON.stringify(r));
    });
    return;
  }
  if (u.pathname === '/icon') {
    const p = u.searchParams.get('path') || '';
    if (!path.isAbsolute(p) || !IMAGES.has(path.extname(p).toLowerCase())) return res.writeHead(404).end();
    return fs.readFile(p, (err, data) => (err ? res.writeHead(404).end() : res.writeHead(200, { 'content-type': TYPES[path.extname(p).toLowerCase()] }).end(data)));
  }
  // 試作のファイル(このフォルダの中だけ)
  const rel = u.pathname === '/' ? 'stage.html' : decodeURIComponent(u.pathname).replace(/^\/+/, '');
  const file = path.resolve(DIR, rel);
  if (!file.startsWith(DIR + path.sep) || !TYPES[path.extname(file)]) return res.writeHead(404).end();
  fs.readFile(file, (err, data) => (err ? res.writeHead(404).end() : res.writeHead(200, { 'content-type': TYPES[path.extname(file)] }).end(data)));
});

if (require.main !== module) {
  module.exports = { launcherArgs };
  return;
}
server.listen(port, '127.0.0.1', () => {
  console.log(`prelude proto/web: http://127.0.0.1:${port}/  (core: ${socketPath()})`);
  connect();
});
