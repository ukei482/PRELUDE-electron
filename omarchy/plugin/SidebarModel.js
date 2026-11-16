// Omarchy のサイドバー(Sidebar.qml)の、コアの様子 → 画面の行。QML を使わないので node のテストで確かめる(test/omarchy-sidebar.test.js)。
// 中身の決め方は proto/web/glue.js の values() と同じ(docs/redesign-rust.md の「サイドバー」の部品)。
.pragma library

// 配置(TabInfo.layout)のペインを左上から順に
function leaves(l, out) {
  out = out || []
  if (!l || typeof l !== "object") return out
  if (l.k === "split") {
    leaves(l.a, out)
    leaves(l.b, out)
  } else if (typeof l.id === "number") out.push(l)
  return out
}

function base(p) {
  return String(p || "").replace(/\/+$/, "").split("/").pop()
}

// ペインの名前: 窓の題名 → URL のホスト・アプリの名前・フォルダの名前 → 種類
function paneTitle(l) {
  var kind = l.type || ""
  var place = ""
  if (kind === "web") {
    var u = String(l.url || "")
    var i = u.indexOf("://")
    place = (i >= 0 ? u.slice(i + 3) : u).split("/")[0]
  } else if (kind === "app") place = l.name || ""
  else place = base(l.path)
  return l.window_title || l.title || place || kind
}

// 種類の短い印(アイコンの代わり。細い列でも出す)
var KIND_MARK = { web: "W", files: "F", console: ">", cli: ">", agent: "AI", app: "A", settings: "S" }
function kindMark(kind) {
  return KIND_MARK[kind] || (kind ? String(kind).charAt(0).toUpperCase() : "·")
}

// タブの行。分割したタブは、その下にペインの行を付ける
function tabRows(state) {
  var st = state || {}
  var out = []
  var tabs = st.tabs || []
  for (var i = 0; i < tabs.length; i++) {
    var t = tabs[i]
    var ls = leaves(t.layout)
    var cur = null
    for (var j = 0; j < ls.length; j++) if (ls[j].id === t.active_pane) cur = ls[j]
    if (!cur) cur = ls[0] || {}
    out.push({
      row: "tab",
      id: t.id,
      num: t.num,
      title: t.title || paneTitle(cur),
      kind: cur.type || "",
      mark: kindMark(cur.type),
      active: t.id === st.active,
      bookmark: t.bookmark || ""
    })
    if (ls.length > 1) {
      for (var k = 0; k < ls.length; k++) {
        out.push({
          row: "pane",
          id: ls[k].id,
          tab: t.id,
          title: paneTitle(ls[k]),
          kind: ls[k].type || "",
          mark: kindMark(ls[k].type),
          active: t.id === st.active && ls[k].id === t.active_pane
        })
      }
    }
  }
  return out
}

// ブックマークの行。フォルダの見出しの下に中身(閉じたフォルダは中身を出さない)。フォルダに入っていないものが先
// bookmark.list の folder はスペースの中の名前、folder.list の path はフォルダのパス
function bookmarkRows(state, bookmarks, folders, expanded) {
  var st = state || {}
  var open = {}
  var tabs = st.tabs || []
  for (var i = 0; i < tabs.length; i++) if (tabs[i].bookmark) open[tabs[i].bookmark] = true
  var exp = expanded || {}
  var dir = st.space_dir || ""
  var bms = bookmarks || []
  var row = function(b, depth) {
    return { row: "bookmark", path: b.path, title: b.title || base(b.path), num: b.num || 0, kind: b.kind || "", mark: kindMark(b.kind), open: !!open[b.path], depth: depth }
  }
  var out = []
  for (var a = 0; a < bms.length; a++) if (!bms[a].folder) out.push(row(bms[a], 0))
  var fs = folders || []
  for (var f = 0; f < fs.length; f++) {
    var fd = fs[f]
    var inside = []
    for (var c = 0; c < bms.length; c++) {
      var b = bms[c]
      if (b.folder && (dir + "/" + b.folder === fd.path || b.folder === fd.title)) inside.push(b)
    }
    var hasOpen = false
    for (var h = 0; h < inside.length; h++) if (open[inside[h].path]) hasOpen = true
    out.push({ row: "folder", path: fd.path, title: fd.title || base(fd.path), expanded: !!exp[fd.path], hasOpen: hasOpen, count: inside.length })
    if (exp[fd.path]) for (var d = 0; d < inside.length; d++) out.push(row(inside[d], 1))
  }
  return out
}

// 新しいタブで開ける種類(Web はランチャーから)
function newKinds(recipes) {
  var out = [{ kind: "web", name: "Web" }]
  var rs = recipes || []
  for (var i = 0; i < rs.length; i++) if (rs[i].type !== "settings") out.push({ kind: rs[i].type, name: rs[i].name || rs[i].type })
  return out
}

// `prelude raw` の返事を読む(壊れていれば fallback)
function parse(text, fallback) {
  try {
    var v = JSON.parse(String(text || ""))
    return v === null || v === undefined ? fallback : v
  } catch (e) {
    return fallback
  }
}

// 幅の設定を読む(数でなければ既定、細すぎ・広すぎは丸める)
function width(value, fallback, min, max) {
  var n = Number(value)
  if (!isFinite(n) || n <= 0) n = fallback
  return Math.round(Math.max(min, Math.min(max, n)))
}
