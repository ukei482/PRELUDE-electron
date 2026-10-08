'use strict';
// アイコン名(freedesktop のアイコンテーマ)→ ファイル → data URL。
// 画面側は CSP で file: の画像を読めないので、main で読んで data URL にして渡す。
const fs = require('fs');
const os = require('os');
const path = require('path');

const ROOTS = [path.join(process.env.XDG_DATA_HOME || path.join(os.homedir(), '.local/share'), 'icons'), '/usr/share/icons', '/usr/local/share/icons'];
const THEMES = ['breeze', 'hicolor', 'Adwaita'];
const SIZES = ['scalable', '64', '64x64', '48', '48x48', '32', '32x32', '128x128', '128', '22', '22x22', '256x256', '16', '16x16'];
const CONTEXTS = ['apps', 'status', 'devices', 'actions', 'places', 'mimetypes', 'categories', 'emblems'];
const MAX_BYTES = 300 * 1024;
const cache = new Map(); // 名前 -> data URL | null

const MIME = { '.svg': 'image/svg+xml', '.png': 'image/png', '.jpg': 'image/jpeg', '.jpeg': 'image/jpeg', '.webp': 'image/webp' };

function fileToDataUrl(file) {
  try {
    const mime = MIME[path.extname(file).toLowerCase()];
    if (!mime) return null;
    const st = fs.statSync(file);
    if (!st.isFile() || st.size > MAX_BYTES) return null;
    return `data:${mime};base64,${fs.readFileSync(file).toString('base64')}`;
  } catch { return null; }
}

function findInTheme(name, extraRoots = []) {
  for (const root of [...extraRoots, ...ROOTS]) {
    for (const theme of THEMES) {
      for (const size of SIZES) {
        for (const ctx of CONTEXTS) {
          // テーマごとにフォルダの並びが違う(hicolor: 48x48/apps、breeze: apps/48)
          for (const dir of [path.join(root, theme, size, ctx), path.join(root, theme, ctx, size)]) {
            for (const ext of ['.svg', '.png']) {
              const f = path.join(dir, name + ext);
              if (fs.existsSync(f)) return f;
            }
          }
        }
      }
    }
  }
  for (const dir of [...extraRoots, '/usr/share/pixmaps']) { // アプリ自身が持つアイコンのフォルダ(IconThemePath)は、直下も探す
    for (const ext of ['.png', '.svg']) {
      const f = path.join(dir, name + ext);
      if (fs.existsSync(f)) return f;
    }
  }
  return null;
}

// name: 絶対パス / file:// URL / アイコン名。見つからなければ null。extraRoots: アプリ指定のアイコンフォルダ
function toDataUrl(name, extraRoots = []) {
  if (!name || typeof name !== 'string') return null;
  const key = extraRoots.length ? `${extraRoots.join('|')}::${name}` : name;
  if (cache.has(key)) return cache.get(key);
  let r = null;
  if (name.startsWith('file://')) { try { r = fileToDataUrl(decodeURIComponent(new URL(name).pathname)); } catch {} }
  else if (path.isAbsolute(name)) r = fileToDataUrl(name);
  else if (/^[\w.+-]+$/.test(name)) { const f = findInTheme(name, extraRoots); r = f && fileToDataUrl(f); }
  cache.set(key, r);
  return r;
}

module.exports = { toDataUrl };
