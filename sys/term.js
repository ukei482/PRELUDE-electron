'use strict';
// ホームの端末。窓(key)ごとに1つ、bash を sys/ptyhost.py の疑似端末で動かす。画面に出すのはレンダラの src/term.js(xterm.js)。
// 端末の中では PRELUDE コマンド(cli/PRELUDE → cli/prelude.js)が使え、ここのソケットを通して、その窓のレンダラ(src/cli.js)に命令を渡す。
const net = require('net');
const fs = require('fs');
const os = require('os');
const path = require('path');
const { spawn } = require('child_process');
const { StringDecoder } = require('string_decoder');
const { instanceTag, CLOSE_FDS } = require('./util');

const ROOT = path.join(__dirname, '..');
const CLI_DIR = path.join(ROOT, 'cli');
// 別プロファイルの PRELUDE(デバッグ用)は別のソケットにする
const SOCK = path.join(process.env.XDG_RUNTIME_DIR || os.tmpdir(), `prelude${instanceTag}-cli.sock`);

const terms = new Map(); // key -> { proc }
const waiting = new Map(); // 命令の id -> 返事を受けたら呼ぶ関数
let seq = 0;
let ctx = null; // { sendTo, childEnv }
let server = null;

// 標準入力の枠: 種類 1 バイト + 長さ 4 バイト + 中身(sys/ptyhost.py)
function frame(kind, body) {
  const b = Buffer.from(body, 'utf8');
  const head = Buffer.alloc(5);
  head.write(kind, 0, 'latin1');
  head.writeUInt32BE(b.length, 1);
  return Buffer.concat([head, b]);
}

function stop(key) {
  const t = terms.get(key);
  if (!t) return;
  terms.delete(key);
  try { t.proc.kill('SIGTERM'); } catch {}
}

function start(key, cols, rows) {
  stop(key); // レンダラを読み込み直したときは、前の端末を終わらせて作り直す
  const env = {
    ...ctx.childEnv(), TERM: 'xterm-256color', COLORTERM: 'truecolor',
    PRELUDE_SOCK: SOCK, PRELUDE_WIN: key, PRELUDE_NODE: process.execPath, PRELUDE_CLI_DIR: CLI_DIR,
  };
  for (const k of ['PRELUDE_DEBUG', 'PRELUDE_EMBEDDED', 'ELECTRON_RUN_AS_NODE']) delete env[k];
  // Electron が開いたままのファイルを渡さないよう、bash で閉じてから動かす(sys/util.js の CLOSE_FDS)
  const proc = spawn('/bin/bash', ['-c', `${CLOSE_FDS}; exec python3 -I "$0" "$@"`, path.join(__dirname, 'ptyhost.py'),
    String(cols | 0 || 80), String(rows | 0 || 24), '/bin/bash', '--rcfile', path.join(CLI_DIR, 'bashrc'), '-i'],
  { env, cwd: os.homedir(), stdio: ['pipe', 'pipe', 'ignore'] });
  const t = { proc };
  terms.set(key, t);
  const dec = new StringDecoder('utf8'); // 文字の途中で切れた出力を次の分とつなぐ
  proc.stdout.on('data', (d) => { if (terms.get(key) === t) ctx.sendTo(key, 'term:data', dec.write(d)); });
  proc.stdin.on('error', () => {});
  proc.on('error', () => {});
  proc.on('exit', (code) => {
    if (terms.get(key) !== t) return;
    terms.delete(key);
    ctx.sendTo(key, 'term:exit', code);
  });
}

const write = (key, data) => { const t = terms.get(key); if (t) t.proc.stdin.write(frame('d', String(data))); };
const resize = (key, cols, rows) => { const t = terms.get(key); if (t) t.proc.stdin.write(frame('r', `${cols | 0} ${rows | 0}`)); };

// PRELUDE コマンドからの命令(1行の JSON: { win, line, cwd })を、その窓のレンダラへ渡して、返事(1行の JSON)を返す
function serve() {
  try { fs.unlinkSync(SOCK); } catch {}
  server = net.createServer((sock) => {
    let buf = '';
    sock.setEncoding('utf8');
    sock.on('error', () => {});
    sock.on('data', (s) => {
      buf += s;
      let i;
      while ((i = buf.indexOf('\n')) >= 0) {
        const line = buf.slice(0, i);
        buf = buf.slice(i + 1);
        let req;
        try { req = JSON.parse(line); } catch { continue; }
        const id = ++seq;
        const reply = (r) => { waiting.delete(id); clearTimeout(timer); if (!sock.destroyed) sock.write(JSON.stringify(r) + '\n'); };
        const timer = setTimeout(() => reply({ ok: false, out: 'PRELUDE did not answer' }), 15000);
        waiting.set(id, reply);
        ctx.sendTo(terms.has(req.win) ? req.win : 'main', 'cli:run', { id, line: String(req.line || ''), cwd: String(req.cwd || '') });
      }
    });
  });
  server.on('error', (e) => console.error('cli socket failed', e.message));
  server.listen(SOCK, () => { try { fs.chmodSync(SOCK, 0o600); } catch {} });
}

function init({ ipcMain, keyOf, sendTo, childEnv }) {
  ctx = { sendTo, childEnv };
  ipcMain.on('term:start', (e, cols, rows) => { const k = keyOf(e.sender); if (k) start(k, cols, rows); });
  ipcMain.on('term:write', (e, data) => write(keyOf(e.sender), data));
  ipcMain.on('term:resize', (e, cols, rows) => resize(keyOf(e.sender), cols, rows));
  ipcMain.on('cli:result', (_e, r) => waiting.get(r?.id)?.({ ok: !!r.ok, out: String(r.out ?? '') }));
  serve();
}

function shutdown() {
  for (const k of [...terms.keys()]) stop(k);
  try { server?.close(); } catch {}
  try { fs.unlinkSync(SOCK); } catch {}
}

module.exports = { init, stop, shutdown };
