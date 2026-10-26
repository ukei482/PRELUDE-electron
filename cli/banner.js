'use strict';
// PRELUDE コマンドを起動したときの画面(neofetch 風)。左にハ音記号の AA(暫定のロゴ)、右に反転した PRELUDE・この PC の情報・
// 色の表示のテスト(16 色・トゥルーカラーのグラデーション)・命令の例を出す。
// bannerText は渡された情報から文字列を作るだけ(test/ から読み込める)。collectInfo が実際の情報を集める。
const os = require('os');
const fs = require('fs');
const path = require('path');
const { spawnSync } = require('child_process');

// ハ音記号(暫定のロゴ)。左に太い縦線と細い縦線、右に上下2つの弧があり、弧は中央で細い縦線へ向かって尖る。
// 点の絵を半ブロックの文字(▀▄█)で1文字に2段ずつ描いている
const LOGO = [
  '█████  ██       ▄▄███████▄▄',
  '█████  ██   ▄████▀▀    ▀▀████▄',
  '█████  ██  ██████▄        ▀███▄',
  '█████  ██   ▀███▀          ▀███▄',
  '█████  ██                   ████',
  '█████  ██                   ████',
  '█████  ██                  ████',
  '█████  ██     ▄███       ▄████',
  '█████  ██    ██████▄▄▄▄████▀▀',
  '█████  ██  ▄███▀▀ ▀▀▀▀▀▀▀',
  '█████  ██ ██▀▀',
  '█████  ██ ██▄▄',
  '█████  ██  ▀███▄▄ ▄▄▄▄▄▄▄',
  '█████  ██    ██████▀▀▀▀████▄▄',
  '█████  ██     ▀███       ▀████',
  '█████  ██                  ████',
  '█████  ██                   ████',
  '█████  ██                   ████',
  '█████  ██   ▄███▄          ▄███▀',
  '█████  ██  ██████▀        ▄███▀',
  '█████  ██   ▀████▄▄    ▄▄████▀',
  '█████  ██       ▀▀███████▀▀',
];

const ESC = '\x1b[';
const inv = (s) => `${ESC}7m${s}${ESC}27m`;
const dim = (s) => `${ESC}2m${s}${ESC}22m`;
const bold = (s) => `${ESC}1m${s}${ESC}22m`;
// 画面の幅(色の指定を除いた文字数。全角は2)
const width = (s) => [...s.replace(/\x1b\[[0-9;]*m/g, '')].reduce((n, c) => n + (/[ᄀ-ᅟ⺀-꓏가-힣豈-﫿︰-﹏＀-｠￠-￦]/.test(c) ? 2 : 1), 0);

// 16 色の見本(普通の 8 色と明るい 8 色)
const palette = (bright) => Array.from({ length: 8 }, (_, i) => `${ESC}${bright ? 100 + i : 40 + i}m   `).join('') + `${ESC}0m`;
// トゥルーカラーのグラデーション(青 → 水色 → 白 → 桃 → 赤)
function gradient(n) {
  const stops = [[25, 38, 163], [40, 170, 230], [235, 235, 245], [240, 120, 190], [220, 40, 60]];
  let s = '';
  for (let i = 0; i < n; i++) {
    const t = (i / Math.max(1, n - 1)) * (stops.length - 1);
    const k = Math.min(stops.length - 2, Math.floor(t)), f = t - k;
    const [r, g, b] = stops[k].map((v, j) => Math.round(v + (stops[k + 1][j] - v) * f));
    s += `${ESC}38;2;${r};${g};${b}m█`;
  }
  return s + `${ESC}0m`;
}

// 最初に試す命令(説明は help で)
const HINTS = ['help', 'tabs', 'open <app|url|path>', 'folder ~', 'web <url>'];

function bannerText(info, cols = 80) {
  const keyW = 8;
  const row = (k, v) => `${bold(k.padEnd(keyW))}${v}`;
  const right = [
    `${inv(' PRELUDE ')} ${dim('v' + info.version)}`,
    dim('─'.repeat(28)),
    row('user', `${info.user}@${info.host}`),
    row('os', info.os),
    row('kernel', info.kernel),
    row('uptime', info.uptime),
    row('shell', info.shell),
    row('term', info.term),
    row('cpu', info.cpu),
    row('memory', info.memory),
    '',
    palette(false),
    palette(true),
    gradient(24),
  ];
  const hints = `${dim('try')}  ${HINTS.map(bold).join(dim('  ·  '))}  ${dim('|')}  ${bold('exit')} ${dim('→ shell,')} ${bold('PRELUDE')} ${dim('→ back here')}`;
  const logoW = Math.max(...LOGO.map(width)) + 3;
  const rightW = Math.max(...right.map(width));
  const out = [''];
  if (cols >= logoW + rightW) { // 横に並べる。右が長ければ切る
    const top = Math.max(0, Math.floor((LOGO.length - right.length) / 2)); // 右はロゴの高さの真ん中に寄せる
    const n = Math.max(LOGO.length, top + right.length);
    for (let i = 0; i < n; i++) {
      const l = LOGO[i] || '';
      out.push(`${l}${' '.repeat(logoW - width(l))}${right[i - top] || ''}`.replace(/\s+$/, ''));
    }
  } else { // 狭いときは縦に並べる
    out.push(...LOGO, '', ...right);
  }
  if (width(hints) <= cols) out.push('', hints, '');
  else { // 狭いときは、収まるところで折り返す
    const lines = [dim('try')];
    for (const w of [...HINTS.map(bold), `${bold('exit')} ${dim('→ shell')}`]) {
      if (width(lines[lines.length - 1] + '  ' + w) <= cols) lines[lines.length - 1] += '  ' + w;
      else lines.push('     ' + w);
    }
    out.push('', ...lines, '');
  }
  return out.join('\n') + '\n';
}

// ---------------------------------------------------------------- 実際の情報
const read = (f) => { try { return fs.readFileSync(f, 'utf8'); } catch { return ''; } };
function fmtUptime(sec) {
  const d = Math.floor(sec / 86400), h = Math.floor((sec % 86400) / 3600), m = Math.floor((sec % 3600) / 60);
  return [d && `${d}d`, (d || h) && `${h}h`, `${m}m`].filter(Boolean).join(' ');
}

function collectInfo(cols, rows) {
  const osName = /^PRETTY_NAME="?([^"\n]*)/m.exec(read('/etc/os-release'))?.[1] || os.type();
  const mem = Object.fromEntries(read('/proc/meminfo').split('\n').map((l) => /^(\w+):\s+(\d+)/.exec(l)).filter(Boolean).map((m) => [m[1], Number(m[2]) * 1024]));
  const total = mem.MemTotal || os.totalmem(), used = total - (mem.MemAvailable ?? os.freemem());
  const gib = (n) => (n / 1073741824).toFixed(1);
  const bash = spawnSync('bash', ['-c', 'echo ${BASH_VERSION%%(*}'], { encoding: 'utf8', timeout: 1000 }).stdout?.trim();
  let version = '?';
  try { version = JSON.parse(read(path.join(__dirname, '..', 'package.json'))).version; } catch {}
  return {
    version, user: os.userInfo().username, host: os.hostname(), os: osName, kernel: os.release(),
    uptime: fmtUptime(os.uptime()), shell: bash ? `bash ${bash}` : 'bash',
    term: `${process.env.TERM || '?'} ${cols}x${rows}`,
    cpu: `${(os.cpus()[0]?.model || '?').replace(/\s+/g, ' ').replace(/\((R|TM)\)/g, '').trim()} (${os.cpus().length})`,
    memory: `${gib(used)} / ${gib(total)} GiB`,
  };
}

module.exports = { LOGO, bannerText, collectInfo, width };
