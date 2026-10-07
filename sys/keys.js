'use strict';
// グローバルショートカット(どのアプリが前面でも効くキー)。Waylandでは Electron が直接は受けられないので、
// KWinスクリプトに registerShortcut させ、押されたら PRELUDE を前面に出して D-Bus で知らせる。
// KDEの「ショートカット」設定に "PRELUDE ..." として現れ、あとから設定画面で変更できる(最初の登録時の既定キーだけ引数で決まる)。
const fs = require('fs');
const os = require('os');
const path = require('path');
const { exec } = require('./util');

const NAME = 'prelude-keys';
const supported = process.platform === 'linux' && /kde/i.test(process.env.XDG_CURRENT_DESKTOP || '');
let file = null;

// keys: { 名前: { key: 'Meta+Space', raise: true } }。名前は Report('shortcut', JSON文字列) で画面側へ渡る。
// raise が false のキー(音量キーなど)は、PRELUDE を前面に出さず、今のウィンドウのまま通知だけする
function script(keys) {
  return `
var EL_PID = ${process.pid};
function electron() {
  var best = null, area = 0;
  workspace.windowList().forEach(function (w) {
    if (w.pid !== EL_PID || !w.normalWindow) return;
    var a = w.frameGeometry.width * w.frameGeometry.height;
    if (a > area) { best = w; area = a; }
  });
  return best;
}
function fire(name, raise) {
  var el = electron();
  if (raise && el) { el.minimized = false; workspace.activeWindow = el; }
  callDBus('org.prelude.Shell', '/org/prelude/Shell', 'org.prelude.Shell', 'Report', 'shortcut', JSON.stringify(name));
}
var KEYS = ${JSON.stringify(keys)};
Object.keys(KEYS).forEach(function (name) {
  registerShortcut('PRELUDE ' + name, 'PRELUDE: ' + name, KEYS[name].key, function () { fire(name, KEYS[name].raise); });
});
`;
}

const kwin = (obj, method, ...args) => exec('gdbus', ['call', '--session', '--dest', 'org.kde.KWin', '--object-path', obj, '--method', method, ...args]);

let chain = Promise.resolve();
// 連続して呼ばれても、読み込み(unload→load)が重ならないよう順番に処理する
const load = (keys) => (chain = chain.then(() => doLoad(keys)).catch(() => {}));

async function doLoad(keys) {
  if (!supported) return;
  if (!file) file = path.join(fs.mkdtempSync(path.join(os.tmpdir(), 'prelude-keys-')), 'keys.js');
  await kwin('/Scripting', 'org.kde.kwin.Scripting.unloadScript', NAME);
  fs.writeFileSync(file, script(keys));
  const out = await kwin('/Scripting', 'org.kde.kwin.Scripting.loadScript', file, NAME);
  const id = out.ok && /(\d+)/.exec(out.out)?.[1];
  if (id != null) await kwin(`/Scripting/Script${id}`, 'org.kde.kwin.Script.run');
}

const shutdown = () => { if (supported) kwin('/Scripting', 'org.kde.kwin.Scripting.unloadScript', NAME); };

module.exports = { supported, load, shutdown };
