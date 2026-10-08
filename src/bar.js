'use strict';
// ステータスバー(下端の細いバー)。左: ランチャー / 右: 音量・ネットワーク・電池・時計。
// app.js より先に読み込まれ、boot() から initBar() が呼ばれる(h / ico / api などは app.js 側の共通定義)。

const bar = { sys: { battery: null, volume: null, net: null }, media: null, notify: { history: [], unread: 0, dnd: false, serving: false }, el: {} };

function paintBar() {
  const { battery, volume, net } = bar.sys;
  const { vol, netEl, bat } = bar.el;
  vol.style.display = volume ? '' : 'none';
  if (volume) vol.textContent = volume.muted ? '音量 ミュート' : `音量 ${volume.percent}%`;
  netEl.style.display = net ? '' : 'none';
  if (net) {
    netEl.textContent = net.kind === 'wifi' ? `Wi-Fi ${net.name}` : net.kind === 'ethernet' ? '有線接続' : 'オフライン';
    netEl.classList.toggle('warn', net.kind === 'none');
  }
  bat.style.display = battery ? '' : 'none';
  if (battery) {
    const chg = battery.status === 'Charging' ? ' 充電中' : battery.status === 'Full' ? ' 満充電' : '';
    bat.textContent = `電池 ${battery.percent}%${chg}`;
    bat.classList.toggle('warn', battery.status === 'Discharging' && battery.percent <= 15);
  }
}

function paintMedia() {
  const m = bar.media;
  const el = bar.el.media;
  el.style.display = m ? '' : 'none';
  if (m) {
    el.textContent = `${m.status === 'Playing' ? '再生中' : '一時停止'}: ${[m.title, m.artist].filter(Boolean).join(' - ') || m.name.replace('org.mpris.MediaPlayer2.', '')}`;
    el.title = 'クリック: 再生/一時停止 / 右クリック: 前後の曲';
  }
}

function paintNotify() {
  const n = bar.notify;
  const el = bar.el.notify;
  el.textContent = `通知${n.unread ? ' ' + n.unread : ''}${n.dnd ? ' (おやすみ)' : ''}`;
  el.classList.toggle('warn', n.unread > 0);
}

const WEEK = ['日', '月', '火', '水', '木', '金', '土'];
function paintClock() {
  const d = new Date();
  const p = (n) => String(n).padStart(2, '0');
  bar.el.clock.textContent = `${d.getMonth() + 1}/${d.getDate()}(${WEEK[d.getDay()]}) ${p(d.getHours())}:${p(d.getMinutes())}`;
}

function initBar() {
  const root = document.getElementById('statusbar');
  const launch = h('button', { class: 'barbtn', title: 'ランチャー', onclick: () => openLauncher() }, ico('window'), 'アプリ・検索');
  // 音量: クリックでクイック設定、ホイールで±5%、中クリックでミュート
  const vol = h('span', {
    class: 'baritem clickable', title: 'クリック: クイック設定 / ホイール: 音量 / 中クリック: ミュート',
    onclick: () => openQuick(),
    onauxclick: (e) => { if (e.button === 1) api.sys.audio('mute'); },
    onwheel: (e) => { e.preventDefault(); api.sys.audio(e.deltaY < 0 ? 'up' : 'down'); },
  });
  const netEl = h('span', { class: 'baritem clickable', title: 'クイック設定', onclick: () => openQuick() });
  const bat = h('span', { class: 'baritem clickable', title: 'クイック設定', onclick: () => openQuick() });
  const notifyEl = h('span', { class: 'baritem clickable', title: '通知センター', onclick: () => openNotifications() });
  const media = h('span', {
    class: 'baritem clickable', style: { display: 'none' },
    onclick: () => api.sys.mediaCmd('playpause'),
    oncontextmenu: (e) => {
      e.preventDefault();
      popup([{ label: '前の曲', run: () => api.sys.mediaCmd('prev') }, { label: '次の曲', run: () => api.sys.mediaCmd('next') }], e.clientX, e.clientY - 70);
    },
  });
  const clock = h('span', { class: 'baritem', title: '' });
  Object.assign(bar.el, { vol, netEl, bat, clock, media, notify: notifyEl });
  root.replaceChildren(launch, media, h('span', { class: 'grow' }), notifyEl, vol, netEl, bat, clock);
  paintClock();
  setInterval(paintClock, 10000);
  api.sys.status().then((s) => { bar.sys = s || bar.sys; paintBar(); });
  api.sys.onStatus((s) => { bar.sys = s; paintBar(); });
  api.notify.state().then((s) => { bar.notify = s; paintNotify(); });
  api.notify.onState((s) => { bar.notify = s; paintNotify(); if (typeof notifUI !== 'undefined' && notifUI) paintNotifications(); });
  api.sys.media().then((m) => { bar.media = m; paintMedia(); });
  api.sys.onMedia((m) => { bar.media = m; paintMedia(); });
}
