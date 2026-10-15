'use strict';
// .desktop ファイルからインストール済みアプリの一覧を作る
const fs = require('fs');
const os = require('os');
const path = require('path');
const { execFile } = require('child_process');

function dirs() {
  const data = (process.env.XDG_DATA_DIRS || '/usr/local/share:/usr/share').split(':').filter(Boolean);
  const home = process.env.XDG_DATA_HOME || path.join(os.homedir(), '.local/share');
  return [...new Set([
    ...data, home,
    '/var/lib/flatpak/exports/share',
    path.join(os.homedir(), '.local/share/flatpak/exports/share'),
  ])].map((d) => path.join(d, 'applications'));
}

// 端末アプリを包むコマンド。見つからなければ x-terminal-emulator に任せる
function terminalCmd(exec) {
  const term = ['/usr/bin/konsole', '/usr/bin/x-terminal-emulator'].find((p) => fs.existsSync(p)) || 'x-terminal-emulator';
  return `${term} -e ${exec}`;
}

function list() {
  const map = new Map();
  for (const dir of dirs()) {
    let files = [];
    try { files = fs.readdirSync(dir).filter((f) => f.endsWith('.desktop')); } catch { continue; }
    for (const f of files) {
      try {
        const sec = fs.readFileSync(path.join(dir, f), 'utf8').split(/\n\[/)[0];
        const get = (k) => new RegExp(`^${k}=(.*)$`, 'm').exec(sec)?.[1]?.trim();
        // 後の(優先度の高い)フォルダで隠されたものは、前のフォルダの同名ファイルも消す
        if (get('Type') !== 'Application' || get('NoDisplay') === 'true' || get('Hidden') === 'true') { map.delete(f); continue; }
        // フィールドコード(%f %u %i など)は引数なしで起動するので取り除く。--icon %i は --icon ごと
        let exec = (get('Exec') || '').replace(/--icon\s+%i/g, '').replace(/%[a-zA-Z]/g, '').replace(/\s+/g, ' ').trim();
        if (!exec || /^(\/usr\/bin\/)?(false|true)$/.test(exec)) continue; // 何もしない項目(snap の補助エントリなど)
        // 端末で動くアプリ(htop など)は、そのままではウィンドウが出ないので端末の中で起動する
        if (get('Terminal') === 'true') exec = terminalCmd(exec);
        // id: .desktop のファイル名。Wayland のウィンドウの app_id(KWin の desktopFileName)と一致することが多い
        map.set(f, { name: get('Name') || f, exec, cls: (get('StartupWMClass') || '').toLowerCase(), icon: get('Icon') || '', id: f.slice(0, -8) });
      } catch {}
    }
  }
  return [...map.values()].sort((a, b) => a.name.localeCompare(b.name));
}

// file を開く既定のアプリ(xdg-mime で調べる)。{ cmd, name, cls, icon, id } か null。
// 既定のアプリは一覧に出ない .desktop(NoDisplay。okularApplication_md など)のこともあるので、ファイルを直接読む。
// Exec のフィールドコード(%f %u など)はファイルのパスに置き換え、無ければ末尾に付ける
const xdgMime = (args) => new Promise((resolve) => {
  execFile('xdg-mime', args, { timeout: 5000 }, (err, out) => resolve(err ? '' : String(out).trim().split('\n')[0] || ''));
});
async function forFile(file) {
  const mime = await xdgMime(['query', 'filetype', file]);
  const id = mime && (await xdgMime(['query', 'default', mime]));
  if (!id || !id.endsWith('.desktop')) return null;
  const p = dirs().reverse().map((d) => path.join(d, id)).find((f) => fs.existsSync(f)); // 後のフォルダ(ホーム)ほど優先
  if (!p) return null;
  try {
    const sec = fs.readFileSync(p, 'utf8').split(/\n\[/)[0];
    const get = (k) => new RegExp(`^${k}=(.*)$`, 'm').exec(sec)?.[1]?.trim();
    const q = `'${file.replace(/'/g, `'\\''`)}'`;
    let used = false;
    let exec = (get('Exec') || '').replace(/--icon\s+%i/g, '')
      .replace(/%[fFuU]/g, () => { if (used) return ''; used = true; return q; })
      .replace(/%[a-zA-Z]/g, '').replace(/\s+/g, ' ').trim();
    if (!exec) return null;
    if (!used) exec += ' ' + q;
    if (get('Terminal') === 'true') exec = terminalCmd(exec);
    return { cmd: exec, name: get('Name') || id, cls: (get('StartupWMClass') || '').toLowerCase(), icon: get('Icon') || '', id: id.slice(0, -8), mime };
  } catch { return null; }
}

module.exports = { list, forFile };
