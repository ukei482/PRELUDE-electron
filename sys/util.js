'use strict';
const { execFile } = require('child_process');

// 外部コマンドを(シェルを通さず)実行する。失敗しても例外は出さず { ok:false } を返す
const exec = (cmd, args = [], timeout = 4000) => new Promise((resolve) => {
  execFile(cmd, args, { timeout, env: { ...process.env, LC_ALL: 'C' } }, (err, out, errOut) => {
    resolve({ ok: !err, out: String(out || ''), err: String(errOut || err?.message || '') });
  });
});

// この PRELUDE を見分ける印。普段のプロファイルなら ''、--user-data-dir で別のプロファイル(prelude.sh debug など)なら、その場所から作った短い印。
// KWin スクリプトの名前や一時ファイルに付けて、シェルとして動いている PRELUDE と取り合わないようにする
const instanceTag = (() => {
  const arg = process.argv.find((a) => a.startsWith('--user-data-dir='));
  return arg ? '-' + require('crypto').createHash('sha1').update(arg).digest('hex').slice(0, 8) : '';
})();

// 起動する子プロセスに、Electron が開いたままのファイルやソケットを渡さない。
// Electron(Chromium)には CLOEXEC を付けずに開くものがあり、Node の spawn はそれを閉じないので、起動したアプリや pactl が
// キャッシュのファイル・デバッグ用のポートなどを持ち続けてしまう(終了後もポートが塞がったままになった)。
// bash で 0〜2 以外を閉じてから実行する(/bin/sh(dash)は2桁のファイル番号を扱えない)
const CLOSE_FDS = 'for f in /proc/$$/fd/*; do n=${f##*/}; case $n in 0|1|2) ;; *) eval "exec $n>&-" 2>/dev/null ;; esac; done';
// シェルのコマンド cmd を、余分なファイルを閉じてから /bin/sh で実行する spawn の引数 [実行ファイル, 引数]
const cleanShell = (cmd) => ['/bin/bash', ['-c', `${CLOSE_FDS}; exec /bin/sh -c "$0"`, cmd]];

module.exports = { exec, instanceTag, CLOSE_FDS, cleanShell };
