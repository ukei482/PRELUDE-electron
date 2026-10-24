'use strict';
// PRELUDE 自身の更新。GitHub の一番新しい版(タグ)を確かめ、scripts/release.sh update で作って切り替え、シェルを再起動する。
// 更新できるのは prelude-shell.service として動いているときだけ(開発版を手で起動しているときは確認のみ)。
// 新しい版が起動直後に落ち続けたときは、prelude-fallback.sh が previous(更新前の版)に戻す。
const path = require('path');
const { app } = require('electron');
const { exec } = require('./util');
const { isShell } = require('./session');

const UNIT = 'prelude-shell.service';
const SCRIPT = path.join(__dirname, '../scripts/release.sh');

const lastLine = (s) => s.trim().split('\n').pop() || '';
const parse = (v) => String(v).replace(/^v/, '').split('.').map((n) => parseInt(n, 10) || 0);
function newer(a, b) { // a が b より新しいか
  const x = parse(a), y = parse(b);
  for (let i = 0; i < Math.max(x.length, y.length); i++) if ((x[i] || 0) !== (y[i] || 0)) return (x[i] || 0) > (y[i] || 0);
  return false;
}

async function check() {
  const current = 'v' + app.getVersion();
  const shell = await isShell();
  const r = await exec(SCRIPT, ['latest'], 30000);
  if (!r.ok) return { current, shell, error: lastLine(r.err) || 'Could not check for a new version' };
  const latest = lastLine(r.out);
  return { current, shell, latest, available: newer(latest, current) };
}

let busy = false;
async function update() {
  if (busy) return { error: 'Already updating' };
  if (!(await isShell())) return { error: 'Can only update while PRELUDE runs as the desktop shell' };
  busy = true;
  try {
    // 新しい版の作成(npm ci と Electron の取得)に時間がかかることがある
    const r = await exec(SCRIPT, ['update'], 15 * 60 * 1000);
    if (!r.ok) return { error: 'Update failed: ' + (lastLine(r.err) || lastLine(r.out)) };
    const [state, tag] = lastLine(r.out).split(' ');
    if (state !== 'updated') return { latest: tag };
    // --no-block: 再起動の受け付けだけで戻る(この PRELUDE 自身が止められるので、完了は待てない)
    const s = await exec('systemctl', ['--user', '--no-block', 'restart', UNIT]);
    if (!s.ok) return { error: `Switched to ${tag} but could not restart (systemctl --user restart prelude-shell)` };
    return { updated: tag };
  } finally {
    busy = false;
  }
}

function init({ ipcMain }) {
  ipcMain.handle('update:check', () => check());
  ipcMain.handle('update:run', () => update());
}

module.exports = { init };
