'use strict';
// .desktop ファイルからインストール済みアプリの一覧を作る
const fs = require('fs');
const os = require('os');
const path = require('path');

function dirs() {
  const data = (process.env.XDG_DATA_DIRS || '/usr/local/share:/usr/share').split(':').filter(Boolean);
  const home = process.env.XDG_DATA_HOME || path.join(os.homedir(), '.local/share');
  return [...new Set([
    ...data, home,
    '/var/lib/flatpak/exports/share',
    path.join(os.homedir(), '.local/share/flatpak/exports/share'),
  ])].map((d) => path.join(d, 'applications'));
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
        if (get('Type') !== 'Application' || get('NoDisplay') === 'true' || get('Hidden') === 'true') continue;
        const exec = (get('Exec') || '').replace(/%[a-zA-Z]/g, '').replace(/\s+/g, ' ').trim();
        if (!exec) continue;
        map.set(f, { name: get('Name') || f, exec, cls: (get('StartupWMClass') || '').toLowerCase(), icon: get('Icon') || '' });
      } catch {}
    }
  }
  return [...map.values()].sort((a, b) => a.name.localeCompare(b.name));
}

module.exports = { list };
