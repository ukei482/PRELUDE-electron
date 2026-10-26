'use strict';
// PRELUDE コマンドの本体(cli/PRELUDE から Electron 同梱の Node で動く)。
// 打った行はそのまま PRELUDE(sys/term.js のソケット → その窓のレンダラの src/cli.js)へ送り、返ってきた文を出す。
// exit / quit / Ctrl+D で抜けて普通のシェルに戻る。clear は画面を消し、banner は起動したときの画面(cli/banner.js)をもう一度出す。ほかの命令は全部 PRELUDE 側で解釈する。
const net = require('net');
const fs = require('fs');
const os = require('os');
const path = require('path');
const readline = require('readline');
const { bannerText, collectInfo } = require('./banner');

const SOCK = process.env.PRELUDE_SOCK;
const WIN = process.env.PRELUDE_WIN || 'main';
const HIST = path.join(process.env.XDG_STATE_HOME || path.join(os.homedir(), '.local', 'state'), 'prelude', 'cli_history');

function ask(line) {
  return new Promise((resolve) => {
    const sock = net.connect(SOCK);
    let buf = '';
    sock.setEncoding('utf8');
    sock.on('connect', () => sock.write(JSON.stringify({ win: WIN, line, cwd: process.cwd() }) + '\n'));
    sock.on('data', (s) => {
      buf += s;
      const i = buf.indexOf('\n');
      if (i < 0) return;
      sock.end();
      try { resolve(JSON.parse(buf.slice(0, i))); } catch { resolve({ ok: false, out: 'Bad answer from PRELUDE' }); }
    });
    sock.on('error', () => resolve({ ok: false, out: 'PRELUDE is not running' }));
  });
}

const show = (r) => { if (r.out) (r.ok ? process.stdout : process.stderr).write(r.out.replace(/\n?$/, '\n')); };

// 補完に使う命令の名前(help --names で PRELUDE から受け取る)
let names = ['exit', 'clear', 'banner'];
const completer = (line) => {
  if (/\s/.test(line)) return [[], line];
  const hits = names.filter((n) => n.startsWith(line));
  return [hits.length ? hits : names, line];
};

async function repl() {
  let history = [];
  try { history = fs.readFileSync(HIST, 'utf8').split('\n').filter(Boolean).reverse().slice(0, 500); } catch {}
  ask('help --names').then((r) => { if (r.ok) names = [...new Set([...r.out.split(/\s+/).filter(Boolean), 'exit', 'clear', 'banner'])].sort(); });
  const rl = readline.createInterface({ input: process.stdin, output: process.stdout, history, historySize: 500, completer, terminal: true });
  rl.on('history', (h) => {
    try { fs.mkdirSync(path.dirname(HIST), { recursive: true }); fs.writeFileSync(HIST, [...h].reverse().join('\n') + '\n'); } catch {}
  });
  rl.setPrompt('\x1b[7mPRELUDE\x1b[0m> '); // 上のバーの PRELUDE と同じく反転して出す
  const banner = () => process.stdout.write(bannerText(collectInfo(process.stdout.columns || 80, process.stdout.rows || 24), process.stdout.columns || 80));
  banner();
  rl.prompt();
  rl.on('SIGINT', () => { rl.write(null, { ctrl: true, name: 'u' }); process.stdout.write('^C\n'); rl.prompt(); });
  rl.on('close', () => { process.stdout.write('\n'); process.exit(0); });
  for await (const raw of rl) {
    const line = raw.trim();
    if (line === 'exit' || line === 'quit') rl.close();
    else if (line === 'clear') process.stdout.write('\x1b[2J\x1b[3J\x1b[H');
    else if (line === 'banner') banner();
    else if (line) { rl.pause(); show(await ask(line)); rl.resume(); }
    rl.prompt();
  }
}

if (!SOCK) { console.error('PRELUDE: only works in the PRELUDE home terminal'); process.exit(1); }
const args = process.argv.slice(2);
// 1つだけ実行するときは、空白を含む引数を "" で囲んで1行にする(PRELUDE 側で同じように区切り直す)
if (args.length) ask(args.map((a) => (/[\s"'\\]/.test(a) || !a ? JSON.stringify(a) : a)).join(' ')).then((r) => { show(r); process.exit(r.ok ? 0 : 1); });
else repl();
