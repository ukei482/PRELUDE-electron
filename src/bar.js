'use strict';
// 上のバーの右側のボタン。左から Bluetooth・電池・Wi-Fi・時計で、どれも [ ] で囲んで出す。
// Bluetooth・電池・Wi-Fi はクイック設定、時計は通知センターを開く。
// app.js より先に読み込まれ、boot() から initBar() が呼ばれる(h / ico / api などは app.js 側の共通定義)。

const bar = { sys: { battery: null, volume: null, net: null, bluetooth: null }, media: null, notify: { history: [], unread: 0, dnd: false, serving: false }, el: {} };

// 中身が無いとき(電池の無い PC・Bluetooth の無い PC)はボタンごと隠す。warn(電池が少ない・オフライン)は反転して目立たせる
function setBtn(el, text, warn = false) {
  el.style.display = text ? '' : 'none';
  if (text) el.querySelector('.tl').textContent = text;
  el.classList.toggle('warn', warn);
}

function paintBar() {
  const { battery, net, bluetooth } = bar.sys;
  const { netEl, bat, bt } = bar.el;
  if (!netEl) return;
  setBtn(netEl, !net ? '' : net.kind === 'wifi' ? `WIFI ${net.name}` : net.kind === 'ethernet' ? 'ETHERNET' : 'OFFLINE', net?.kind === 'none');
  setBtn(bat, battery ? `BAT ${battery.percent}%${battery.status === 'Charging' ? ' CHG' : battery.status === 'Full' ? ' FULL' : ''}` : '',
    battery?.status === 'Discharging' && battery.percent <= 15);
  setBtn(bt, bluetooth ? `BT ${bluetooth.powered ? 'ON' : 'OFF'}${bluetooth.connected ? ` (${bluetooth.connected})` : ''}` : '');
}

const WEEK = ['SUN', 'MON', 'TUE', 'WED', 'THU', 'FRI', 'SAT'];
function paintClock() {
  const d = new Date();
  const p = (n) => String(n).padStart(2, '0');
  // 未読の通知があれば先頭に * を付ける
  setBtn(bar.el.clock, `${bar.notify.unread ? '*' : ''}${WEEK[d.getDay()]} ${d.getMonth() + 1}/${d.getDate()} ${p(d.getHours())}:${p(d.getMinutes())}`);
}

function initBar() {
  const btn = (title, onclick) => h('button', { class: 'tbtn', title, onclick }, '[', h('span', { class: 'tl' }), ']');
  const bt = btn('Bluetooth (quick settings)', () => openQuick());
  const bat = btn('Battery (quick settings)', () => openQuick());
  const netEl = btn('Network (quick settings)', () => openQuick());
  const clock = btn('Notifications', () => openNotifications());
  Object.assign(bar.el, { bt, bat, netEl, clock });
  // 2枚目以降の画面は時計だけ(Bluetooth・電池・Wi-Fi・通知はシステム全体のものなので主画面に1つ)
  document.getElementById('topbar').append(h('span', { class: 'grow' }), ...(IS_SUB ? [clock] : [bt, bat, netEl, clock]));
  if (IS_SUB) Object.assign(clock, { onclick: null, title: '' });
  paintClock();
  setInterval(paintClock, 10000);
  if (IS_SUB) return;
  api.sys.status().then((s) => { bar.sys = s || bar.sys; paintBar(); });
  api.sys.onStatus((s) => { bar.sys = s; paintBar(); });
  // 通知センター・クイック設定が使う状態
  api.notify.state().then((s) => { bar.notify = s; paintClock(); });
  api.notify.onState((s) => { bar.notify = s; paintClock(); if (typeof notifUI !== 'undefined' && notifUI) paintNotifications(); });
  api.sys.media().then((m) => { bar.media = m; });
  api.sys.onMedia((m) => { bar.media = m; });
}
