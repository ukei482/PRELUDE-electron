'use strict';
// ステータスバー(下端の細いバー)。左: ランチャー / 右: 音量・ネットワーク・電池・時計。
// app.js より先に読み込まれ、boot() から initBar() が呼ばれる(h / ico / api などは app.js 側の共通定義)。

const bar = { sys: { battery: null, volume: null, net: null }, media: null, notify: { history: [], unread: 0, dnd: false, serving: false }, el: {} };

function paintBar() {
  const { battery, volume, net } = bar.sys;
  const { vol, netEl, bat } = bar.el;
  vol.style.display = volume ? '' : 'none';
  if (volume) vol.textContent = volume.muted ? 'VOL MUTE' : `VOL ${volume.percent}%`;
  netEl.style.display = net ? '' : 'none';
  if (net) {
    netEl.textContent = net.kind === 'wifi' ? `Wi-Fi ${net.name}` : net.kind === 'ethernet' ? 'Ethernet' : 'Offline';
    netEl.classList.toggle('warn', net.kind === 'none');
  }
  bat.style.display = battery ? '' : 'none';
  if (battery) {
    const chg = battery.status === 'Charging' ? ' CHG' : battery.status === 'Full' ? ' FULL' : '';
    bat.textContent = `BAT ${battery.percent}%${chg}`;
    bat.classList.toggle('warn', battery.status === 'Discharging' && battery.percent <= 15);
  }
}

function paintMedia() {
  const m = bar.media;
  const el = bar.el.media;
  el.style.display = m ? '' : 'none';
  if (m) {
    el.textContent = `${m.status === 'Playing' ? 'Playing' : 'Paused'}: ${[m.title, m.artist].filter(Boolean).join(' - ') || m.name.replace('org.mpris.MediaPlayer2.', '')}`;
    el.title = 'Click: play/pause / Right-click: prev/next';
  }
}

function paintNotify() {
  const n = bar.notify;
  const el = bar.el.notify;
  el.textContent = `NOTIF${n.unread ? ' ' + n.unread : ''}${n.dnd ? ' (DND)' : ''}`;
  el.classList.toggle('warn', n.unread > 0);
}

const WEEK = ['SUN', 'MON', 'TUE', 'WED', 'THU', 'FRI', 'SAT'];
function paintClock() {
  const d = new Date();
  const p = (n) => String(n).padStart(2, '0');
  bar.el.clock.textContent = `${WEEK[d.getDay()]} ${d.getMonth() + 1}/${d.getDate()} ${p(d.getHours())}:${p(d.getMinutes())}`;
}

function initBar() {
  const root = document.getElementById('statusbar');
  const launch = h('button', { class: 'barbtn', title: 'Launcher', onclick: () => openLauncher() }, ico('window'), 'Apps / Search');
  // 音量: クリックでクイック設定、ホイールで±5%、中クリックでミュート
  const vol = h('span', {
    class: 'baritem clickable', title: 'Click: quick settings / Wheel: volume / Middle click: mute',
    onclick: () => openQuick(),
    onauxclick: (e) => { if (e.button === 1) api.sys.audio('mute'); },
    onwheel: (e) => { e.preventDefault(); api.sys.audio(e.deltaY < 0 ? 'up' : 'down'); },
  });
  const netEl = h('span', { class: 'baritem clickable', title: 'Quick settings', onclick: () => openQuick() });
  const bat = h('span', { class: 'baritem clickable', title: 'Quick settings', onclick: () => openQuick() });
  const trayEl = h('span', { class: 'traybox', style: { display: 'none' } });
  const notifyEl = h('span', { class: 'baritem clickable', title: 'Notifications', onclick: () => openNotifications() });
  const media = h('span', {
    class: 'baritem clickable', style: { display: 'none' },
    onclick: () => api.sys.mediaCmd('playpause'),
    oncontextmenu: (e) => {
      e.preventDefault();
      popup([{ label: 'Previous track', run: () => api.sys.mediaCmd('prev') }, { label: 'Next track', run: () => api.sys.mediaCmd('next') }], e.clientX, e.clientY - 70);
    },
  });
  const clock = h('span', { class: 'baritem', title: '' });
  Object.assign(bar.el, { vol, netEl, bat, clock, media, notify: notifyEl, tray: trayEl });
  // 2枚目以降の画面のバーは、ランチャーと時計だけ(音量・通知・トレイなどシステム全体のものは主画面に1つ)
  if (IS_SUB) root.replaceChildren(launch, h('span', { class: 'grow' }), clock);
  else root.replaceChildren(launch, media, h('span', { class: 'grow' }), trayEl, notifyEl, vol, netEl, bat, clock);
  paintClock();
  setInterval(paintClock, 10000);
  api.sys.status().then((s) => { bar.sys = s || bar.sys; paintBar(); });
  api.sys.onStatus((s) => { bar.sys = s; paintBar(); });
  api.tray.items().then((l) => { bar.tray = l; paintTray(); });
  api.tray.onItems((l) => { bar.tray = l; paintTray(); });
  api.notify.state().then((s) => { bar.notify = s; paintNotify(); });
  api.notify.onState((s) => { bar.notify = s; paintNotify(); if (typeof notifUI !== 'undefined' && notifUI) paintNotifications(); });
  api.sys.media().then((m) => { bar.media = m; paintMedia(); });
  api.sys.onMedia((m) => { bar.media = m; paintMedia(); });
}
