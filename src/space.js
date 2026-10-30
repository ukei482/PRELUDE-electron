'use strict';
// スペース(計算だけの部分。DOM・IPC には触らない)。
// スペースは「スペースのフォルダ」(既定は ~/スペース)の下のフォルダ1つ。中の .prelude がそのスペースのブックマークで、
// 中のサブフォルダ(1段だけ)がブックマークのフォルダになる。タブはスペースごとに持ち、切り替えると今のタブは全部閉じる。
// 画面では app.js より先に読み込まれ、関数をそのまま共通の名前として出す。テスト(test/space.test.js)では require で読む。
(function (root) {
  const SPACE_DEFAULT = 'Main';

  const byName = (a, b) => String(a).localeCompare(String(b), 'ja', { numeric: true, sensitivity: 'base' });

  // ブックマークの並び。開いているもの(タブの順)が先、残りは名前順。フォルダはフォルダどうしで名前順にして、ブックマークの後ろに置く。
  // bookmarks: { id, kind, title, folder }(folder はフォルダの id か '')、tabIds: タブの順に並べた、タブに結びついたブックマークの id
  function spaceSort(bookmarks, tabIds) {
    const pos = new Map();
    tabIds.forEach((id, i) => { if (id && !pos.has(id)) pos.set(id, i); });
    const cmp = (a, b) => {
      const pa = pos.has(a.id) ? pos.get(a.id) : Infinity;
      const pb = pos.has(b.id) ? pos.get(b.id) : Infinity;
      return pa !== pb ? pa - pb : byName(a.title, b.title) || byName(a.id, b.id);
    };
    const marks = bookmarks.filter((b) => b.kind !== 'folder');
    const folders = bookmarks.filter((b) => b.kind === 'folder').sort((a, b) => byName(a.title, b.title));
    const out = marks.filter((b) => !b.folder).sort(cmp);
    for (const f of folders) out.push(f, ...marks.filter((b) => b.folder === f.id).sort(cmp));
    return out;
  }

  // 0.10.0 までのサイドバーのブックマーク(設定の bookmarks)→ スペースへ書き出すもの。
  // toRoot は bmfile.js の bmFileRoot。知らない種類(Rust版のものなど)は rest に残す
  function spaceMigrate(list, toRoot) {
    const src = Array.isArray(list) ? list : [];
    const folders = new Map(src.filter((b) => b && b.kind === 'folder' && b.id).map((b) => [b.id, String(b.title || 'Folder')]));
    const items = [];
    const rest = [];
    for (const b of src) {
      if (!b || b.kind === 'folder') continue;
      const kind = b.kind;
      const target = kind === 'web' ? b.url : kind === 'files' ? b.path : kind === 'app' ? b.webUrl || b.cmd : '';
      const r = ['web', 'files', 'app'].includes(kind) && target ? toRoot({ ...b, target }) : kind === 'tab' && b.root ? toRoot(b) : null;
      if (!r) { rest.push(b); continue; }
      items.push({ id: b.id, folder: folders.get(b.folder) || '', title: String(b.title || target || 'Bookmark'), root: r });
    }
    return { folders: [...new Set(folders.values())], items, rest };
  }

  // 保存したタブ(workspace.json)の形をそろえる。0.10.0 までの { active, tabs } は既定のスペースのタブとして読む。
  // 戻り値: { space: 今のスペース, spaces: { 名前: { active, tabs } } }
  function spaceWsLoad(ws, fallback = SPACE_DEFAULT) {
    const spaces = {};
    if (ws && ws.spaces && typeof ws.spaces === 'object') {
      for (const [k, v] of Object.entries(ws.spaces)) if (v && Array.isArray(v.tabs)) spaces[k] = { active: v.active | 0, tabs: v.tabs };
    }
    const space = ws && typeof ws.space === 'string' && ws.space ? ws.space : fallback;
    if (ws && Array.isArray(ws.tabs)) spaces[space] = { active: ws.active | 0, tabs: ws.tabs };
    return { space, spaces };
  }

  // 保存する形。今のスペースのタブは上に(古い版で開いても今のタブが戻る)、ほかのスペースは spaces に置く
  function spaceWsSave(space, cur, spaces) {
    const others = {};
    for (const [k, v] of Object.entries(spaces || {})) if (k !== space) others[k] = v;
    return { space, active: cur.active, tabs: cur.tabs, spaces: others };
  }

  // 前のスペースがもう無いとき(フォルダを消した・名前を変えた)に開くスペース
  const spacePick = (want, names) => (names.includes(want) ? want : names.includes(SPACE_DEFAULT) ? SPACE_DEFAULT : names[0] || SPACE_DEFAULT);

  const exported = { SPACE_DEFAULT, spaceSort, spaceMigrate, spaceWsLoad, spaceWsSave, spacePick };
  if (typeof module === 'object' && module.exports) module.exports = exported;
  else Object.assign(root, exported);
})(this);
