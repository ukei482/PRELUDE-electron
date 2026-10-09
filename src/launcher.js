'use strict';
// ランチャー(ショートカット or バーのボタン)。開いているタブ・ブックマーク・アプリ・コマンド・Web検索を1つの入力欄で探す。
// app.js より先に読み込まれる。S / openTab / startApp / suspend / overlay などは app.js 側の共通定義を呼び出し時に使う。

let launcherUI = null;

function closeLauncher() {
  if (!launcherUI) return;
  launcherUI.bd.remove();
  launcherUI.box.remove();
  launcherUI = null;
  suspend(-1);
}

// アプリを開く。既定はペインに取り込む(設定で別ウィンドウにもできる)。取り込み未対応の環境では別ウィンドウ
function launchApp(a) {
  const usage = (C.appUsage || []).filter((u) => u.exec !== a.exec);
  usage.unshift({ exec: a.exec, n: ((C.appUsage || []).find((u) => u.exec === a.exec)?.n || 0) + 1 });
  api.config.set('appUsage', usage.slice(0, 50));
  if (!S.appSupported || C.behavior.appOpen === 'window') {
    api.app.spawn(a.exec).then((ok) => { if (!ok) toast('起動できませんでした'); });
    return;
  }
  const t = openTab('app', { cmd: a.exec, cls: a.cls, name: a.name });
  startApp(t.root, a.exec, a.cls, a.name);
}

const LAUNCHER_COMMANDS = [
  { name: '設定', keys: 'settings せってい', run: () => openSettings() },
  { name: 'ホーム画面', keys: 'home ほーむ', run: () => openTab('home') },
  { name: '新しいタブ(Web)', keys: 'new tab web あたらしい', run: () => openTab('web') },
  { name: '新しいタブ(フォルダ)', keys: 'new tab folder files あたらしい', run: () => openTab('files') },
  { name: '全画面の切り替え', keys: 'fullscreen ぜんがめん', run: () => api.win.cmd('toggleFullscreen') },
  { name: 'Kubuntuに戻る', keys: 'kubuntu plasma desktop return もどる', when: () => sessionShell, run: () => returnToKde() },
];

// 前方一致を先に、部分一致を後に
function matchRank(text, q) {
  const t = text.toLowerCase();
  if (t.startsWith(q)) return 0;
  if (t.split(/[\s\-_.]+/).some((w) => w.startsWith(q))) return 1;
  return t.includes(q) ? 2 : -1;
}

function launcherEntries(q) {
  q = q.trim().toLowerCase();
  const out = [];
  const used = new Map((C.appUsage || []).map((u, i) => [u.exec, i])); // 小さいほど最近・よく使う
  const usedRank = (a) => (used.has(a.exec) ? used.get(a.exec) : 999);

  if (!q) {
    for (const a of (S.appList || []).filter((x) => used.has(x.exec)).sort((x, y) => usedRank(x) - usedRank(y)).slice(0, 8)) {
      out.push({ kind: 'アプリ', name: a.name, run: () => launchApp(a) });
    }
    return out;
  }
  const pick = (items, text, rank = (x) => 0) => items
    .map((x) => ({ x, r: matchRank(text(x), q) }))
    .filter((m) => m.r >= 0)
    .sort((a, b) => a.r - b.r || rank(a.x) - rank(b.x));

  for (const { x: tab } of pick(S.tabs, tabTitle)) out.push({ kind: 'タブ', name: tabTitle(tab), run: () => showTab(tab) });
  for (const { x: b } of pick(S.bookmarks, (b) => b.title || b.target)) out.push({ kind: 'ブックマーク', name: b.title || b.target, run: () => openBookmark(b) });
  for (const { x: a } of pick(S.appList || [], (a) => a.name, usedRank).slice(0, 8)) out.push({ kind: 'アプリ', name: a.name, run: () => launchApp(a) });
  for (const { x: c } of pick(LAUNCHER_COMMANDS.filter((c) => !c.when || c.when()), (c) => c.name + ' ' + c.keys)) out.push({ kind: 'コマンド', name: c.name, run: c.run });
  out.push({ kind: 'Web', name: `Webで開く／検索: ${q}`, run: () => openTab('web', { url: q }) });
  return out.slice(0, 14);
}

async function openLauncher() {
  if (launcherUI) { closeLauncher(); return; }
  if (!S.appList) S.appList = []; // 一覧の取得を待たずに開く
  api.app.list().then((l) => { S.appList = l; if (launcherUI) launcherUI.paint(); });

  let sel = 0, entries = [];
  const bd = h('div', { class: 'backdrop modal-bg', onpointerdown: closeLauncher });
  const input = h('input', { type: 'text', spellcheck: 'false', placeholder: 'アプリ・タブ・ブックマーク・URL・検索' });
  const list = h('div', { class: 'lres' });
  const box = h('div', { class: 'launcher' }, input, list);
  const run = (i) => { const e = entries[i]; if (!e) return; closeLauncher(); e.run(); };
  const paint = () => {
    entries = launcherEntries(input.value);
    sel = Math.min(sel, Math.max(0, entries.length - 1));
    list.replaceChildren(...(entries.length ? entries.map((e, i) => h('div', {
      class: 'lrow' + (i === sel ? ' sel' : ''),
      onpointermove: () => { if (sel !== i) { sel = i; list.querySelectorAll('.lrow').forEach((r, j) => r.classList.toggle('sel', j === i)); } },
      onclick: () => run(i),
    }, h('span', { class: 'lname' }, e.name), h('span', { class: 'lkind' }, e.kind)))
      : [h('div', { class: 'lnone' }, '入力して検索')]));
    list.querySelector('.sel')?.scrollIntoView({ block: 'nearest' });
  };
  input.addEventListener('input', () => { sel = 0; paint(); });
  input.addEventListener('keydown', (e) => {
    if (e.isComposing) return; // 日本語入力の確定中は触らない
    if (e.key === 'Escape') closeLauncher();
    else if (e.key === 'Enter') run(sel);
    else if (e.key === 'ArrowDown') { e.preventDefault(); sel = Math.min(entries.length - 1, sel + 1); paint(); }
    else if (e.key === 'ArrowUp') { e.preventDefault(); sel = Math.max(0, sel - 1); paint(); }
  });
  overlay.append(bd, box);
  launcherUI = { bd, box, paint };
  suspend(+1);
  paint();
  input.focus();
}
