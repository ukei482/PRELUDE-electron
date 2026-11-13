// 偽のコア: コアが無くても試作を触れるように、状態とブックマークを持ち、命令(prelude-proto の Command)をそれらしく当てはめる。
// 形は本物のコアと同じ(state・bookmark.list・folder.list・recipe.list・space.list)。
(function (root, factory) {
  const m = factory();
  if (typeof module === 'object' && module.exports) module.exports = m;
  else root.PreludeFake = m;
})(typeof self !== 'undefined' ? self : this, function () {
  'use strict';

  // アプリのアイコンの代わり(色の付いた角丸に字)
  const tile = (bg, ch, fg = '#fff') =>
    'data:image/svg+xml,' +
    encodeURIComponent(
      `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 32 32"><rect width="32" height="32" rx="8" fill="${bg}"/><text x="16" y="22" font-size="17" font-family="sans-serif" font-weight="700" text-anchor="middle" fill="${fg}">${ch}</text></svg>`,
    );
  const ICONS = {
    github: tile('#24292f', 'G'),
    docs: tile('#e8710a', 'D'),
    kate: tile('#1d99f3', 'K'),
    yazi: tile('#f5c211', 'Y', '#3a2e00'),
    foot: tile('#3b3f4c', '>'),
    claude: tile('#d97757', '✳'),
    mdn: tile('#000', 'M'),
    youtube: tile('#ff0033', '▶'),
    gimp: tile('#6e5d4a', 'W'),
  };

  function create() {
    let nextId = 100;
    const id = () => nextId++;
    const dir = (s) => `/home/u/.local/share/prelude/spaces/${s}`;
    const leaf = (o) => Object.assign({ k: 'leaf', id: id() }, o);
    const split = (d, a, b, ratio = 0.5) => ({ k: 'split', dir: d, ratio, a, b });

    const spaces = {
      仕事: {
        tabs: [],
        closed: [],
        folders: [
          { path: `${dir('仕事')}/研究`, title: '研究' },
          { path: `${dir('仕事')}/毎日`, title: '毎日' },
        ],
        bookmarks: [
          { title: 'PRELUDE の開発', folder: '', kind: 'layout' },
          { title: 'GitHub', folder: '', kind: 'web' },
          { title: 'Claude と相談', folder: '', kind: 'agent' },
          { title: '論文を読む', folder: '研究', kind: 'layout' },
          { title: 'MDN', folder: '研究', kind: 'web' },
          { title: '受信箱', folder: '毎日', kind: 'web' },
          { title: '日報', folder: '毎日', kind: 'app' },
        ],
      },
      趣味: { tabs: [], closed: [], folders: [], bookmarks: [{ title: '動画', folder: '', kind: 'web' }, { title: 'お絵かき', folder: '', kind: 'app' }] },
      'PRELUDE 開発': { tabs: [], closed: [], folders: [], bookmarks: [] },
    };
    for (const [name, s] of Object.entries(spaces)) {
      s.bookmarks = s.bookmarks.map((b) => {
        const folder = b.folder ? `${dir(name)}/${b.folder}` : '';
        return { ...b, folder, path: `${folder || dir(name)}/${b.title}.prelude` };
      });
    }
    const w = spaces['仕事'];
    const bm = (t) => w.bookmarks.find((b) => b.title === t).path;
    w.tabs = [
      {
        id: id(), num: 1, title: 'PRELUDE の開発', bookmark: bm('PRELUDE の開発'),
        layout: split('row',
          leaf({ type: 'app', name: 'Kate', cmd: 'kate', window_title: 'sidebar.css — Kate', icon: ICONS.kate }),
          split('col', leaf({ type: 'console', path: '/home/u/PRELUDE', window_title: 'cargo test', icon: ICONS.foot }), leaf({ type: 'files', path: '/home/u/PRELUDE/proto/web', icon: ICONS.yazi }), 0.6),
          0.58),
      },
      { id: id(), num: 2, title: 'GitHub', bookmark: bm('GitHub'), layout: leaf({ type: 'web', url: 'https://github.com/ukei482/prelude-electron/pulls', title: 'Pull requests', icon: ICONS.github }) },
      { id: id(), num: 3, title: 'Claude と相談', bookmark: bm('Claude と相談'), layout: leaf({ type: 'agent', path: '/home/u/PRELUDE', window_title: 'claude', icon: ICONS.claude }) },
      { id: id(), num: 4, title: 'docs.rs — slint', bookmark: null, layout: leaf({ type: 'web', url: 'https://docs.rs/slint/latest/slint/', title: 'slint - Rust', icon: ICONS.docs, muted: true }) },
    ];
    for (const t of w.tabs) {
      const ls = leaves(t.layout);
      t.active_pane = ls[0].id;
    }
    w.closed = [{ title: 'Wikipedia', layout: () => leaf({ type: 'web', url: 'https://ja.wikipedia.org/', icon: '' }) }];
    let space = '仕事';
    let active = w.tabs[0].id;
    let bookmarksRev = 1;
    let level = 4; // 窓のつなぎの段(見た目の確かめで変えられる)

    function leaves(l, out = []) {
      if (l.k === 'split') {
        leaves(l.a, out);
        leaves(l.b, out);
      } else out.push(l);
      return out;
    }
    const cur = () => spaces[space];
    const tabOf = (ref) => cur().tabs.find((t) => (ref && ref.id != null ? t.id === ref.id : ref && ref.num != null ? t.num === ref.num : t.id === active));
    const freeNum = () => {
      const used = new Set(cur().tabs.map((t) => t.num));
      let n = 1;
      while (used.has(n)) n++;
      return n;
    };
    const titleOfLeaf = (l) => l.title || l.window_title || l.name || (l.url ? l.url.split('/')[2] : '') || l.type;
    const addTab = (layout, title, bookmark = null, index = 0) => {
      const t = { id: id(), num: freeNum(), title, bookmark, layout };
      t.active_pane = leaves(layout)[0].id;
      cur().tabs.splice(index, 0, t);
      active = t.id;
      return t;
    };
    const removeLeaf = (l, pid) => {
      if (l.k !== 'split') return l.id === pid ? null : l;
      const a = removeLeaf(l.a, pid);
      const b = removeLeaf(l.b, pid);
      if (!a) return b;
      if (!b) return a;
      return { ...l, a, b };
    };
    const layoutFor = (b) => {
      if (b.kind === 'web') return leaf({ type: 'web', url: 'https://example.com/', title: b.title, icon: '' });
      if (b.kind === 'agent') return leaf({ type: 'agent', path: '/home/u', icon: ICONS.claude });
      if (b.kind === 'app') return leaf({ type: 'app', name: b.title, cmd: 'gimp', icon: ICONS.gimp });
      return split('row', leaf({ type: 'web', url: 'https://arxiv.org/', title: 'arXiv', icon: '' }), leaf({ type: 'app', name: 'Okular', cmd: 'okular', icon: '' }));
    };
    const closeTab = (t) => {
      const s = cur();
      const i = s.tabs.indexOf(t);
      if (i < 0) return;
      s.tabs.splice(i, 1);
      const layout = t.layout;
      s.closed.unshift({ title: t.title, bookmark: t.bookmark, layout: () => layout });
      if (active === t.id) active = (s.tabs[i] || s.tabs[i - 1] || {}).id ?? null;
    };
    const order = (s) => {
      // 直下 → フォルダの名前の順。番号を振り直す
      const top = s.bookmarks.filter((b) => !b.folder);
      const inF = s.folders.flatMap((f) => s.bookmarks.filter((b) => b.folder === f.path));
      s.bookmarks = [...top, ...inF];
      s.bookmarks.forEach((b, i) => (b.num = i + 1));
    };
    Object.values(spaces).forEach(order);

    const COMMANDS = {
      'state.get': () => snapshot().state,
      'space.list': () => Object.keys(spaces),
      'space.switch': ({ name }) => {
        if (!spaces[name]) throw new Error(`スペース ${name} はありません`);
        space = name;
        active = (cur().tabs[0] || {}).id ?? null;
      },
      'space.create': ({ name }) => {
        spaces[name] = spaces[name] || { tabs: [], closed: [], folders: [], bookmarks: [] };
      },
      // folder はスペースの中の名前(本物のコアと同じ)
      'bookmark.list': () => cur().bookmarks.map(({ path, title, folder, num, kind }) => ({ path, title, folder: folder ? folder.slice(dir(space).length + 1) : '', num, kind })),
      'folder.list': () => cur().folders,
      'recipe.list': () => [
        { type: 'files', name: 'ファイラ' },
        { type: 'console', name: 'コンソール' },
        { type: 'agent', name: 'AI' },
      ],
      'bookmark.open': ({ path }) => {
        const t = cur().tabs.find((x) => x.bookmark === path);
        if (t) active = t.id;
        else {
          const b = cur().bookmarks.find((x) => x.path === path);
          addTab(layoutFor(b), b.title, path);
        }
        return { tab: active, held: [] };
      },
      'bookmark.rename': ({ path, title }) => {
        const b = cur().bookmarks.find((x) => x.path === path);
        const np = `${b.folder || dir(space)}/${title}.prelude`;
        cur().tabs.forEach((t) => t.bookmark === path && (t.bookmark = np));
        Object.assign(b, { title, path: np });
        bookmarksRev++;
        return np;
      },
      'bookmark.move': ({ path, folder }) => {
        const b = cur().bookmarks.find((x) => x.path === path);
        const np = `${folder || dir(space)}/${b.title}.prelude`;
        cur().tabs.forEach((t) => t.bookmark === path && (t.bookmark = np));
        Object.assign(b, { folder, path: np });
        order(cur());
        bookmarksRev++;
        return np;
      },
      'bookmark.reorder': ({ path, before }) => {
        const s = cur();
        const b = s.bookmarks.find((x) => x.path === path);
        s.bookmarks.splice(s.bookmarks.indexOf(b), 1);
        const i = before ? s.bookmarks.findIndex((x) => x.path === before) : s.bookmarks.length;
        s.bookmarks.splice(i < 0 ? s.bookmarks.length : i, 0, b);
        order(s);
        bookmarksRev++;
      },
      'bookmark.trash': ({ path }) => {
        const s = cur();
        s.bookmarks = s.bookmarks.filter((x) => x.path !== path);
        s.tabs.forEach((t) => t.bookmark === path && (t.bookmark = null));
        order(s);
        bookmarksRev++;
      },
      'bookmark.save': ({ title }) => {
        const t = tabOf();
        const s = cur();
        const path = `${dir(space)}/${title}.prelude`;
        s.bookmarks.push({ title, folder: '', path, kind: leaves(t.layout).length > 1 ? 'layout' : leaves(t.layout)[0].type });
        order(s);
        t.bookmark = path;
        t.title = title;
        bookmarksRev++;
        return path;
      },
      'bookmark.overwrite': () => {
        bookmarksRev++;
      },
      'folder.create': ({ name }) => {
        const s = cur();
        s.folders.push({ path: `${dir(space)}/${name}`, title: name });
        s.folders.sort((a, b) => a.title.localeCompare(b.title, 'ja'));
        order(s);
        bookmarksRev++;
      },
      'folder.rename': ({ path, name }) => {
        const s = cur();
        const f = s.folders.find((x) => x.path === path);
        const np = `${dir(space)}/${name}`;
        s.bookmarks.forEach((b) => {
          if (b.folder === path) {
            const old = b.path;
            b.folder = np;
            b.path = `${np}/${b.title}.prelude`;
            s.tabs.forEach((t) => t.bookmark === old && (t.bookmark = b.path));
          }
        });
        Object.assign(f, { path: np, title: name });
        bookmarksRev++;
      },
      'folder.remove': ({ path }) => {
        const s = cur();
        s.bookmarks.forEach((b) => b.folder === path && Object.assign(b, { folder: '', path: `${dir(space)}/${b.title}.prelude` }));
        s.folders = s.folders.filter((f) => f.path !== path);
        order(s);
        bookmarksRev++;
      },
      'tab.open': ({ pane, index }) => addTab(leaf({ ...pane, icon: { files: ICONS.yazi, console: ICONS.foot, agent: ICONS.claude }[pane.type] || '' }), pane.type === 'files' ? (pane.path || '').split('/').pop() || 'ファイラ' : { console: 'コンソール', agent: 'AI', web: 'Web' }[pane.type] || pane.type, null, index || 0).id,
      'tab.close': ({ tab }) => closeTab(tabOf(tab)),
      'tab.activate': ({ tab }) => {
        active = tabOf(tab).id;
      },
      'tab.duplicate': ({ tab }) => {
        const t = tabOf(tab);
        const clone = JSON.parse(JSON.stringify(t.layout));
        leaves(clone).forEach((l) => (l.id = id()));
        const n = addTab(clone, t.title, null, cur().tabs.indexOf(t) + 1);
        return n.id;
      },
      'tab.renumber': ({ tab, num }) => {
        const t = tabOf(tab);
        const other = cur().tabs.find((x) => x.num === num);
        if (other) other.num = t.num;
        t.num = num;
      },
      'tab.reopen': () => {
        const c = cur().closed.shift();
        if (!c) throw new Error('閉じたタブはありません');
        return addTab(c.layout(), c.title, c.bookmark || null).id;
      },
      'tab.move': ({ tab, index }) => {
        const s = cur();
        const t = tabOf(tab);
        s.tabs.splice(s.tabs.indexOf(t), 1);
        s.tabs.splice(Math.min(index, s.tabs.length), 0, t);
      },
      'pane.focus': ({ pane }) => {
        const t = cur().tabs.find((x) => leaves(x.layout).some((l) => l.id === pane));
        t.active_pane = pane;
        active = t.id;
      },
      'pane.close': ({ pane }) => {
        const t = cur().tabs.find((x) => leaves(x.layout).some((l) => l.id === pane));
        const rest = removeLeaf(t.layout, pane);
        if (!rest) return closeTab(t);
        t.layout = rest;
        t.active_pane = leaves(rest)[0].id;
      },
      'web.mute': ({ pane, on }) => {
        for (const t of cur().tabs) for (const l of leaves(t.layout)) if (l.id === pane) l.muted = on || undefined;
      },
    };

    function snapshot() {
      const s = cur();
      const tabs = s.tabs.map((t) => {
        if (!t.title) t.title = titleOfLeaf(leaves(t.layout)[0]);
        return { id: t.id, num: t.num, title: t.title, bookmark: t.bookmark, active_pane: t.active_pane, layout: JSON.parse(JSON.stringify(t.layout)) };
      });
      return {
        online: true,
        status: '',
        state: { space, space_dir: dir(space), active, tabs, windows: { backend: level ? 'sway' : 'none', level }, closed: s.closed.length, bookmarks_rev: bookmarksRev, config_rev: 0 },
        spaces: COMMANDS['space.list'](),
        bookmarks: COMMANDS['bookmark.list'](),
        folders: COMMANDS['folder.list'](),
        recipes: COMMANDS['recipe.list'](),
      };
    }

    // 命令 1 つ → 返事。知らない命令は投げる
    function run(cmd, args = {}) {
      const f = COMMANDS[cmd];
      if (!f) throw new Error(`${cmd}: 偽のコアには無い命令`);
      return f(args);
    }

    return { run, snapshot, setLevel: (n) => (level = n) };
  }

  return { create, ICONS };
});
