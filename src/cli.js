'use strict';
// PRELUDE コマンドの命令。ホームの端末で PRELUDE と打った後の行(cli/prelude.js → sys/term.js のソケット → ここ)を解釈して実行し、
// 端末に出す文を返す。タブ・ブックマーク・アプリを開く命令は、開いたらホームを閉じてタブへ移る。
// exit と clear は cli/prelude.js 側で処理する(ここには来ない)。
// app.js より先に読み込まれる。S / openTab / showTab / launcherEntries などは呼び出し時に app.js・launcher.js のものを使う。

const CLI_COMMANDS = [
  { name: 'help', args: '', desc: 'Show this list', run: cliHelp },
  { name: 'tabs', args: '', desc: 'List the tabs', run: () => cliTabs() },
  { name: 'tab', args: '<n | bn>', desc: 'Go to tab n, or open bookmark n', run: cliTab },
  { name: 'bookmarks', args: '', desc: 'List the bookmarks', run: () => cliBookmarks() },
  { name: 'apps', args: '[words]', desc: 'List the apps', run: cliApps },
  { name: 'open', args: '<words | url | path>', desc: 'Open the best match: app, tab, bookmark, URL, folder or file', run: cliOpen },
  { name: 'web', args: '[url | words]', desc: 'Open a web tab', run: (a) => cliGo(() => openTab('web', { url: a.join(' ') }), `Web ${a.join(' ')}`.trim()) },
  { name: 'folder', args: '[path]', desc: 'Open a folder tab (default: this folder)', run: (a, cwd) => { const p = cliAbs(a[0], cwd); return cliGo(() => openTab('files', { path: p }), `Folder ${p}`); } },
  { name: 'field', args: '[path]', desc: 'Open a field tab (default: this folder)', run: (a, cwd) => { const p = cliAbs(a[0], cwd); return cliGo(() => openTab('field', { path: p }), `Field ${p}`); } },
  { name: 'launcher', args: '', desc: 'Open the launcher', run: () => { openLauncher(); return ''; } },
  { name: 'settings', args: '', desc: 'Open the settings', when: () => !IS_SUB, run: () => cliGo(openSettings, 'Settings') },
  { name: 'back', args: '', desc: 'Back to the tabs (same as the PRELUDE button)', run: () => { hideHome(); return ''; } },
  { name: 'clear', args: '', desc: 'Clear the screen' },
  { name: 'exit', args: '', desc: 'Leave for the shell (type PRELUDE to come back)' },
];

let cliHome = '';
const cliAbs = (p, cwd) => cliPath(p || '.', cwd, cliHome);
// タブを開く命令: 開いてホームを閉じる(openTab / showTab がホームを閉じる)
const cliGo = (fn, what) => { fn(); return `Opened: ${what}`; };
const cliCommands = () => CLI_COMMANDS.filter((c) => !c.when || c.when());

function cliHelp(args) {
  const list = cliCommands();
  if (args[0] === '--names') return list.map((c) => c.name).join(' ');
  const w = Math.max(...list.map((c) => (c.name + ' ' + c.args).length));
  return list.map((c) => `  ${(c.name + ' ' + c.args).padEnd(w)}  ${c.desc}`).join('\n')
    + '\n\n  A number (3) or b + number (b3) also goes to that tab / bookmark.';
}

function cliTabs() {
  if (!S.tabs.length) return 'No tabs';
  return S.tabs.map((t) => `${String(t.num || '').padStart(3)}  ${tabTitle(t)}${t.id === S.active ? '  *' : ''}`).join('\n');
}

function cliBookmarks() {
  const list = S.bookmarks.filter((b) => b.kind !== 'folder');
  if (!list.length) return 'No bookmarks';
  return list.map((b) => `${('b' + b.num).padStart(4)}  ${b.title || b.target}`).join('\n');
}

async function cliApps(args) {
  if (!S.appList?.length) S.appList = await api.app.list();
  const q = args.join(' ').toLowerCase();
  const list = S.appList.filter((a) => !q || a.name.toLowerCase().includes(q));
  return list.length ? list.map((a) => a.name).join('\n') : 'No apps';
}

function cliTab(args) {
  const m = /^(b?)(\d+)$/i.exec(args[0] || '');
  if (!m) throw new Error('Usage: tab 3  (bookmark: tab b3)');
  const n = Number(m[2]);
  if (m[1]) {
    const b = S.bookmarks.find((x) => x.num === n && x.kind !== 'folder');
    if (!b) throw new Error(`No bookmark b${n}`);
    return cliGo(() => openBookmark(b), `Bookmark b${n}`);
  }
  const tab = S.tabs.find((t) => t.num === n);
  if (tab) return cliGo(() => showTab(tab), `Tab ${n}`);
  if (!api.tab.where(n)) throw new Error(`No tab ${n}`);
  api.tab.goto(String(n)); // 別の画面の窓のタブ
  return `Tab ${n} (on another screen)`;
}

// パスならフォルダ(またはファイル)を、それ以外はランチャーで一番上に出るもの(アプリ・タブ・ブックマーク・Web)を開く
async function cliOpen(args, cwd) {
  const q = args.join(' ').trim();
  if (!q) throw new Error('Usage: open <words | url | path>');
  if (args.length === 1 && cliLooksPath(q)) {
    const full = cliAbs(q, cwd);
    const [st] = await api.fs.stats('/', [full]);
    if (!st) throw new Error(`Not found: ${full}`);
    if (st[0] === -1) return cliGo(() => openTab('files', { path: full }), `Folder ${full}`);
    await openFileAt(null, null, full); // タブに開けばホームを閉じる。別ウィンドウで開いたときはホームのまま
    return `Opened: ${full}`;
  }
  if (!S.appList?.length) S.appList = await api.app.list();
  const e = launcherEntries(q)[0];
  return cliGo(e.run, `${e.kind} ${e.name}`);
}

async function cliRun(line, cwd) {
  const args = cliSplit(line);
  const name = (args.shift() || '').toLowerCase();
  if (/^b?\d+$/.test(name)) return cliTab([name]);
  const c = cliCommands().find((x) => x.name === name);
  if (!c?.run) throw new Error(`Unknown command: ${name}  ("help" for the list)`);
  return (await c.run(args, cwd)) ?? '';
}

function initCli() {
  api.fs.home().then((h) => { cliHome = h; });
  api.term.onCli(async ({ id, line, cwd }) => {
    try { api.term.cliResult({ id, ok: true, out: String(await cliRun(line, cwd)) }); }
    catch (e) { api.term.cliResult({ id, ok: false, out: e.message }); }
  });
}
