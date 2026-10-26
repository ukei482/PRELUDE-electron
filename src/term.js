'use strict';
// ホーム。起動直後と、タブの + の「Home」で出る、上のバーの下いっぱいの白い端末。タブより上の階層で、出している間はタブを隠す。
// 端末の中身は bash(main の sys/term.js が疑似端末で動かす)で、最初は PRELUDE コマンド(cli/。命令は src/cli.js)が動いている。
// タブへ戻るのは左上の PRELUDE ボタン(または PRELUDE コマンドの back・タブを開く命令)。
// app.js より先に読み込まれる。$ / api / C / render / syncViews / openTab などは呼び出し時に app.js のものを使う。
// 表示は xterm.js(node_modules/@xterm。index.html で読み込む)。描くのは WebGL のアドオン。

const homeTerm = { on: false, term: null, fit: null, ready: null };

const termColors = () => {
  const css = getComputedStyle(document.documentElement);
  const fg = css.getPropertyValue('--bg').trim() || '#1926a3';
  const bg = css.getPropertyValue('--accent').trim() || '#ffffff';
  // 白地で読めない白・黄は濃くする
  return { background: bg, foreground: fg, cursor: fg, cursorAccent: bg, selectionBackground: fg, selectionForeground: bg, white: '#8a8a8a', brightWhite: '#6a6a6a', yellow: '#9a7a00', brightYellow: '#8a6d00' };
};

// 端末は最初にホームを出したときに作る(窓ごとに1つ)
const initHomeTerm = () => (homeTerm.ready ||= openHomeTerm());
async function openHomeTerm() {
  const el = $('#home-term');
  const t = new Terminal({
    fontFamily: '"VCR OSD Mono", "DotGothic16", monospace', fontSize: Math.round((C.appearance.fontSize || 14) * 1.2),
    lineHeight: 1.2, cursorBlink: true, scrollback: 5000, theme: termColors(), allowProposedApi: false,
  });
  const fit = new FitAddon.FitAddon();
  t.loadAddon(fit);
  Object.assign(homeTerm, { term: t, fit });
  // 書体を読み込んでから開く(後から読み込まれると文字の幅を測り間違える)
  await Promise.all(['VCR OSD Mono', 'DotGothic16'].map((f) => document.fonts.load(`16px "${f}"`).catch(() => {})));
  t.open(el);
  // WebGL で描く(ブロックの文字 ▄▀█ を行間なしにマスいっぱいに描くので、ロゴの AA がつながる)。使えなければ普通の描き方のまま
  try {
    const gl = new WebglAddon.WebglAddon();
    gl.onContextLoss(() => gl.dispose());
    t.loadAddon(gl);
  } catch {}
  fit.fit();
  // Ctrl+Shift+C で選んだ文字をコピーする。Ctrl+Shift+V は貼り付け(ブラウザの貼り付けを xterm が受ける)
  t.attachCustomKeyEventHandler((e) => {
    if (e.type !== 'keydown' || !e.ctrlKey || !e.shiftKey) return true;
    if (e.code === 'KeyC') { if (t.hasSelection()) api.fs.copyText(t.getSelection()); return false; }
    if (e.code === 'KeyV') return false;
    return true;
  });
  // シェルを exit したら、新しいシェル(また PRELUDE コマンドから)を始める。起動してすぐ終わるのが続くときは止めて、Enter を待つ
  let startedAt = 0, quick = 0, stuck = false;
  const start = () => { startedAt = Date.now(); api.term.start(t.cols, t.rows); };
  t.onData((d) => {
    if (!stuck) api.term.write(d);
    else if (d === '\r') { stuck = false; quick = 0; t.reset(); start(); }
  });
  t.onResize(({ cols, rows }) => api.term.resize(cols, rows));
  api.term.onData((d) => t.write(d));
  api.term.onExit(() => {
    quick = Date.now() - startedAt < 2000 ? quick + 1 : 0;
    if (quick >= 3) { t.write('\r\n[the shell keeps exiting right after start; press Enter to retry]\r\n'); stuck = true; return; }
    t.reset();
    start();
  });
  new ResizeObserver(() => { if (homeTerm.on) fit.fit(); }).observe(el);
  api.config.onChanged(() => { t.options.theme = termColors(); });
  start();
}

function showHome() {
  closePopup(); closeLauncher(); closeQuick(); closeNotifications();
  homeTerm.on = true;
  document.body.classList.add('at-home');
  api.term.home(true);
  syncViews(); // Web・取り込んだアプリの窓はタブの上に重なっているので隠す
  topMenuTip();
  initHomeTerm().then(() => { homeTerm.fit.fit(); homeTerm.term.focus(); });
}

// タブへ戻る。タブが1つも無ければ新しいタブを開く
function hideHome() {
  if (!homeTerm.on) return;
  leaveHome();
  if (!S.tabs.length) openTab(C.behavior.startKind);
  else render();
}

// 状態だけ戻す(この後で描き直す openTab / showTab から呼ぶ)
function leaveHome() {
  if (!homeTerm.on) return;
  homeTerm.on = false;
  document.body.classList.remove('at-home');
  api.term.home(false);
  topMenuTip();
}

// 左上の PRELUDE ボタン: ホームではタブへ戻る。タブではサイドバーの表示を切り替える
const topMenuClick = () => (homeTerm.on ? hideHome() : cycleSidebar());
const topMenuTip = () => { const b = $('#top-menu'); if (b) b.title = homeTerm.on ? 'Back to the tabs' : sidebarTip(); };
