'use strict';
// ブックマークのファイル(.prelude)の読み書き(計算だけの部分。DOM・IPC には触らない)。
// ファイルは TOML: version = 2, title, [paths], [layout](docs/redesign-rust.md の判断 5)。layout はタブの配置(app.js の serNode の形)で、
// 1つのペインだけのブックマーク(Web・フォルダ・アプリ)も、ペイン1つの配置として書く。
// 文字列 ⇔ 中身はメインプロセス(bmcodec.js)が受け持ち、ここは中身(オブジェクト)だけを扱う。Rust 版(crates/prelude-format)も同じ形。
// 0.11 までの JSON の形 { prelude: 'bookmark', version: 1, title, paths, root } も読める(書き直すと TOML になる)。
// ホームの下のパスは「~/…」で書き、読むときに今の PC のホームへ戻す(別の PC へ持っていっても開ける)。
// paths(省略できる)はこの作業空間のフォルダ: files = ファイラの最初のフォルダ、open・save = アプリの「開く」「保存」の最初のフォルダ、
// download = ダウンロードの保存先、terminal = コンソール・アプリの作業フォルダ。項目は自由で、知らない項目も残す。
// 「~/…」のほか、相対パスは .prelude があるフォルダから数える。paths はファイルに書いたままの形で持ち、使うときに bmFilePaths で決める。
// ファイルは人からもらうこともあるので、読むときは形を確かめる。配置の知らない項目は捨てる。
// 配置・title・paths 以外の項目([ui.<部品名>] など)は rest として持ち、書き直すときにそのまま残す。
// アプリのペインの起動コマンドは、そのまま実行されると危ないので、bmFileCommands で取り出して開く側で確かめる。
// 画面では app.js より先に読み込まれ、関数をそのまま共通の名前として出す。テスト(test/bmfile.test.js)では require で読む。
(function (root) {
  const BMFILE_EXT = 'prelude';
  const TYPES = ['web', 'files', 'field', 'app', 'home', 'settings'];
  const VERSION = 2; // これより新しい版のファイルは読まない
  const PATH_KEYS = ['files', 'open', 'save', 'download', 'terminal']; // bmFilePaths で決める項目
  const PATH_FALLBACK = { files: null, download: 'save' }; // 無いときの代わりの項目(ほかは files)
  const MAX_PATHS = 64;
  const MAX_DEPTH = 8; // 分割の深さと、ペインの数の上限(壊れた・悪意のあるファイルで固まらないように)
  const MAX_LEAVES = 16;

  const str = (v, max = 4096) => (typeof v === 'string' ? v.slice(0, max) : '');
  const toHome = (p, home) => (home && p && (p === home || p.startsWith(home + '/')) ? '~' + p.slice(home.length) : p);
  const fromHome = (p, home) => (home && (p === '~' || p.startsWith('~/')) ? home + p.slice(1) : p);

  // パスを絶対パスにする。~ はホーム、相対パスは dir(.prelude があるフォルダ)から。. と .. はたどる
  function absPath(p, home, dir) {
    p = fromHome(p, home);
    if (!p.startsWith('/')) p = (dir || home || '') + '/' + p;
    const out = [];
    for (const s of p.split('/')) {
      if (!s || s === '.') continue;
      if (s === '..') out.pop(); else out.push(s);
    }
    return '/' + out.join('/');
  }

  // 書いたままの paths → 使うフォルダ { files, open, save, download, terminal }。
  // 項目ごとに ブックマーク → スペース(space = { paths, dir }。省略できる) → 代わりの項目 の順で決める。
  // 代わりの項目は download → save → files、open・save・terminal → files。相対パスは、それぞれのファイルがあるフォルダから。
  // どこにも無い項目は入れない(使う側の既定 = ホーム・ダウンロードのフォルダなどになる)
  function bmFilePaths(paths, home, dir, space) {
    const srcs = [[paths || {}, dir], space ? [space.paths || {}, space.dir] : null].filter(Boolean);
    const one = (k) => {
      for (const [p, d] of srcs) if (typeof p[k] === 'string' && p[k]) return absPath(p[k], home, d);
      const f = k in PATH_FALLBACK ? PATH_FALLBACK[k] : 'files';
      return f ? one(f) : undefined;
    };
    return Object.fromEntries(PATH_KEYS.map((k) => [k, one(k)]).filter(([, v]) => v));
  }

  // 配置の中のパスの無いフォルダ・フィールドのペインを、files のフォルダにした写し
  const bmFileFillPaths = (layout, files) => mapLeaves(layout, (l) => ((l.type === 'files' || l.type === 'field') && !l.path && files ? { ...l, path: files } : l));

  // サイドバーのブックマーク(app.js の S.bookmarks の1件)→ 配置
  function bmFileRoot(b) {
    if (b.kind === 'tab') return b.root;
    if (b.kind === 'web') return { k: 'leaf', type: 'web', url: b.target };
    if (b.kind === 'files') return { k: 'leaf', type: 'files', path: b.target };
    if (b.kind === 'app') return { k: 'leaf', type: 'app', cmd: b.cmd || '', cls: b.cls || '', name: b.title || '', webUrl: b.webUrl || '' };
    return null;
  }

  // 配置の各ペインに f をかけた写し
  const mapLeaves = (n, f) => (n.k === 'split' ? { ...n, a: mapLeaves(n.a, f), b: mapLeaves(n.b, f) } : f(n));
  const leavesOf = (n) => (n.k === 'split' ? [...leavesOf(n.a), ...leavesOf(n.b)] : [n]);

  // ファイルに書く中身(bmcodec.js が TOML にする)。paths と rest は書いたままの形(bmFileParse が返すもの)を渡す
  function bmFileData(title, layout, home, paths, rest) {
    const r = mapLeaves(layout, (l) => {
      const o = { k: 'leaf', type: l.type };
      for (const k of ['url', 'path', 'cmd', 'cls', 'name', 'webUrl', 'sort']) if (l[k]) o[k] = k === 'path' ? toHome(l[k], home) : l[k];
      return o;
    });
    const ps = {};
    for (const [k, v] of Object.entries(paths || {})) if (typeof v === 'string' && v) ps[k] = toHome(v, home);
    const d = { version: VERSION, title: String(title || '') };
    if (Object.keys(ps).length) d.paths = ps;
    return { ...d, layout: r, ...restOf(rest) };
  }
  // rest から、決まった項目と同じ名前のものを除く
  const restOf = (rest) => Object.fromEntries(Object.entries(rest && typeof rest === 'object' ? rest : {}).filter(([k]) => !['version', 'title', 'paths', 'layout', 'prelude', 'root'].includes(k)));

  // ファイルの中身(bmcodec.js が文字列から戻したもの)→ { title, paths, root, rest }。
  // paths は書いたままの形(無ければ {})。rest はそれ以外の項目。形がおかしければ Error(message は画面に出す)
  function bmFileParse(d, home) {
    if (!d || typeof d !== 'object') throw new Error('Not a bookmark file');
    const legacy = d.prelude === 'bookmark'; // 0.11 までの JSON
    const layout = legacy ? d.root : d.layout;
    if (!layout || typeof layout !== 'object' || (d.version != null && typeof d.version !== 'number')) throw new Error('Not a bookmark file');
    if (Number(d.version) > (legacy ? 1 : VERSION)) throw new Error('This file was made by a newer version of PRELUDE');
    let count = 0;
    const walk = (n, depth) => {
      if (!n || typeof n !== 'object' || depth > MAX_DEPTH) throw new Error('Layout is corrupted');
      if (n.k === 'split') return { k: 'split', dir: n.dir === 'col' ? 'col' : 'row', ratio: Number(n.ratio) || 0.5, a: walk(n.a, depth + 1), b: walk(n.b, depth + 1) };
      if (++count > MAX_LEAVES) throw new Error('Too many panes');
      const type = TYPES.includes(n.type) ? n.type : 'files';
      const l = { k: 'leaf', type };
      if (type === 'web') l.url = str(n.url);
      if (type === 'files' || type === 'field') l.path = fromHome(str(n.path), home);
      if (type === 'files' && n.sort && typeof n.sort === 'object') l.sort = { key: str(n.sort.key, 16), desc: !!n.sort.desc };
      if (type === 'app') Object.assign(l, { cmd: str(n.cmd), cls: str(n.cls, 256), name: str(n.name, 256), webUrl: str(n.webUrl) });
      return l;
    };
    const paths = {};
    if (d.paths && typeof d.paths === 'object') for (const [k, v] of Object.entries(d.paths).slice(0, MAX_PATHS)) if (str(v)) paths[k] = str(v);
    return { title: str(d.title, 256), paths, root: walk(layout, 0), rest: legacy ? {} : restOf(d) };
  }

  // 配置の中で起動されるコマンド(重なりなし)。実ブラウザのペイン(webUrl)は URL だけで、コマンドは PRELUDE が作るので含めない
  const bmFileCommands = (layout) => [...new Set(leavesOf(layout).filter((l) => l.type === 'app' && !l.webUrl && l.cmd && l.cmd.trim()).map((l) => l.cmd.trim()))];
  // コマンドを外した写し(起動しないで開く時)。only を渡すと、その中のコマンドだけを外す。アプリのペインは「アプリを取り込む」の画面になる
  const bmFileStripCommands = (layout, only) => mapLeaves(layout, (l) => (l.type === 'app' && !l.webUrl && (!only || only.includes(String(l.cmd || '').trim())) ? { ...l, cmd: '' } : l));

  // 配置 → サイドバーのブックマークの中身(id・番号・フォルダは呼ぶ側で付ける)。ペイン1つなら Web・フォルダ・アプリのブックマークにする
  function bmFileToBookmark(title, layout) {
    if (layout.k === 'leaf' && layout.type === 'web' && layout.url) return { kind: 'web', title, target: layout.url };
    if (layout.k === 'leaf' && layout.type === 'files' && layout.path) return { kind: 'files', title, target: layout.path };
    if (layout.k === 'leaf' && layout.type === 'app' && (layout.cmd || layout.webUrl)) {
      return { kind: 'app', title, target: layout.webUrl || layout.cmd, cmd: layout.webUrl ? '' : layout.cmd, cls: layout.webUrl ? '' : layout.cls || '', webUrl: layout.webUrl || '' };
    }
    return { kind: 'tab', title, root: layout };
  }

  // タイトル → ファイル名(拡張子つき)。/ などファイル名に使えない字は「-」にする
  function bmFileName(title) {
    const base = String(title || '').replace(/[/\\\0:*?"<>|\n\r\t]+/g, '-').replace(/^[.\s-]+|[\s-]+$/g, '').slice(0, 80).trim();
    return (base || 'ブックマーク') + '.' + BMFILE_EXT;
  }

  const exported = { BMFILE_EXT, bmFileRoot, bmFileData, bmFileParse, bmFilePaths, bmFileFillPaths, bmFileCommands, bmFileStripCommands, bmFileToBookmark, bmFileName };
  if (typeof module === 'object' && module.exports) module.exports = exported;
  else Object.assign(root, exported);
})(this);
