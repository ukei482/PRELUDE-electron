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
  ['home', 'Home'],
  ['files', 'Folder'],
  ['field', 'Field'],
  ['web', 'Web'],
  ['app', 'App'],
];

const SCHEMA = [
  {
    title: 'Appearance',
    fields: [
      { path: 'appearance.accent', label: 'Accent color', type: 'color' },
      { path: 'appearance.bg', label: 'Background color', type: 'color' },
      { path: 'appearance.panel', label: 'Selection color', type: 'color' },
      { path: 'appearance.panel2', label: 'Hover color', type: 'color' },
      { path: 'appearance.line', label: 'Border / scrollbar color', type: 'color' },
      { path: 'appearance.text', label: 'Text color', type: 'color' },
      { path: 'appearance.dim', label: 'Dim text color', type: 'color' },
      { path: 'appearance.danger', label: 'Warning color', type: 'color' },
      { path: 'appearance.wallpaper', label: 'Home wallpaper (image file path, empty = none)', type: 'text' },
      { path: 'appearance.fontSize', label: 'Font size (px)', type: 'number', min: 10, max: 22 },
      { path: 'appearance.sidebarWidth', label: 'Sidebar width (px)', type: 'number', min: 140, max: 400 },
    ],
  },
  {
    // タブの開き方。既定は Arc に合わせる(別タブのリンクは新しいタブ、Alt+クリックは分割、ブックマーク(ピン留め)から別サイトは新しいタブ、新しいタブは一番上)
    title: 'Tabs',
    fields: [
      { path: 'tabs.linkOpen', label: 'Links that open in another tab (target=_blank, middle click, Ctrl+click)', type: 'select', options: [['tab', 'New tab'], ['split', 'Split']] },
      { path: 'tabs.altClick', label: 'Alt+click on a link', type: 'select', options: [['split', 'Split'], ['tab', 'New tab'], ['here', 'Navigate in place']] },
      { path: 'tabs.pinnedOtherSite', label: 'Links to another site from a bookmark tab', type: 'select', options: [['tab', 'New tab'], ['split', 'Split'], ['stay', 'Navigate in place']] },
      { path: 'tabs.newTabPosition', label: 'New tab position', type: 'select', options: [['top', 'Top'], ['next', 'After current tab'], ['bottom', 'Bottom']] },
    ],
  },
  {
    title: 'Behavior',
    fields: [
      { path: 'behavior.appOpen', label: 'Open apps from the launcher', type: 'select', options: [['embed', 'Embed in a pane'], ['window', 'Separate window']] },
      { path: 'behavior.viewInside', label: 'Extensions to view inside PRELUDE (web pane, space separated)', type: 'text' },
      { path: 'behavior.fileOpen', label: 'Where to open files (embeds the default app in a pane)', type: 'select', options: [['tab', 'New tab'], ['split', 'Split right'], ['window', 'Separate window (no embed)']] },
      { path: 'behavior.startKind', label: 'New tab type', type: 'select', options: KINDS },
      { path: 'behavior.homepage', label: 'Web home page', type: 'text' },
      { path: 'behavior.searchEngine', label: 'Search engine (%s = query)', type: 'text' },
      { path: 'behavior.chromiumSites', label: 'Sites to open in Chromium (space separated, includes subdomains, shares Google sign-in)', type: 'text' },
      { path: 'behavior.startPath', label: 'Folder start path (empty = home)', type: 'text' },
      { path: 'behavior.spaceDir', label: 'Space folder (each folder inside is a space, empty = ~/スペース)', type: 'text' },
      { path: 'behavior.downloadDir', label: 'Download start folder (empty = Downloads)', type: 'text' },
      { path: 'behavior.showHidden', label: 'Show hidden files (starting with .)', type: 'bool' },
      { path: 'behavior.terminal', label: 'Terminal command (%d = folder, empty = Konsole / Windows Terminal)', type: 'text' },
      { path: 'behavior.sidebarMode', label: 'Sidebar (☰ at top left cycles full → compact → hidden)', type: 'select', options: [['full', 'Full'], ['compact', 'Compact (icons only)'], ['hidden', 'Hidden']] },
      { path: 'behavior.startFullscreen', label: 'Start in fullscreen (from next launch)', type: 'bool' },
      { path: 'behavior.subDisplays', label: 'Open PRELUDE windows on secondary displays', type: 'bool' },
      { path: 'behavior.devDir', label: 'Dev build folder (opened in a pane by "PRELUDE dev build" in the launcher, empty = ~/PRELUDE-electron)', type: 'text' },
    ],
  },
  {
    title: 'Shortcuts',
    fields: [
      { path: 'shortcuts.newTab', label: 'New tab', type: 'text' },
      { path: 'shortcuts.closeTab', label: 'Close tab', type: 'text' },
      { path: 'shortcuts.reopenTab', label: 'Reopen closed tab', type: 'text' },
      { path: 'shortcuts.fullscreen', label: 'Fullscreen', type: 'text' },
      { path: 'shortcuts.launcher', label: 'Launcher (when PRELUDE is focused)', type: 'text' },
      { path: 'shortcuts.globalLauncher', label: 'Global launcher key (initial default; change it later in KDE shortcut settings)', type: 'text' },
    ],
  },
];

function defaults() {
  return {
    appearance: {
      bg: '#1926a3', panel: '#2b3dc8', panel2: '#3a50e0', line: '#ffffff',
      text: '#f0f3ff', dim: '#aab4ee', accent: '#ffffff', danger: '#ff8080',
      wallpaper: '', fontSize: 13, sidebarWidth: 220,
    },
    tabs: {
      linkOpen: 'tab',
      altClick: 'split',
      pinnedOtherSite: 'tab',
      newTabPosition: 'top',
    },
    behavior: {
      startKind: 'home',
      appOpen: 'embed',
      fileOpen: 'tab',
      viewInside: 'pdf html htm svg',
      homepage: 'https://www.google.com',
      searchEngine: 'https://www.google.com/search?q=%s',
      // Web ペイン(Electron)は Google にログインできないので、ログインが要る Google のサービスは最初から Chromium のペインで開く。
      // 検索(www.google.com)は含めない(新しいタブや検索のたびに Chromium になり、アドレス欄などが使えなくなるため)
      chromiumSites: 'mail.google.com drive.google.com docs.google.com sheets.google.com slides.google.com calendar.google.com photos.google.com keep.google.com meet.google.com chat.google.com contacts.google.com myaccount.google.com gemini.google.com notebooklm.google.com youtube.com',
      startPath: '',
      spaceDir: '',
      downloadDir: '',
      showHidden: false,
      terminal: '',
      sidebarMode: 'full',
      startFullscreen: true,
      subDisplays: true,
      devDir: '',
    },
    bookmarks: [],
    appUsage: [],
    shortcuts: { newTab: 'Ctrl+T', closeTab: 'Ctrl+W', reopenTab: 'Ctrl+Shift+T', fullscreen: 'F11', launcher: 'Ctrl+Shift+P', globalLauncher: 'Meta+Space' },
  };
}

// 0.7.0 までの既定の配色
const OLD_COLORS = {
  bg: '#faf3e0', panel: '#d7bfae', panel2: '#a1887f', line: '#a1887f',
  text: '#3e2f2a', dim: '#6f5a52', accent: '#5d4037', danger: '#9c4a3a',
};

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
        const over = JSON.parse(raw);
        // 0.4.11 までの「サイドバーを表示」(on/off)は、サイドバーの3段階(全部・細い列・非表示)へ引き継ぐ
        const b = over?.behavior;
        if (b && b.sidebarMode == null && b.sidebarVisible === false) b.sidebarMode = 'hidden';
        // 0.7.0 までの既定の配色(生成りと茶色)のままなら、新しい既定の配色(青)にする。自分で色を変えた人はそのまま
        const ap = over?.appearance;
        if (ap && Object.keys(OLD_COLORS).every((k) => String(ap[k]).toLowerCase() === OLD_COLORS[k])) {
          for (const k of Object.keys(OLD_COLORS)) delete ap[k];
        }
        merge(this.data, over);
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
