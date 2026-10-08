'use strict';
// ホーム画面(ペインの種類「ホーム」)。壁紙・時計・最近使ったアプリ・ブックマーク・クイック操作。
// app.js より先に読み込まれる。h / ico / S / C / api / openTab / openBookmark / launchApp などは呼び出し時に app.js・launcher.js のものを使う。

const WEEKDAY = ['日', '月', '火', '水', '木', '金', '土'];
const wall = { path: null, url: null }; // 壁紙は大きいので、設定のパスが変わったときだけ読み直す
const iconCache = new Map(); // アイコン名 -> data URL | null

async function getWallpaper() {
  const want = (C.appearance.wallpaper || '').trim();
  if (wall.path !== want) {
    const r = await api.wallpaper();
    wall.path = r.path;
    wall.url = r.url;
  }
  return wall.url;
}

async function appIconUrl(name) {
  if (!name) return null;
  if (!iconCache.has(name)) iconCache.set(name, await api.appIcon(name));
  return iconCache.get(name);
}

function buildHome(tab, leaf, head, body) {
  head.append(ico('home'), h('span', { class: 'ptitle' }, 'ホーム'), ...commonButtons(tab, leaf));
  const clock = h('div', { class: 'hclock' });
  const date = h('div', { class: 'hdate' });
  const apps = h('div', { class: 'hgrid' });
  const marks = h('div', { class: 'hchips' });
  const root = h('div', { class: 'home' },
    h('div', { class: 'hhero' }, clock, date),
    h('div', { class: 'hcard' }, h('div', { class: 'hh' }, '最近使ったアプリ'), apps),
    h('div', { class: 'hcard' }, h('div', { class: 'hh' }, 'ブックマーク'), marks),
    h('div', { class: 'hcard hquick' },
      h('button', { class: 'textbtn', onclick: () => openLauncher() }, ico('window'), 'アプリ・検索（Ctrl+Shift+P）'),
      h('button', { class: 'textbtn', onclick: () => openTab('files') }, ico('folder'), 'フォルダ'),
      h('button', { class: 'textbtn', onclick: () => openTab('web') }, ico('globe'), 'Web'),
      h('button', { class: 'textbtn', onclick: () => openSettings() }, ico('settings'), '設定')));
  body.append(root);

  const paintClock = () => {
    const d = new Date();
    const p = (n) => String(n).padStart(2, '0');
    clock.textContent = `${p(d.getHours())}:${p(d.getMinutes())}`;
    date.textContent = `${d.getMonth() + 1}月${d.getDate()}日 ${WEEKDAY[d.getDay()]}曜日`;
  };
  paintClock(); // この時点ではまだ画面に取り付いていない(isConnected は偽)ので、初回は無条件に描く
  const timer = setInterval(() => {
    if (!root.isConnected) clearInterval(timer); // 画面から外れたら止める
    else paintClock();
  }, 10000);

  getWallpaper().then((url) => {
    if (!url) return;
    root.style.backgroundImage = `url("${url}")`;
    root.classList.add('has-wall');
  });

  // よく使うアプリ(ランチャーで開いた回数・新しさの順)。最近のものが無ければ案内を出す
  const paintApps = (list) => {
    const used = (C.appUsage || []).map((u) => list.find((a) => a.exec === u.exec)).filter(Boolean).slice(0, 10);
    apps.replaceChildren(...(used.length ? used.map((a) => {
      const img = h('img', { class: 'hicon' });
      const tile = h('div', { class: 'htile', title: a.exec, onclick: () => launchApp(a) }, h('span', { class: 'hicowrap' }, ico('window'), img), h('span', { class: 'hname' }, a.name));
      img.style.display = 'none';
      appIconUrl(a.icon).then((u) => { if (u) { img.src = u; img.style.display = ''; tile.querySelector('.ico').style.display = 'none'; } });
      return tile;
    }) : [h('div', { class: 'empty' }, 'ランチャーでアプリを開くと、ここに並びます')]));
  };
  if (S.appList) paintApps(S.appList);
  else api.app.list().then((l) => { S.appList = l; if (root.isConnected) paintApps(l); });

  marks.replaceChildren(...(S.bookmarks.length ? S.bookmarks.map((b) => h('button', {
    class: 'textbtn hchip', title: b.target, onclick: () => openBookmark(b),
  }, ico(b.kind === 'web' ? 'globe' : b.kind === 'app' ? 'window' : 'folder'), b.title || b.target))
    : [h('div', { class: 'empty' }, 'ペインの ☆ で追加')]));
}
