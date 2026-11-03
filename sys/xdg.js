'use strict';
// どこにアプリ・アイコン・コマンドがあるかを、決め打ちのパス(/usr/bin・/usr/share・/snap/bin)ではなく
// 環境(PATH・XDG_DATA_DIRS)から探す。Kubuntu でも NixOS(/run/current-system/sw・~/.nix-profile)でも同じに動く。
// 決まりは XDG Base Directory。Rust 版は crates/prelude-core/src/xdg.rs(同じ読み方)。
const fs = require('fs');
const os = require('os');
const path = require('path');

const uniq = (a) => [...new Set(a.filter(Boolean))];
const list = (s) => String(s || '').split(path.delimiter).filter(Boolean);

// $XDG_DATA_HOME(無ければ ~/.local/share)
function dataHome(env = process.env, home = os.homedir()) {
  return env.XDG_DATA_HOME || path.join(home, '.local/share');
}

// $XDG_DATA_DIRS(無ければ決まりどおり /usr/local/share:/usr/share)。優先度の高いものが先
function dataDirs(env = process.env) {
  const d = list(env.XDG_DATA_DIRS);
  return d.length ? uniq(d) : ['/usr/local/share', '/usr/share'];
}

// names のうち、PATH(と extra のフォルダ)で最初に見つかったコマンドの絶対パス。無ければ null。
// names は好きな順。1 つ目の名前を PATH 全体で探してから、次の名前へ進む
function which(names, { env = process.env, extra = [] } = {}) {
  const dirs = uniq([...list(env.PATH), ...extra]);
  for (const n of [].concat(names)) {
    if (path.isAbsolute(n)) { if (isExec(n)) return n; continue; }
    for (const d of dirs) {
      const f = path.join(d, n);
      if (isExec(f)) return f;
    }
  }
  return null;
}

function isExec(f) {
  try { return fs.statSync(f).isFile() && (fs.accessSync(f, fs.constants.X_OK), true); } catch { return false; }
}

module.exports = { dataHome, dataDirs, which };
