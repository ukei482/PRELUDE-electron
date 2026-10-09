'use strict';
// セッションの切り替え。PRELUDE が systemd の prelude-shell.service として plasmashell の代わりに動いているときだけ、
// 「Kubuntuに戻る」ことができる。実際の手順は scripts/prelude-return.sh(段階を踏み、失敗したら PRELUDE のまま)。
const fs = require('fs');
const os = require('os');
const path = require('path');
const { exec } = require('./util');

const UNIT = 'prelude-shell.service';
const ROOT = process.env.PRELUDE_ROOT || path.join(process.env.XDG_DATA_HOME || path.join(os.homedir(), '.local/share'), 'prelude');

// このプロセスが prelude-shell.service の本体か(開発版を手で起動しているだけなら false)
async function isShell() {
  if (process.platform !== 'linux' || !process.env.INVOCATION_ID) return false;
  const r = await exec('systemctl', ['--user', 'show', UNIT, '-p', 'InvocationID', '-p', 'ActiveState']);
  const inv = /^InvocationID=(\w+)/m.exec(r.out)?.[1];
  return r.ok && /^ActiveState=active/m.test(r.out) && inv === process.env.INVOCATION_ID;
}

// 配置済みの単独スクリプト($ROOT/bin。版に依存しない)を優先し、無ければこの作業フォルダのもの
function returnScript() {
  for (const p of [path.join(ROOT, 'bin/prelude-return.sh'), path.join(__dirname, '../scripts/prelude-return.sh')]) {
    try { fs.accessSync(p, fs.constants.X_OK); return p; } catch {}
  }
  return null;
}

async function returnToKde() {
  if (!(await isShell())) return { error: 'PRELUDE がデスクトップシェルとして動いていません(shell-mode.sh on --now で切り替えた時だけ使えます)' };
  const script = returnScript();
  if (!script) return { error: '戻るためのスクリプトが見つかりません(scripts/release.sh install-bin)' };
  // スクリプトは systemd-run で別ユニットに移ってから、PRELUDE(自分)を止める。ここでは起動の成否だけを見る
  const pre = await exec(script, ['--check'], 10000);
  if (!pre.ok) return { error: 'Kubuntuに戻れる状態ではありません: ' + (pre.out + pre.err).trim().split('\n').pop() };
  const r = await exec(script, [], 10000);
  return r.ok ? {} : { error: '切り替えを始められませんでした' };
}

function init({ ipcMain }) {
  ipcMain.handle('session:state', async () => ({ shell: await isShell() }));
  ipcMain.handle('session:return', () => returnToKde());
}

module.exports = { init, isShell, returnToKde };
