'use strict';
// システムトレイ(バー右側のアイコン)。左クリックで開く/切替、右クリックでメニュー、中クリックで第2の操作。
// app.js より先に読み込まれる。bar / h / api / popup などは呼び出し時に共通のものを使う。

bar.tray = [];

function paintTray() {
  const box = bar.el.tray;
  if (!box) return;
  // Passive(使われていない)は出さない。Plasma と同じ扱い
  const shown = bar.tray.filter((t) => t.status !== 'Passive');
  box.style.display = shown.length ? '' : 'none';
  box.replaceChildren(...shown.map((t) => {
    const e = h('span', {
      class: 'trayicon' + (t.status === 'NeedsAttention' ? ' attn' : ''), title: [t.title, t.tooltip].filter((s, i, a) => s && a.indexOf(s) === i).join('\n'),
      onclick: (ev) => (t.itemIsMenu ? showTrayMenu(t, ev) : api.tray.activate(t.id, ev.screenX, ev.screenY)),
      onauxclick: (ev) => { if (ev.button === 1) api.tray.secondary(t.id, ev.screenX, ev.screenY); },
      oncontextmenu: (ev) => { ev.preventDefault(); showTrayMenu(t, ev); },
    }, t.icon ? h('img', { src: t.icon, alt: '' }) : h('span', { class: 'tfb' }, (t.title || '?').slice(0, 1)));
    return e;
  }));
}

// メニューの木 → 既存のポップアップ項目。サブメニューは、押すと同じ場所に子の一覧を開く
function trayItems(t, nodes, x, y) {
  const out = [];
  for (const n of nodes) {
    if (!n.visible) continue;
    if (n.type === 'separator') { out.push({ sep: true }); continue; }
    const label = (n.toggle === true ? '✓ ' : n.toggle === false ? '　 ' : '') + n.label;
    if (n.children.length) out.push({ label: label + '  ▸', disabled: !n.enabled, run: () => popup(trayItems(t, n.children, x, y), x, y, { bottom: 28 }) });
    else out.push({ label, disabled: !n.enabled, run: () => api.tray.menuClick(t.id, n.id) });
  }
  return out;
}

async function showTrayMenu(t, ev) {
  const x = ev.clientX, y = ev.clientY;
  const tree = t.hasMenu ? await api.tray.menu(t.id) : null;
  if (!tree || !tree.length) { api.tray.activate(t.id, ev.screenX, ev.screenY); return; } // メニューが無いアイテムは通常の操作
  popup(trayItems(t, tree, x, y), x, y, { bottom: 28 });
}
