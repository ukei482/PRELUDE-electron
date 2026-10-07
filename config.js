'use strict';
// PRELUDE config — 変更できる設定はすべてここ(JSONで保存)。
// 設定を足すには:
//   1. defaults() に初期値を足す  2. SCHEMA にフィールドを足す(設定タブは SCHEMA から自動生成)
//   3. 読むときは config.get('behavior.xxx')
// 保存先: userData/config.json  (初回のみ、Rust版の %APPDATA%\PRELUDE\config.json があれば引き継ぐ)

const fs = require('fs');
const path = require('path');
const { app } = require('electron');

const KINDS = [
  ['files', 'フォルダ'],
  ['web', 'Web'],
  ['app', 'アプリ'],
];

const SCHEMA = [
  {
    title: '外観',
    fields: [
      { path: 'appearance.accent', label: 'アクセント色', type: 'color' },
      { path: 'appearance.bg', label: '背景色', type: 'color' },
      { path: 'appearance.panel', label: 'パネル色', type: 'color' },
      { path: 'appearance.panel2', label: 'パネル色（ホバー）', type: 'color' },
      { path: 'appearance.line', label: '罫線・スクロールバー色', type: 'color' },
      { path: 'appearance.text', label: '文字色', type: 'color' },
      { path: 'appearance.dim', label: '補助文字色', type: 'color' },
      { path: 'appearance.danger', label: '警告色', type: 'color' },
      { path: 'appearance.fontSize', label: '文字サイズ (px)', type: 'number', min: 10, max: 22 },
      { path: 'appearance.sidebarWidth', label: 'サイドバー幅 (px)', type: 'number', min: 140, max: 400 },
    ],
  },
  {
    title: '動作',
    fields: [
      { path: 'behavior.appOpen', label: 'アプリを開く方法（ランチャー）', type: 'select', options: [['embed', 'ペインに取り込む'], ['window', '別ウィンドウ']] },
      { path: 'behavior.startKind', label: '新しいタブの種類', type: 'select', options: KINDS },
      { path: 'behavior.homepage', label: 'Webのホームページ', type: 'text' },
      { path: 'behavior.searchEngine', label: '検索エンジン（%s が検索語）', type: 'text' },
      { path: 'behavior.startPath', label: 'フォルダの開始位置（空=ホーム）', type: 'text' },
      { path: 'behavior.downloadDir', label: 'ダウンロードの開始フォルダ（空=ダウンロードフォルダ）', type: 'text' },
      { path: 'behavior.showHidden', label: '隠しファイル（.で始まる）を表示', type: 'bool' },
      { path: 'behavior.sidebarVisible', label: 'サイドバーを表示', type: 'bool' },
      { path: 'behavior.startFullscreen', label: '起動時に全画面で開く（次回起動から）', type: 'bool' },
    ],
  },
  {
    title: 'ショートカット',
    fields: [
      { path: 'shortcuts.newTab', label: '新しいタブ', type: 'text' },
      { path: 'shortcuts.closeTab', label: 'タブを閉じる', type: 'text' },
      { path: 'shortcuts.fullscreen', label: '全画面', type: 'text' },
      { path: 'shortcuts.launcher', label: 'ランチャー（PRELUDEが前面のとき）', type: 'text' },
      { path: 'shortcuts.globalLauncher', label: 'グローバルのランチャーキー（初回登録時の既定。以降の変更はKDEのショートカット設定で）', type: 'text' },
    ],
  },
];

function defaults() {
  return {
    appearance: {
      bg: '#faf3e0', panel: '#d7bfae', panel2: '#a1887f', line: '#a1887f',
      text: '#3e2f2a', dim: '#6f5a52', accent: '#5d4037', danger: '#9c4a3a',
      fontSize: 13, sidebarWidth: 220,
    },
    behavior: {
      startKind: 'files',
      appOpen: 'embed',
      homepage: 'https://www.google.com',
      searchEngine: 'https://www.google.com/search?q=%s',
      startPath: '',
      downloadDir: '',
      showHidden: false,
      sidebarVisible: true,
      startFullscreen: true,
    },
    bookmarks: [],
    appUsage: [],
    shortcuts: { newTab: 'Ctrl+T', closeTab: 'Ctrl+W', fullscreen: 'F11', launcher: 'Ctrl+Shift+P', globalLauncher: 'Meta+Space' },
  };
}

// base に存在するキーだけ、同じ型の値で上書きする
function merge(base, over) {
  if (!base || typeof base !== 'object' || !over || typeof over !== 'object') return;
  for (const k of Object.keys(base)) {
    if (!(k in over)) continue;
    const b = base[k];
    const o = over[k];
    if (Array.isArray(b)) {
      if (Array.isArray(o)) base[k] = o;
    } else if (b && typeof b === 'object') {
      merge(b, o);
    } else if (typeof b === typeof o) {
      base[k] = o;
    }
  }
}

class Config {
  constructor() {
    this.file = path.join(app.getPath('userData'), 'config.json');
    this.data = defaults();
    this.load();
  }

  load() {
    let raw = null;
    try {
      raw = fs.readFileSync(this.file, 'utf8');
    } catch {
      // 初回起動: Rust版の設定があれば引き継ぐ
      try {
        raw = fs.readFileSync(path.join(app.getPath('appData'), 'PRELUDE', 'config.json'), 'utf8');
      } catch {}
    }
    if (raw) {
      try {
        merge(this.data, JSON.parse(raw));
      } catch {}
    }
  }

  save() {
    try {
      fs.mkdirSync(path.dirname(this.file), { recursive: true });
      fs.writeFileSync(this.file, JSON.stringify(this.data, null, 2));
    } catch (e) {
      console.error('config save failed', e);
    }
  }

  get(p) {
    return p.split('.').reduce((v, k) => (v == null ? undefined : v[k]), this.data);
  }

  set(p, value) {
    const ks = p.split('.');
    const last = ks.pop();
    let cur = this.data;
    for (const k of ks) cur = cur[k] ??= {};
    cur[last] = value;
    this.save();
  }

  reset() {
    const { bookmarks, appUsage } = this.data;
    this.data = defaults();
    this.data.bookmarks = bookmarks;
    this.data.appUsage = appUsage;
    this.save();
  }
}

module.exports = { Config, SCHEMA };
