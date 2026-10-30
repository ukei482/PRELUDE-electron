// KWin スクリプト: 取り込むアプリの窓を、ペインの場所に重ねて置く(枠なし・最前面)。prelude-core の embed.rs が作る。
// Wayland では他のアプリの窓を子にできないので、ペインの場所に窓を重ねる(Electron 版の appembed.js と同じ考え方)。
// 状態(ペインの場所・PID・窓の id)は下の STATE に埋め込み、変わるたびにスクリプトごと読み直す。
// 違い: 場所は UI が知らせた画面の座標そのまま(PRELUDE の窓からの相対ではない)。PRELUDE の窓の扱い(疑似全画面など)は UI の側
var STATE = /*STATE*/;

// 報告先は prelude-core の D-Bus サービス(embed.rs)
function send(kind, data) { callDBus(STATE.service, '/org/prelude/Core', 'org.prelude.Core', 'Report', kind, JSON.stringify(data)); }
function wid(w) { return String(w.internalId); }
function keyHit(p, w) {
  var c = (w.resourceClass || '').toLowerCase(), n = (w.resourceName || '').toLowerCase(), d = (w.desktopFileName || '').toLowerCase();
  var ds = d.split('.').pop();
  for (var i = 0; i < p.keys.length; i++) {
    var k = p.keys[i];
    if (k && (k === c || k === n || k === d || k === ds)) return true;
  }
  return false;
}
// 窓をペインのものにする。以後は窓の id で照合し続ける(コアにも知らせて、次のスクリプトに引き継ぐ)
function claim(p, w, how) {
  p.wids.push(wid(w));
  send('claim', { pane: p.id, wid: wid(w), pid: w.pid, how: how });
  return p;
}
// PRELUDE の UI・オーバーレイの窓は取り込まない
function prelude(w) { return /^PRELUDE(-overlay:|:)/.test(w.caption || '') || STATE.self.indexOf(w.pid) >= 0; }
// 窓の属するペイン。順に: 取り込み済みの窓の id → 起動したプロセスの PID → (まだ窓が無いペインだけ)起動後に現れた同じアプリの窓
function paneOf(w, activated) {
  if (!w.normalWindow || prelude(w)) return null;
  var id = wid(w), i, p;
  for (i = 0; i < STATE.panes.length; i++) if (STATE.panes[i].wids.indexOf(id) >= 0) return STATE.panes[i];
  // Web のペイン: 題名が印の窓(web.rs が作ったアプリの窓。最初のページが題名を印にする)
  for (i = 0; i < STATE.panes.length; i++) if (STATE.panes[i].marker && !STATE.panes[i].wids.length && w.caption === STATE.panes[i].marker) return claim(STATE.panes[i], w, 'marker');
  for (i = 0; i < STATE.panes.length; i++) if (STATE.panes[i].pids.indexOf(w.pid) >= 0) return claim(STATE.panes[i], w, 'pid');
  for (i = 0; i < STATE.panes.length; i++) {
    p = STATE.panes[i];
    // 既に開いている窓を前に出すだけのアプリ(システム設定など)は、その窓を取り込む
    if (!p.wids.length && p.old && (activated || p.adopt || p.old.indexOf(id) < 0) && keyHit(p, w)) return claim(p, w, p.old.indexOf(id) >= 0 ? 'adopt' : 'key');
  }
  return null;
}
var gone = {};
// ペインの本体の窓: 取り込んだ順で最初の、今もある普通の窓(ダイアログは除く)
function mainOf(p) {
  var byId = {};
  workspace.windowList().forEach(function (w) { if (!w.transient && !gone[wid(w)]) byId[wid(w)] = w; });
  for (var i = 0; i < p.wids.length; i++) if (byId[p.wids[i]]) return byId[p.wids[i]];
  return null;
}
function apply(w) {
  var p = paneOf(w);
  if (!p) return;
  hook(w);
  if (!p.rect) { w.minimized = true; return; } // 見えていないペイン(ほかのタブ・UI がまだ場所を知らせていない)
  w.minimized = false;
  w.keepAbove = true;
  w.skipTaskbar = true; w.skipPager = true; w.skipSwitcher = true;
  var geo = { x: p.rect.x, y: p.rect.y, width: p.rect.width, height: p.rect.height };
  if (w.transient) { placeChild(w, geo); return; }
  if (w !== mainOf(p)) { placeExtra(w, geo); return; }
  w.noBorder = true;
  w.frameGeometry = geo;
  if (p.activate) workspace.activeWindow = w; // 最小化から戻した窓は、アクティブにしないと描かれないことがある
}
// 本体のほかに開いた窓。枠付きで、ペインより少し小さくして中央に浮かべる。ペインの中で動かした窓はそのまま
function placeExtra(w, pane) {
  var f = w.frameGeometry;
  var inside = f.x >= pane.x && f.y >= pane.y && f.x + f.width <= pane.x + pane.width && f.y + f.height <= pane.y + pane.height;
  if (!w.noBorder && inside) return;
  w.noBorder = false;
  var width = Math.round(Math.min(f.width, pane.width * 0.85)), height = Math.round(Math.min(f.height, pane.height * 0.85));
  w.frameGeometry = { x: Math.round(pane.x + (pane.width - width) / 2), y: Math.round(pane.y + (pane.height - height) / 2), width: width, height: height };
}
// 子の窓(ダイアログ)。枠を付けたまま自分の大きさで、ペインからはみ出すときだけ中央に置き直す
function placeChild(w, pane) {
  w.noBorder = false;
  var f = w.frameGeometry;
  var width = Math.min(f.width, pane.width), height = Math.min(f.height, pane.height);
  var inside = f.x >= pane.x && f.y >= pane.y && f.x + f.width <= pane.x + pane.width && f.y + f.height <= pane.y + pane.height;
  if (inside && width === f.width && height === f.height) return;
  w.frameGeometry = { x: Math.round(pane.x + (pane.width - width) / 2), y: Math.round(pane.y + (pane.height - height) / 2), width: width, height: height };
}
// ペインごとの窓の数と題名(本体の題名)。全部の窓の id は、次に起動するアプリの「起動前からあった窓」に使う
function report() {
  var counts = {}, titles = {}, all = [];
  STATE.panes.forEach(function (p) { counts[p.id] = 0; });
  workspace.windowList().forEach(function (w) {
    all.push(wid(w));
    var p = paneOf(w);
    if (!p) return;
    counts[p.id]++;
    if (w.caption && w === mainOf(p)) titles[p.id] = w.caption;
  });
  send('windows', { counts: counts, titles: titles, all: all });
}
// 題名が変わったら: 取り込んだ窓なら知らせ直す。まだ取り込んでいない窓は、印の題名になったかもしれないので確かめる
var hooked = {};
function hook(w) {
  var id = wid(w);
  if (hooked[id] || !w.captionChanged) return;
  hooked[id] = true;
  w.captionChanged.connect(function () { if (paneOf(w)) { apply(w); report(); } });
}
workspace.windowAdded.connect(function (w) {
  hook(w);
  w.windowShown && w.windowShown.connect(function () { apply(w); });
  apply(w);
  report();
});
workspace.windowRemoved.connect(function (w) {
  gone[wid(w)] = true;
  var p = paneOf(w);
  if (p && !w.transient) workspace.windowList().forEach(apply); // 本体が閉じたら、次の窓を本体にする
  report();
});
workspace.windowActivated.connect(function (w) {
  if (w && paneOf(w, true)) { apply(w); report(); }
});
// 閉じたペインの窓を閉じる・借りていた窓を普通の窓に戻す
workspace.windowList().forEach(function (w) {
  var id = wid(w);
  if (STATE.close.indexOf(id) >= 0 || STATE.closePids.indexOf(w.pid) >= 0) w.closeWindow();
  else if (STATE.release.indexOf(id) >= 0) {
    w.keepAbove = false; w.noBorder = false; w.skipTaskbar = false; w.skipPager = false; w.skipSwitcher = false; w.minimized = false;
  }
});
workspace.windowList().forEach(function (w) { hook(w); apply(w); });
report();
