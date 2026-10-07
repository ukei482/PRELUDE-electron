'use strict';
// クイック設定パネル(バーの音量・Wi-Fi・電池をクリック)。音量・Wi-Fi・Bluetooth・明るさ・電源プロファイル・セッション操作。
// 状態の取得と操作は main 側の sys/quick.js(api.quick.get / do)。app.js より先に読み込まれる。

let quickUI = null;

function closeQuick() {
  if (!quickUI) return;
  quickUI.bd.remove();
  quickUI.box.remove();
  quickUI = null;
  suspend(-1);
}

const PROFILE_LABEL = { performance: '高性能', balanced: 'バランス', 'power-saver': '省電力' };
const throttle = (fn, ms) => {
  let t = 0, pending = null;
  return (...a) => {
    pending = a;
    if (t) return;
    t = setTimeout(() => { t = 0; fn(...pending); }, ms);
  };
};

async function openQuick() {
  if (quickUI) { closeQuick(); return; }
  const bd = h('div', { class: 'backdrop', onpointerdown: closeQuick });
  const box = h('div', { class: 'qpanel' });
  overlay.append(bd, box);
  quickUI = { bd, box };
  suspend(+1);

  const act = async (op, arg, repaint = true) => {
    const r = await api.quick.do(op, arg);
    if (r?.error) toast(r.error);
    if (repaint && quickUI) paint(await api.quick.get());
  };
  const section = (title, ...kids) => h('div', { class: 'qsec' }, h('div', { class: 'qh' }, title), ...kids);
  const toggle = (on, op) => h('button', { class: 'textbtn' + (on ? ' primary' : ''), onclick: () => act(op, !on) }, on ? 'オン' : 'オフ');
  const slider = (value, op, min = 0) => {
    const send = throttle((v) => act(op, v, false), 80);
    return h('input', { type: 'range', min, max: 100, value, oninput: (e) => send(Number(e.target.value)) });
  };

  function paint(d) {
    const kids = [];
    const mp = bar.media;
    if (mp) {
      kids.push(section('再生中',
        h('div', { class: 'qrow' }, h('span', { class: 'qgrow' }, [mp.title, mp.artist].filter(Boolean).join(' / ') || mp.name.replace('org.mpris.MediaPlayer2.', '')),
          h('button', { class: 'textbtn', onclick: () => api.sys.mediaCmd('prev') }, '前'),
          h('button', { class: 'textbtn', onclick: () => api.sys.mediaCmd('playpause') }, mp.status === 'Playing' ? '一時停止' : '再生'),
          h('button', { class: 'textbtn', onclick: () => api.sys.mediaCmd('next') }, '次'))));
    }
    if (d.audio) {
      const a = d.audio;
      kids.push(section('音量',
        h('div', { class: 'qrow' }, slider(a.percent, 'volume'), h('button', { class: 'textbtn' + (a.muted ? ' primary' : ''), onclick: () => act('mute') }, 'ミュート')),
        a.sinks.length > 1 ? h('select', { class: 'qsel', onchange: (e) => act('sink', Number(e.target.value)) },
          a.sinks.map((s) => h('option', { value: s.id, selected: s.current || null }, s.name))) : null));
    }
    if (d.brightness) kids.push(section('明るさ', h('div', { class: 'qrow' }, slider(d.brightness.percent, 'brightness', 5))));
    if (d.wifi) {
      kids.push(section('Wi-Fi',
        h('div', { class: 'qrow' }, h('span', { class: 'qgrow' }, d.wifi.enabled ? '有効' : '無効'), toggle(d.wifi.enabled, 'wifiPower')),
        ...d.wifi.networks.map((n) => h('div', {
          class: 'qitem' + (n.active ? ' on' : ''), title: n.saved ? '' : '未登録のネットワーク',
          onclick: () => { if (!n.active) act('wifiConnect', n.ssid); },
        }, h('span', { class: 'qgrow' }, n.ssid), h('span', { class: 'qmeta' }, `${n.active ? '接続中 ' : ''}${n.secured ? '鍵 ' : ''}${n.signal}%`)))));
    }
    if (d.bluetooth) {
      kids.push(section('Bluetooth',
        h('div', { class: 'qrow' }, h('span', { class: 'qgrow' }, d.bluetooth.powered ? '有効' : '無効'), toggle(d.bluetooth.powered, 'btPower')),
        ...d.bluetooth.devices.map((v) => h('div', {
          class: 'qitem' + (v.connected ? ' on' : ''), onclick: () => act(v.connected ? 'btDisconnect' : 'btConnect', v.mac),
        }, h('span', { class: 'qgrow' }, v.name), h('span', { class: 'qmeta' }, v.connected ? '接続中' : '')))));
    }
    if (d.profile && d.profile.available.length) {
      kids.push(section('電源プロファイル', h('div', { class: 'qrow' },
        d.profile.available.map((p) => h('button', { class: 'textbtn qgrow' + (p === d.profile.current ? ' primary' : ''), onclick: () => act('profile', p) }, PROFILE_LABEL[p] || p)))));
    }
    kids.push(section('セッション', h('div', { class: 'qrow qwrap' },
      [['ロック', 'lock'], ['スリープ', 'suspend'], ['ログアウト', 'logout'], ['再起動', 'reboot'], ['電源オフ', 'shutdown']].map(([l, op]) =>
        h('button', { class: 'textbtn', onclick: () => { closeQuick(); act(op, null, false); } }, l)))));
    const top = box.scrollTop;
    box.replaceChildren(...kids);
    box.scrollTop = top;
  }
  paint({ audio: null, wifi: null, bluetooth: null, brightness: null, profile: null });
  const d = await api.quick.get();
  if (quickUI && quickUI.box === box) paint(d);
}
