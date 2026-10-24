'use strict';
// クイック設定パネル(バーの音量・Wi-Fi・電池をクリック)。音量・Wi-Fi・Bluetooth・明るさ・電源プロファイル・セッション操作。
// 状態の取得と操作は main 側の sys/quick.js(api.quick.get / do)。app.js より先に読み込まれる。

let quickUI = null;

// PRELUDE が plasmashell の代わり(prelude-shell.service)として動いているときだけ、Kubuntu(plasmashell)に戻せる
// (状態の取得は app.js の boot から呼ぶ。このファイルは app.js より先に読み込まれ、その時点では api がまだ使えない。
//  ここで呼ぶと例外でこのファイルの残りが実行されず、下の returning が未初期化のまま「Kubuntuに戻る」が動かなくなる)
let sessionShell = false;
function loadSessionState() { return api.session.state().then((s) => { sessionShell = !!s?.shell; }); }

// 戻る手順(scripts/prelude-return.sh)を始める。成功すると PRELUDE 自身が終了するので、その間は画面に案内を出す
let returning = false;
async function returnToKde() {
  if (returning) return;
  returning = true;
  toast('Returning to Kubuntu… (takes a few seconds; stays on PRELUDE if it fails)');
  const r = await api.session.returnToKde();
  if (r?.error) { returning = false; toast(r.error); }
}

// PRELUDE の更新(main 側の sys/update.js)。新しい版があれば作って切り替え、PRELUDE 自身を再起動する
let updating = false;
async function runUpdate() {
  if (updating) return;
  updating = true;
  toast('Checking and preparing the new version… (may take about a minute; restarts when done)', 8000);
  const r = await api.update.run();
  updating = false;
  if (r?.error) toast(r.error, 6000);
  else if (r?.updated) toast(`Updated to ${r.updated}. Restarting…`, 10000);
  else toast(`Up to date (${r?.latest || ''})`);
  return r;
}

// 「Kubuntuに戻る」ボタン(クイック設定と設定ペインで共通)。
// 誤クリックで画面が切り替わらないよう、押すと「もう一度押すと実行」に変わる(3秒で元に戻る)
function returnButton(cls, before) {
  const back = h('button', { class: cls, title: 'Quit PRELUDE and return to the Kubuntu (plasmashell) desktop' }, 'Return to Kubuntu');
  let armed = 0;
  back.onclick = () => {
    if (!armed) {
      back.textContent = 'Press again to return';
      back.classList.add('danger');
      armed = setTimeout(() => { armed = 0; back.textContent = 'Return to Kubuntu'; back.classList.remove('danger'); }, 3000);
      return;
    }
    clearTimeout(armed);
    armed = 0;
    before?.();
    returnToKde();
  };
  return back;
}

function closeQuick() {
  if (!quickUI) return;
  quickUI.bd.remove();
  quickUI.box.remove();
  quickUI = null;
  suspend(-1);
}

const PROFILE_LABEL = { performance: 'Performance', balanced: 'Balanced', 'power-saver': 'Power saver' };
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
  const toggle = (on, op) => h('button', { class: 'textbtn' + (on ? ' primary' : ''), onclick: () => act(op, !on) }, on ? 'ON' : 'OFF');
  const slider = (value, op, min = 0) => {
    const send = throttle((v) => act(op, v, false), 80);
    return h('input', { type: 'range', min, max: 100, value, oninput: (e) => send(Number(e.target.value)) });
  };

  function paint(d) {
    const kids = [];
    const mp = bar.media;
    if (mp) {
      kids.push(section('Now playing',
        h('div', { class: 'qrow' }, h('span', { class: 'qgrow' }, [mp.title, mp.artist].filter(Boolean).join(' / ') || mp.name.replace('org.mpris.MediaPlayer2.', '')),
          h('button', { class: 'textbtn', onclick: () => api.sys.mediaCmd('prev') }, 'Prev'),
          h('button', { class: 'textbtn', onclick: () => api.sys.mediaCmd('playpause') }, mp.status === 'Playing' ? 'Pause' : 'Play'),
          h('button', { class: 'textbtn', onclick: () => api.sys.mediaCmd('next') }, 'Next'))));
    }
    if (d.audio) {
      const a = d.audio;
      kids.push(section('Volume',
        h('div', { class: 'qrow' }, slider(a.percent, 'volume'), h('button', { class: 'textbtn' + (a.muted ? ' primary' : ''), onclick: () => act('mute') }, 'Mute')),
        a.sinks.length > 1 ? h('select', { class: 'qsel', onchange: (e) => act('sink', Number(e.target.value)) },
          a.sinks.map((s) => h('option', { value: s.id, selected: s.current || null }, s.name))) : null));
    }
    if (d.brightness) kids.push(section('Brightness', h('div', { class: 'qrow' }, slider(d.brightness.percent, 'brightness', 1))));
    if (d.wifi) {
      kids.push(section('Wi-Fi',
        h('div', { class: 'qrow' }, h('span', { class: 'qgrow' }, d.wifi.enabled ? 'Enabled' : 'Disabled'), toggle(d.wifi.enabled, 'wifiPower')),
        ...d.wifi.networks.map((n) => h('div', {
          class: 'qitem' + (n.active ? ' on' : ''), title: n.saved ? '' : 'Unsaved network',
          onclick: () => { if (!n.active) act('wifiConnect', n.ssid); },
        }, h('span', { class: 'qgrow' }, n.ssid), h('span', { class: 'qmeta' }, `${n.active ? 'Connected ' : ''}${n.secured ? 'Locked ' : ''}${n.signal}%`)))));
    }
    if (d.bluetooth) {
      kids.push(section('Bluetooth',
        h('div', { class: 'qrow' }, h('span', { class: 'qgrow' }, d.bluetooth.powered ? 'Enabled' : 'Disabled'), toggle(d.bluetooth.powered, 'btPower')),
        ...d.bluetooth.devices.map((v) => h('div', {
          class: 'qitem' + (v.connected ? ' on' : ''), onclick: () => act(v.connected ? 'btDisconnect' : 'btConnect', v.mac),
        }, h('span', { class: 'qgrow' }, v.name), h('span', { class: 'qmeta' }, v.connected ? 'Connected' : '')))));
    }
    if (d.profile && d.profile.available.length) {
      kids.push(section('Power profile', h('div', { class: 'qrow' },
        d.profile.available.map((p) => h('button', { class: 'textbtn qgrow' + (p === d.profile.current ? ' primary' : ''), onclick: () => act('profile', p) }, PROFILE_LABEL[p] || p)))));
    }
    kids.push(section('Screenshot', h('div', { class: 'qrow qwrap' },
      [['Region', 'region'], ['Screen', 'screen'], ['Open Spectacle', 'open']].map(([l, kind]) =>
        h('button', { class: 'textbtn', onclick: () => { closeQuick(); act('screenshot', kind, false); } }, l)))));
    kids.push(section('Session', h('div', { class: 'qrow qwrap' },
      [['Lock', 'lock'], ['Sleep', 'suspend'], ['Log out', 'logout'], ['Restart', 'reboot'], ['Shut down', 'shutdown']].map(([l, op]) =>
        h('button', { class: 'textbtn', onclick: () => { closeQuick(); act(op, null, false); } }, l)))));
    if (sessionShell) kids.push(section('Desktop', h('div', { class: 'qrow' }, returnButton('textbtn qgrow', closeQuick))));
    const top = box.scrollTop;
    box.replaceChildren(...kids);
    box.scrollTop = top;
  }
  paint({ audio: null, wifi: null, bluetooth: null, brightness: null, profile: null });
  const d = await api.quick.get();
  if (quickUI && quickUI.box === box) paint(d);
}
