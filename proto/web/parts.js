// サイドバーの部品の決まり(docs/redesign-rust.md の「サイドバー」の「部品」。proto/slint/ui/parts.slint と同じ中身)。
// 部品ごとに、props(コア → 部品。表示するもの)と callbacks(部品 → コア。頼むこと)を持つ。見た目と並べ方は決めない。
//
// 決まりを守らせる仕組み(Slint ならビルドで止まるところを、ここでは使った瞬間に止める):
// - ホスト(コアとのつなぎ)が満たす props は、全部そろっていて型が合っていないと make() が投げる
// - サイドバーが決まりに無い部品・props を読むと投げる(Proxy)
// - サイドバーが決まりに無い callback を呼ぶか、引数の型が違うと投げる
// サイドバーはこの parts 以外の方法でコアとやり取りしない。テスト(test/parts.test.js)は、サイドバーを偽の値で描いて確かめる。
//
// 型: 'bool'・'int'・'string'・'image'(画像の URL。空なら無し)・['型'](その配列)・構造体の名前
(function (root, factory) {
  const m = factory();
  if (typeof module === 'object' && module.exports) module.exports = m;
  else root.PreludeParts = m;
})(typeof self !== 'undefined' ? self : this, function () {
  'use strict';

  const STRUCTS = {
    BookmarkItem: {
      path: 'string',
      title: 'string',
      num: 'int', // ランチャーの b3 と同じ番号。0 は番号無し
      kind: 'string', // 1 つのペインならその種類、分割していれば layout
      icon: 'image',
      open: 'bool', // このブックマークを開いたタブがあるか
      folder: 'string', // 入っているフォルダのパス(スペースの直下なら空)
    },
    BookmarkFolder: {
      path: 'string',
      title: 'string',
      expanded: 'bool', // サイドバーが覚える見た目の状態
      hasOpen: 'bool', // 中に開いているブックマークがあるか
    },
    TabItem: {
      id: 'int',
      num: 'int',
      title: 'string',
      kind: 'string', // 今のペインの種類
      icon: 'image',
      active: 'bool',
      bookmark: 'string', // 開いたブックマーク(.prelude のパス。無ければ空)
      paneCount: 'int',
    },
    PaneItem: {
      id: 'int',
      tab: 'int', // 入っているタブの id
      title: 'string',
      kind: 'string',
      icon: 'image',
      active: 'bool', // タブの中の今のペインか
    },
    PaneKind: { kind: 'string', name: 'string' },
  };

  const PARTS = {
    // 枠: つながっているか・幅
    SidebarFrame: {
      props: { online: 'bool', status: 'string', compact: 'bool', windowsLevel: 'int' },
      callbacks: { toggleCompact: [], hide: [] },
    },
    Spaces: {
      props: { current: 'string', all: ['string'] },
      callbacks: { switchTo: ['string'], create: ['string'] },
    },
    Bookmarks: {
      props: { items: ['BookmarkItem'], folders: ['BookmarkFolder'] },
      callbacks: {
        open: ['string'],
        openAll: ['string'], // フォルダの中を全部開く
        rename: ['string', 'string'], // (パス, 新しい題名)
        moveTo: ['string', 'string'], // (パス, フォルダのパス。空ならスペースの直下)
        reorder: ['string', 'string'], // (パス, この前に置くブックマークのパス。空なら最後)
        reveal: ['string'], // 入っているフォルダをファイラで見る
        trash: ['string'],
        createFolder: ['string'],
        renameFolder: ['string', 'string'],
        removeFolder: ['string'], // 中身はスペースの直下へ出す
        setExpanded: ['string', 'bool'],
      },
    },
    Tabs: {
      props: { items: ['TabItem'], panes: ['PaneItem'], canReopen: 'bool' },
      callbacks: {
        activate: ['int'],
        close: ['int'],
        focusPane: ['int'],
        moveTo: ['int', 'int'], // (タブの id, 移す先の位置。0 が一番上)
        duplicate: ['int'],
        renumber: ['int', 'int'],
        closeBelow: ['int'],
        reopen: [],
      },
    },
    NewTab: {
      props: { kinds: ['PaneKind'] },
      callbacks: { open: ['string'] }, // web はランチャーを出す
    },
    // 分割ボタン(今のペインの操作)
    PaneBar: {
      props: {
        pane: 'int', // 今のペインの id(無ければ 0)
        kind: 'string',
        title: 'string',
        url: 'string',
        canSplit: 'bool', // 窓のつなぎの段が 2 以上
        canMute: 'bool',
        muted: 'bool',
      },
      callbacks: { split: ['string'], close: [], changeUrl: [], setMuted: ['bool'] },
    },
    // ブックマークボタン(今のタブをブックマークにする)
    BookmarkButton: {
      props: { bookmark: 'string', bookmarkTitle: 'string', paneCount: 'int' },
      callbacks: { save: ['string'], overwrite: [] },
    },
  };

  function check(type, v, where) {
    if (Array.isArray(type)) {
      if (!Array.isArray(v)) throw new TypeError(`${where}: 配列ではない`);
      v.forEach((x, i) => check(type[0], x, `${where}[${i}]`));
      return;
    }
    const ok = {
      bool: () => typeof v === 'boolean',
      int: () => Number.isInteger(v),
      string: () => typeof v === 'string',
      image: () => typeof v === 'string',
    }[type];
    if (ok) {
      if (!ok()) throw new TypeError(`${where}: ${type} ではない(${JSON.stringify(v)})`);
      return;
    }
    const s = STRUCTS[type];
    if (!s) throw new TypeError(`${where}: 知らない型 ${type}`);
    if (v === null || typeof v !== 'object') throw new TypeError(`${where}: ${type} ではない`);
    for (const k of Object.keys(s)) {
      if (!(k in v)) throw new TypeError(`${where}.${k}: ${type} の項目が無い`);
      check(s[k], v[k], `${where}.${k}`);
    }
    for (const k of Object.keys(v)) if (!(k in s)) throw new TypeError(`${where}.${k}: ${type} に無い項目`);
  }

  // 読むと投げる、読み取り専用の包み(構造体の中・配列の要素も)
  function guard(v, where) {
    if (v === null || typeof v !== 'object') return v;
    return new Proxy(v, {
      get(t, k) {
        if (typeof k === 'symbol' || (Array.isArray(t) && (k in Array.prototype || k === 'length'))) {
          const x = Reflect.get(t, k);
          return typeof x === 'function' ? x.bind(t.map((e, i) => guard(e, `${where}[${i}]`))) : x;
        }
        if (Array.isArray(t) && /^\d+$/.test(k)) return guard(t[k], `${where}[${k}]`);
        if (!(k in t)) throw new TypeError(`${where}.${String(k)}: 決まりに無い`);
        return guard(t[k], `${where}.${k}`);
      },
      set() {
        throw new TypeError(`${where}: 部品は props を書き換えない`);
      },
    });
  }

  // values: { 部品の名前: { props の値 } }(ホストが満たす部品だけ)。send(部品, callback, 引数の配列) はホストへ頼む
  // → parts: parts.Tabs.items のように読み、parts.Tabs.call('activate', 3) のように頼む
  function make(values, send) {
    const out = {};
    for (const [name, vals] of Object.entries(values)) {
      const p = PARTS[name];
      if (!p) throw new TypeError(`${name}: 決まりに無い部品`);
      check(name, vals, name); // 部品の props は、部品の名前の構造体として確かめる(下)
      const props = {};
      for (const k of Object.keys(p.props)) props[k] = vals[k];
      props.call = (cb, ...args) => {
        const sig = p.callbacks[cb];
        if (!sig) throw new TypeError(`${name}.${cb}: 決まりに無い callback`);
        if (args.length !== sig.length) throw new TypeError(`${name}.${cb}: 引数は ${sig.length} 個`);
        sig.forEach((t, i) => check(t, args[i], `${name}.${cb} の引数 ${i + 1}`));
        send(name, cb, args);
      };
      out[name] = guard(props, name);
    }
    return new Proxy(out, {
      get(t, k) {
        if (typeof k === 'symbol') return t[k];
        if (!PARTS[k]) throw new TypeError(`${String(k)}: 決まりに無い部品`);
        return t[k]; // 決まりにあってホストが満たしていない部品は undefined(使わないサイドバーもある)
      },
    });
  }

  // 部品の props も、部品の名前の構造体として確かめられるように
  for (const [name, p] of Object.entries(PARTS)) STRUCTS[name] = p.props;

  return { STRUCTS, PARTS, make, check };
});
