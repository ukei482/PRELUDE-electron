'use strict';
const { execFile } = require('child_process');

// 外部コマンドを(シェルを通さず)実行する。失敗しても例外は出さず { ok:false } を返す
const exec = (cmd, args = [], timeout = 4000) => new Promise((resolve) => {
  execFile(cmd, args, { timeout, env: { ...process.env, LC_ALL: 'C' } }, (err, out, errOut) => {
    resolve({ ok: !err, out: String(out || ''), err: String(errOut || err?.message || '') });
  });
});

module.exports = { exec };
