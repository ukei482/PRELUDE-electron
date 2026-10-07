'use strict';
// ステータスバー用のシステム状態(電池・音量・ネットワーク)。変化したときだけ画面側へ送る。
// 外部コマンド(wpctl / nmcli / pactl)と /sys を読むだけで、状態の変更は音量操作のみ。
const { execFile, spawn } = require('child_process');
const fs = require('fs');

let state = null; // 直近に送った状態(JSON文字列)
let last = { battery: null, volume: null, net: null };
let push = () => {};

const run = (cmd, args) => new Promise((resolve) => {
  execFile(cmd, args, { timeout: 3000 }, (err, out) => resolve(err ? null : out));
});

function readBattery() {
  const base = '/sys/class/power_supply';
  try {
    const bat = fs.readdirSync(base).find((n) => /^BAT/i.test(n));
    if (!bat) return null; // デスクトップPCなど
    const rd = (f) => fs.readFileSync(`${base}/${bat}/${f}`, 'utf8').trim();
    return { percent: Number(rd('capacity')), status: rd('status') }; // Charging / Discharging / Full ...
  } catch { return null; }
}

async function readVolume() {
  const out = await run('wpctl', ['get-volume', '@DEFAULT_AUDIO_SINK@']);
  const m = out && /Volume:\s*([\d.]+)/.exec(out);
  return m ? { percent: Math.round(Number(m[1]) * 100), muted: /\[MUTED\]/.test(out) } : null;
}

async function readNet() {
  const out = await run('nmcli', ['-t', '-f', 'TYPE,STATE,CONNECTION', 'device']);
  if (out == null) return null;
  let wifi = null, eth = null;
  for (const line of out.split('\n')) {
    const [type, st, ...conn] = line.split(':');
    if (st !== 'connected') continue; // "connected (externally)" などは対象外
    if (type === 'wifi') wifi = conn.join(':');
    else if (type === 'ethernet') eth = conn.join(':');
  }
  if (wifi != null) return { kind: 'wifi', name: wifi };
  if (eth != null) return { kind: 'ethernet', name: eth };
  return { kind: 'none', name: '' };
}

async function refresh(parts = ['battery', 'volume', 'net']) {
  if (parts.includes('battery')) last.battery = readBattery();
  const jobs = [];
  if (parts.includes('volume')) jobs.push(readVolume().then((v) => { last.volume = v; }));
  if (parts.includes('net')) jobs.push(readNet().then((v) => { last.net = v; }));
  await Promise.all(jobs);
  const json = JSON.stringify(last);
  if (json !== state) { state = json; push(last); }
}

function init({ send, ipcMain }) {
  if (process.platform !== 'linux') {
    ipcMain.handle('sys:status', () => last);
    ipcMain.handle('sys:audio', () => {});
    return null;
  }
  push = (s) => send('sys:status', s);
  ipcMain.handle('sys:status', async () => { await refresh(); return last; });

  // 音量の操作。引数は固定の語だけ受け付ける
  ipcMain.handle('sys:audio', async (_e, cmd) => {
    if (cmd === 'up') await run('wpctl', ['set-volume', '-l', '1.0', '@DEFAULT_AUDIO_SINK@', '5%+']);
    else if (cmd === 'down') await run('wpctl', ['set-volume', '@DEFAULT_AUDIO_SINK@', '5%-']);
    else if (cmd === 'mute') await run('wpctl', ['set-mute', '@DEFAULT_AUDIO_SINK@', 'toggle']);
    else return;
    await refresh(['volume']);
  });

  refresh();
  setInterval(() => refresh(['battery', 'net']), 5000);

  // 音量の変化は pactl subscribe で即座に拾う(無ければ定期取得のみ)
  let timer = null;
  try {
    const sub = spawn('pactl', ['subscribe'], { stdio: ['ignore', 'pipe', 'ignore'] });
    sub.on('error', () => {});
    sub.stdout.on('data', (d) => {
      if (!/sink|server/.test(String(d))) return;
      clearTimeout(timer);
      timer = setTimeout(() => refresh(['volume']), 80);
    });
    process.on('exit', () => { try { sub.kill(); } catch {} });
  } catch {}
  setInterval(() => refresh(['volume']), 15000);
  return { refresh: () => refresh() };
}

module.exports = { init };
