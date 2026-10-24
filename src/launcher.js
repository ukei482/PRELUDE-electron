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

const NO_EMBED = /(^|\/)spectacle(\s|$)/;

// アプリを開く。既定はペインに取り込む(設定で別ウィンドウにもできる)。取り込み未対応の環境では別ウィンドウ
function launchApp(a) {
  const usage = (C.appUsage || []).filter((u) => u.exec !== a.exec);
  usage.unshift({ exec: a.exec, n: ((C.appUsage || []).find((u) => u.exec === a.exec)?.n || 0) + 1 });
  api.config.set('appUsage', usage.slice(0, 50));
  // スクリーンショットのアプリはペインに取り込まない(範囲選びの画面や「画面全体」が取り込みと噛み合わない)
  if (!S.appSupported || C.behavior.appOpen === 'window' || NO_EMBED.test(a.exec)) {
    api.app.spawn(a.exec).then((ok) => { if (!ok) toast('起動できませんでした'); });
    return;
  }
  const t = openTab('app', { cmd: a.exec, cls: a.cls, name: a.name, icon: a.icon });
  startApp(t.root, a.exec, a.cls, a.name, a.icon);
}

// ------------------------------------------------------------------ 開発版
// 作業フォルダの PRELUDE(開発版)を、新しいタブのアプリのペインに取り込んで動かす。起動コマンドは main.js の dev:info。
// 開発版のペインは起動コマンドの印 PRELUDE_EMBEDDED=1 で見分ける(タブを復元した後も同じ)
const DEV_NAME = 'PRELUDE 開発版';
const isDevLeaf = (l) => l?.type === 'app' && /\bPRELUDE_EMBEDDED=1\b/.test(l.cmd || '');
function findDevLeaf() {
  for (const tab of S.tabs) {
    const leaf = leaves(tab.root).find(isDevLeaf);
    if (leaf) return { tab, leaf };
  }
  return null;
}

// 開発版のタブを開く。もうあればそのタブへ
async function openDevPrelude() {
  const f = findDevLeaf();
  if (f) { f.tab.activePane = f.leaf.id; showTab(f.tab); return; }
  if (!S.appSupported) { toast('開発版の取り込みは KDE Plasma(Wayland)だけで使えます'); return; }
  const d = await api.dev.info();
  if (d.err) { toast(d.err); return; }
  if (d.pid) { toast(`開発版は別に動いています(pid ${d.pid})。終了してから開いてください`); return; }
  const t = openTab('app', { cmd: d.cmd, name: DEV_NAME });
  startApp(t.root, d.cmd, '', DEV_NAME);
}

// 開発版を起動し直す(main.js などメインプロセスの変更は、レンダラの再読込では届かない)。
// 同じプロファイルは同時に1つしか動かないので、前の開発版が終わるのを待ってから起動する
async function restartDevPrelude(leaf = findDevLeaf()?.leaf) {
  if (!leaf || leaf.restarting) return;
  const d = await api.dev.info();
  if (d.err) { toast(d.err); return; }
  leaf.restarting = true;
  if (leaf.running) api.app.close(leaf.id);
  Object.assign(leaf, { running: false, dormant: false, title: '' });
  render();
  try {
    for (let i = 0; i < 60 && (await api.dev.info()).pid; i++) await new Promise((r) => setTimeout(r, 250));
    const pid = (await api.dev.info()).pid;
    if (pid) { toast(`前の開発版が終わりません(pid ${pid})`); return; }
    await startApp(leaf, d.cmd, '', DEV_NAME);
  } finally {
    leaf.restarting = false;
    render();
  }
}

const LAUNCHER_COMMANDS = [
  { name: '設定', keys: 'settings せってい', when: () => !IS_SUB, run: () => openSettings() },
  { name: 'ホーム画面', keys: 'home ほーむ', run: () => openTab('home') },
  { name: '新しいタブ(Web)', keys: 'new tab web あたらしい', run: () => openTab('web') },
  { name: '新しいタブ(フォルダ)', keys: 'new tab folder files あたらしい', run: () => openTab('files') },
  { name: '新しいタブ(フィールド)', keys: 'new tab field tree folder files ふぃーるど あたらしい', run: () => openTab('field') },
  { name: '全画面の切り替え', keys: 'fullscreen ぜんがめん', when: () => !IS_SUB, run: () => api.win.cmd('toggleFullscreen') },
  { name: '最大化の切り替え', keys: 'maximize window さいだいか', when: () => !IS_SUB, run: () => api.win.cmd('toggleMaximize') },
  { name: 'サイドバーの切り替え', keys: 'sidebar tabs さいどばー たぶ', run: () => cycleSidebar() },
  { name: 'スクリーンショット(範囲を選ぶ)', keys: 'screenshot capture print すくりーんしょっと さつえい はんい', run: () => api.quick.do('screenshot', 'region') },
  { name: 'スクリーンショット(今の画面)', keys: 'screenshot capture print screen full すくりーんしょっと がめん ぜんたい', run: () => api.quick.do('screenshot', 'screen') },
  { name: 'スクリーンショット(前面の窓)', keys: 'screenshot capture print window すくりーんしょっと まど', run: () => api.quick.do('screenshot', 'window') },
  { name: 'PRELUDE 開発版を開く', keys: 'prelude dev debug test develop かいはつばん でばっぐ てすと', when: () => !S.devSelf, run: () => openDevPrelude() },
  { name: 'PRELUDE 開発版を再起動', keys: 'prelude dev debug test restart reload かいはつばん でばっぐ てすと さいきどう', when: () => !!findDevLeaf(), run: () => restartDevPrelude() },
  { name: 'PRELUDE を更新', keys: 'update upgrade prelude こうしん あっぷでーと', when: () => sessionShell && !IS_SUB, run: () => runUpdate() },
  { name: 'Kubuntuに戻る', keys: 'kubuntu plasma desktop return もどる', when: () => sessionShell && !IS_SUB, run: () => returnToKde() },
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

  // 数字だけならその番号のタブ(別の画面の窓のタブも)、b+数字ならその番号のブックマークを一番上に出す
  const num = /^(b?)(\d+)$/.exec(q);
  if (num && !num[1]) {
    const n = Number(num[2]);
    const tab = S.tabs.find((t) => t.num === n);
    const where = tab ? null : api.tab.where(n);
    if (tab) out.push({ kind: `タブ ${n}`, name: tabTitle(tab), run: () => showTab(tab) });
    else if (where) out.push({ kind: `タブ ${n}`, name: `${where.label}のタブ`, run: () => api.tab.goto(String(n)) });
  } else if (num) {
    const b = S.bookmarks.find((x) => x.num === Number(num[2]) && x.kind !== 'folder');
    if (b) out.push({ kind: `ブックマーク b${b.num}`, name: b.title || b.target, run: () => openBookmark(b) });
  }
  for (const { x: tab } of pick(S.tabs, tabTitle)) out.push({ kind: `タブ ${tab.num}`, name: tabTitle(tab), run: () => showTab(tab) });
  for (const { x: b } of pick(S.bookmarks.filter((b) => b.kind !== 'folder'), (b) => b.title || b.target)) out.push({ kind: `ブックマーク b${b.num}`, name: b.title || b.target, run: () => openBookmark(b) });
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
