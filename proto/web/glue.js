// コアとサイドバーの部品のつなぎ(Slint の試作の src/core.rs にあたる。本番ではホストの外側が受け持つ)。
// - values(): コアの様子(状態・ブックマーク・フォルダ・レシピ)と、サイドバーが覚える見た目の状態 → 部品の props
// - steps(): 部品の callback → コアの命令(prelude-proto の Command)・見た目の状態の変更・ランチャー
// DOM を使わないので、node のテストで確かめられる。
(function (root, factory) {
  const m = factory();
  if (typeof module === 'object' && module.exports) module.exports = m;
  else root.PreludeGlue = m;
})(typeof self !== 'undefined' ? self : this, function () {
  'use strict';

  // 配置(TabInfo.layout)のペインを左上から順に(prelude-proto の panes と同じ)
  function leaves(l, out = []) {
    if (!l || typeof l !== 'object') return out;
    if (l.k === 'split') {
      leaves(l.a, out);
      leaves(l.b, out);
    } else if (Number.isInteger(l.id)) out.push(l);
    return out;
  }

  // 出す名前: 窓の題名 → URL のホスト・アプリの名前・フォルダの名前 → 種類
  function paneTitle(l) {
    const kind = l.type || '';
    const base = (p) => String(p || '').replace(/\/+$/, '').split('/').pop();
    let place = '';
    if (kind === 'web') {
      const u = String(l.url || '');
      place = (u.split('://')[1] ?? u).split('/')[0];
    } else if (kind === 'app') place = l.name || '';
    else place = base(l.path);
    return [l.window_title, l.title, place].find((t) => t) || kind;
  }

  // ペインの画面の上の大きさ(配置の比率から。画面全体を screen とする)
  function paneRect(l, id, r = { w: 16, h: 9 }) {
    if (!l || typeof l !== 'object') return null;
    if (l.k !== 'split') return l.id === id ? r : null;
    const t = typeof l.ratio === 'number' ? l.ratio : 0.5;
    const [ra, rb] = l.dir === 'col' ? [{ w: r.w, h: r.h * t }, { w: r.w, h: r.h * (1 - t) }] : [{ w: r.w * t, h: r.h }, { w: r.w * (1 - t), h: r.h }];
    return paneRect(l.a, id, ra) || paneRect(l.b, id, rb);
  }
  // 自動の分割の向き: 横長(幅が高さ以上)なら右、縦長なら下
  const autoSplit = (r) => (!r || r.w >= r.h ? 'right' : 'down');
  // 分割の向き → ランチャーの引数(コアの分割は row・col と、前に置くか)
  const SPLIT = { right: ['row', false], left: ['row', true], down: ['col', false], up: ['col', true], row: ['row', false], col: ['col', false] };

  const folderOf = (path) => path.replace(/\/[^/]*$/, '');
  // bookmark.list の folder はスペースの中の名前(「研究」)。部品と命令はフォルダのパスで指すので、パスにする
  const bmFolder = (b, st) => (b.folder ? `${(st && st.space_dir) || ''}/${b.folder}` : '');

  // core: { online, status, state, spaces, bookmarks, folders, recipes }
  // ui: { compact, expanded: { フォルダのパス: bool }, splitMode: 'auto' | 'pick', screen: { w, h } }
  // iconUrl(パス) → 画像の URL(ホストがファイルを出す道。空なら空)
  function values(core, ui, iconUrl = (p) => p) {
    const st = core.state || { space: '', tabs: [], active: null, windows: { backend: '', level: 0 }, closed: 0 };
    const icon = (p) => (p ? iconUrl(p) : '');
    const tabs = st.tabs || [];
    const openBms = new Set(tabs.map((t) => t.bookmark).filter(Boolean));
    const level = (st.windows && st.windows.level) || 0;

    const items = tabs.map((t) => {
      const ls = leaves(t.layout);
      const cur = ls.find((l) => l.id === t.active_pane) || ls[0] || {};
      return {
        id: t.id,
        num: t.num,
        title: t.title,
        kind: cur.type || '',
        icon: icon(cur.icon),
        active: t.id === st.active,
        bookmark: t.bookmark || '',
        paneCount: ls.length,
      };
    });
    const panes = tabs.flatMap((t) => {
      const ls = leaves(t.layout);
      if (ls.length < 2) return [];
      return ls.map((l) => ({ id: l.id, tab: t.id, title: paneTitle(l), kind: l.type || '', icon: icon(l.icon), active: l.id === t.active_pane }));
    });

    const activeTab = tabs.find((t) => t.id === st.active);
    const cur = activeTab ? leaves(activeTab.layout).find((l) => l.id === activeTab.active_pane) : null;
    // ブックマークのアイコン: 開いていればそのタブのアイコン(コアの bookmark.list にはまだアイコンが無い)
    const bmIcon = Object.fromEntries(items.filter((t) => t.bookmark).map((t) => [t.bookmark, t.icon]));
    const bookmarks = (core.bookmarks || []).map((b) => ({ ...b, folder: bmFolder(b, st) }));
    const titleOfBm = Object.fromEntries(bookmarks.map((b) => [b.path, b.title]));

    return {
      SidebarFrame: { online: !!core.online, status: core.status || '', compact: !!ui.compact, windowsLevel: level },
      Spaces: { current: st.space || '', all: core.spaces || [] },
      Bookmarks: {
        items: bookmarks.map((b) => ({
          path: b.path,
          title: b.title,
          num: b.num || 0,
          kind: b.kind || '',
          icon: bmIcon[b.path] || '',
          open: openBms.has(b.path),
          folder: b.folder || '',
        })),
        folders: (core.folders || []).map((f) => ({
          path: f.path,
          title: f.title,
          expanded: !!(ui.expanded || {})[f.path],
          hasOpen: bookmarks.some((b) => b.folder === f.path && openBms.has(b.path)),
        })),
      },
      Tabs: { items, panes, canReopen: (st.closed || 0) > 0 },
      NewTab: { kinds: [{ kind: 'web', name: 'Web' }, ...(core.recipes || []).map((r) => ({ kind: r.type, name: r.name }))] },
      PaneBar: {
        pane: cur ? cur.id : 0,
        kind: cur ? cur.type || '' : '',
        title: cur ? paneTitle(cur) : '',
        url: cur && cur.type === 'web' ? cur.url || '' : '',
        canSplit: level >= 2 && !!cur,
        canMute: !!cur && cur.type === 'web',
        muted: !!(cur && cur.muted),
        canGoBack: !!cur && cur.type === 'web',
        splitMode: ui.splitMode === 'pick' ? 'pick' : 'auto',
        autoSplit: autoSplit(activeTab && cur ? paneRect(activeTab.layout, cur.id, ui.screen || undefined) : null),
      },
      BookmarkButton: {
        bookmark: activeTab ? activeTab.bookmark || '' : '',
        bookmarkTitle: activeTab && activeTab.bookmark ? titleOfBm[activeTab.bookmark] || '' : '',
        paneCount: activeTab ? leaves(activeTab.layout).length : 0,
      },
    };
  }

  // 部品の callback → 手順の配列。手順は次のどれか:
  // { cmd, args }(コアの命令)・{ ui: 'compact' | 'hide' } ・{ ui: 'expand', path, on }・{ launcher: { split?, before?, pane?, url? } }(prelude-launcher の引数)
  function steps(part, cb, args, core) {
    const st = (core && core.state) || { tabs: [] };
    const c = (cmd, a = {}) => ({ cmd, args: a });
    const tab = (id) => ({ id });
    const k = `${part}.${cb}`;
    const [a, b] = args;
    switch (k) {
      case 'SidebarFrame.toggleCompact':
        return [{ ui: 'compact' }];
      case 'SidebarFrame.hide':
        return [{ ui: 'hide' }];
      case 'Spaces.switchTo':
        return [c('space.switch', { name: a })];
      case 'Spaces.create':
        return [c('space.create', { name: a })];
      case 'Bookmarks.open':
        return [c('bookmark.open', { path: a })];
      case 'Bookmarks.openAll':
        return (core.bookmarks || []).filter((x) => bmFolder(x, st) === a).map((x) => c('bookmark.open', { path: x.path }));
      case 'Bookmarks.rename':
        return [c('bookmark.rename', { path: a, title: b })];
      case 'Bookmarks.moveTo':
        return [c('bookmark.move', { path: a, folder: b })];
      case 'Bookmarks.reorder':
        return [c('bookmark.reorder', { path: a, before: b })];
      case 'Bookmarks.reveal':
        return [c('tab.open', { pane: { type: 'files', path: folderOf(a) } })];
      case 'Bookmarks.trash':
        return [c('bookmark.trash', { path: a })];
      case 'Bookmarks.createFolder':
        return [c('folder.create', { name: a })];
      case 'Bookmarks.renameFolder':
        return [c('folder.rename', { path: a, name: b })];
      case 'Bookmarks.removeFolder':
        return [c('folder.remove', { path: a })];
      case 'Bookmarks.setExpanded':
        return [{ ui: 'expand', path: a, on: b }];
      case 'Tabs.activate':
        return [c('tab.activate', { tab: tab(a) })];
      case 'Tabs.close':
        return [c('tab.close', { tab: tab(a) })];
      case 'Tabs.focusPane':
        return [c('pane.focus', { pane: a })];
      case 'Tabs.moveTo':
        return [c('tab.move', { tab: tab(a), index: b })];
      case 'Tabs.duplicate':
        return [c('tab.duplicate', { tab: tab(a) })];
      case 'Tabs.renumber':
        return [c('tab.renumber', { tab: tab(a), num: b })];
      case 'Tabs.closeBelow': {
        const i = st.tabs.findIndex((t) => t.id === a);
        return i < 0 ? [] : st.tabs.slice(i + 1).map((t) => c('tab.close', { tab: tab(t.id) }));
      }
      case 'Tabs.reopen':
        return [c('tab.reopen')];
      case 'NewTab.open':
        return a === 'web' ? [{ launcher: {} }] : [c('tab.open', { pane: { type: a } })];
      case 'PaneBar.split': {
        const v = values(core, {}).PaneBar;
        const [dir, before] = SPLIT[a] || SPLIT.right;
        return v.pane ? [{ launcher: { split: dir, before, pane: v.pane } }] : [];
      }
      case 'PaneBar.back':
      case 'PaneBar.forward': {
        const v = values(core, {}).PaneBar;
        return v.pane && v.canGoBack ? [c('web.go', { pane: v.pane, action: cb })] : [];
      }
      case 'PaneBar.close':
        return [c('pane.close', { pane: values(core, {}).PaneBar.pane })];
      case 'PaneBar.changeUrl': {
        const v = values(core, {}).PaneBar;
        return [{ launcher: { pane: v.pane, url: v.url } }];
      }
      case 'PaneBar.setMuted':
        return [c('web.mute', { pane: values(core, {}).PaneBar.pane, on: a })];
      case 'BookmarkButton.save':
        return [c('bookmark.save', { title: a })];
      case 'BookmarkButton.overwrite':
        return [c('bookmark.overwrite')];
    }
    throw new Error(`${k}: つなぎが無い`);
  }

  return { values, steps, leaves, paneTitle, paneRect, autoSplit };
});
