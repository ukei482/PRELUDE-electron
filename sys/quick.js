'use strict';
// クイック設定(音量・Wi-Fi・Bluetooth・明るさ・電源プロファイル・セッション操作)。
// 画面側からは quick:get(状態の取得)と quick:do(操作)の2つだけを呼ぶ。操作は固定の種類だけ受け付け、引数は検証する。
const { exec } = require('./util');

const BR = ['--dest', 'org.kde.Solid.PowerManagement', '--object-path', '/org/kde/Solid/PowerManagement/Actions/BrightnessControl'];
const gdbus = (method, ...args) => exec('gdbus', ['call', '--session', ...BR, '--method', `org.kde.Solid.PowerManagement.Actions.BrightnessControl.${method}`, ...args]);
const MAC = /^[0-9A-F]{2}(:[0-9A-F]{2}){5}$/i;
const num = (v, lo, hi) => { const n = Math.round(Number(v)); return Number.isFinite(n) ? Math.min(hi, Math.max(lo, n)) : null; };

// ---- 音量 / 出力デバイス
// スクリーンショットの種類 → Spectacle のショートカット名(KDE の「ショートカット」設定の Spectacle の項目)
// screen は「マウスのある画面」だけ(FullScreenScreenShot はすべての画面をつないだ1枚になり、画面が2台あると何も映っていない側まで写る)
const SHOTS = { open: '_launch', region: 'RectangularRegionScreenShot', screen: 'CurrentMonitorScreenShot', window: 'ActiveWindowScreenShot' };

async function audio() {
  const [vol, st] = await Promise.all([exec('wpctl', ['get-volume', '@DEFAULT_AUDIO_SINK@']), exec('wpctl', ['status'])]);
  const m = vol.ok && /Volume:\s*([\d.]+)/.exec(vol.out);
  const sinks = [];
  let inSinks = false;
  for (const line of st.out.split('\n')) {
    if (/Sinks:/.test(line)) { inSinks = true; continue; }
    if (inSinks && /(Sink endpoints|Sources|Source endpoints|Filters|Streams):/.test(line)) break;
    const d = inSinks && /(\*)?\s*(\d+)\.\s+(.+?)\s+\[vol:/.exec(line);
    if (d) sinks.push({ id: Number(d[2]), name: d[3].trim(), current: !!d[1] });
  }
  return m ? { percent: Math.round(Number(m[1]) * 100), muted: /\[MUTED\]/.test(vol.out), sinks } : null;
}

// ---- Wi-Fi
const unesc = (s) => s.replace(/\\(.)/g, '$1');
const splitTerse = (line) => line.split(/(?<!\\):/).map(unesc); // nmcli -t は ':' を '\:' にする
async function wifi() {
  const radio = await exec('nmcli', ['radio', 'wifi']);
  if (!radio.ok) return null;
  const enabled = radio.out.trim() === 'enabled';
  if (!enabled) return { enabled, networks: [] };
  const [list, saved] = await Promise.all([
    exec('nmcli', ['-t', '-f', 'IN-USE,SSID,SIGNAL,SECURITY', 'dev', 'wifi', 'list', '--rescan', 'no']),
    exec('nmcli', ['-t', '-f', 'NAME,TYPE', 'con', 'show']),
  ]);
  const savedNames = new Set(saved.out.split('\n').map(splitTerse).filter((f) => f[1] === '802-11-wireless').map((f) => f[0]));
  const best = new Map(); // 同じSSIDの複数アクセスポイントは1つにまとめる(使用中 > 電波が強い順)
  for (const line of list.out.split('\n')) {
    const [inUse, ssid, signal, security] = splitTerse(line);
    if (!ssid) continue;
    const n = { ssid, signal: Number(signal) || 0, secured: !!security, active: inUse === '*', saved: savedNames.has(ssid) };
    const o = best.get(ssid);
    if (!o || (n.active && !o.active) || (n.active === o.active && n.signal > o.signal)) best.set(ssid, n);
  }
  return { enabled, networks: [...best.values()].sort((a, b) => (b.active - a.active) || (b.saved - a.saved) || (b.signal - a.signal)).slice(0, 12) };
}

async function wifiConnect(ssid) {
  if (typeof ssid !== 'string' || !ssid) return { error: '無効なネットワーク名です' };
  const w = await wifi();
  const n = w?.networks.find((x) => x.ssid === ssid);
  if (!n) return { error: 'ネットワークが見つかりません' };
  if (!n.saved && n.secured) return { error: 'パスワードが必要です。先にKDEのネットワーク設定で一度登録してください' };
  const r = n.saved ? await exec('nmcli', ['--wait', '15', 'con', 'up', 'id', ssid], 20000) : await exec('nmcli', ['--wait', '15', 'dev', 'wifi', 'connect', ssid], 20000);
  return r.ok ? {} : { error: '接続できませんでした' };
}

// ---- Bluetooth
async function bluetooth() {
  const show = await exec('bluetoothctl', ['show']);
  if (!show.ok || !/Powered:/.test(show.out)) return null;
  const powered = /Powered:\s*yes/.test(show.out);
  if (!powered) return { powered, devices: [] };
  const list = await exec('bluetoothctl', ['devices', 'Paired']);
  const devices = [];
  for (const line of list.out.split('\n')) {
    const m = /^Device\s+([0-9A-F:]{17})\s+(.+)$/i.exec(line);
    if (!m) continue;
    const info = await exec('bluetoothctl', ['info', m[1]]);
    devices.push({ mac: m[1], name: m[2], connected: /Connected:\s*yes/.test(info.out) });
  }
  return { powered, devices: devices.slice(0, 10) };
}

// ---- 明るさ / 電源プロファイル
async function brightness() {
  const [v, max] = await Promise.all([gdbus('brightness'), gdbus('brightnessMax')]);
  const a = /(\d+)/.exec(v.out), b = /(\d+)/.exec(max.out);
  return v.ok && max.ok && a && b && Number(b[1]) > 0 ? { percent: Math.round((Number(a[1]) / Number(b[1])) * 100) } : null;
}
async function profile() {
  const [cur, list] = await Promise.all([exec('powerprofilesctl', ['get']), exec('powerprofilesctl', ['list'])]);
  if (!cur.ok) return null;
  const available = [...list.out.matchAll(/^[* ]\s*([a-z-]+):/gm)].map((m) => m[1]);
  return { current: cur.out.trim(), available };
}

async function get() {
  const [a, w, b, br, p] = await Promise.all([audio(), wifi(), bluetooth(), brightness(), profile()]);
  return { audio: a, wifi: w, bluetooth: b, brightness: br, profile: p };
}

async function doOp(op, arg) {
  switch (op) {
    case 'volume': { const n = num(arg, 0, 100); return n == null ? {} : exec('wpctl', ['set-volume', '-l', '1.0', '@DEFAULT_AUDIO_SINK@', `${n}%`]); }
    case 'mute': return exec('wpctl', ['set-mute', '@DEFAULT_AUDIO_SINK@', 'toggle']);
    case 'sink': { const n = num(arg, 1, 1e6); return n == null ? {} : exec('wpctl', ['set-default', String(n)]); }
    case 'wifiPower': return exec('nmcli', ['radio', 'wifi', arg ? 'on' : 'off']);
    case 'wifiConnect': return wifiConnect(arg);
    case 'btPower': return exec('bluetoothctl', ['power', arg ? 'on' : 'off']);
    case 'btConnect': case 'btDisconnect':
      if (!MAC.test(String(arg))) return {};
      return exec('bluetoothctl', [op === 'btConnect' ? 'connect' : 'disconnect', arg], 15000);
    case 'brightness': {
      const n = num(arg, 1, 100); // 0%だと画面が真っ暗になって戻せなくなるので下限は1%(暗い部屋で使う人もいるので、見える範囲までは下げられる)
      const max = /(\d+)/.exec((await gdbus('brightnessMax')).out);
      return n == null || !max ? {} : gdbus('setBrightness', String(Math.round((n / 100) * Number(max[1]))));
    }
    case 'profile': {
      const p = await profile();
      return p && p.available.includes(arg) ? exec('powerprofilesctl', ['set', arg]) : {};
    }
    // スクリーンショット。Spectacle のグローバルショートカットを呼ぶ(Print キーを押したのと同じ。plasmashell が無くても動く)。
    // 押したパネルやランチャーが写り込まないよう、閉じて描き直されるのを少し待ってから撮る
    case 'screenshot': {
      const action = Object.hasOwn(SHOTS, arg) ? SHOTS[arg] : null;
      if (!action) return {};
      if (arg !== 'open') await new Promise((r) => setTimeout(r, 350));
      const r = await exec('gdbus', ['call', '--session', '--dest', 'org.kde.kglobalaccel', '--object-path', '/component/org_kde_spectacle_desktop',
        '--method', 'org.kde.kglobalaccel.Component.invokeShortcut', action]);
      return r.ok ? {} : { error: 'スクリーンショットを撮れませんでした(Spectacle が見つかりません)' };
    }
    case 'lock': return exec('loginctl', ['lock-session']);
    case 'suspend': return exec('systemctl', ['suspend']);
    // ログアウト・再起動・シャットダウンは、KDE自身の確認ダイアログを出す
    case 'logout': case 'reboot': case 'shutdown': {
      const m = { logout: 'promptLogout', reboot: 'promptReboot', shutdown: 'promptShutDown' }[op];
      return exec('gdbus', ['call', '--session', '--dest', 'org.kde.LogoutPrompt', '--object-path', '/LogoutPrompt', '--method', `org.kde.LogoutPrompt.${m}`]);
    }
    default: return {};
  }
}

function init({ ipcMain, refreshStatus, onSelfChange }) {
  ipcMain.handle('quick:get', () => get());
  ipcMain.handle('quick:do', async (_e, op, arg) => {
    if (['volume', 'mute', 'brightness'].includes(String(op))) onSelfChange?.(); // 自分の操作(スライダー)ではOSDを出さない
    const r = await doOp(String(op), arg);
    refreshStatus?.();
    return { error: r?.error || '' };
  });
}

module.exports = { init, doOp };
