// サイドバー(部品の決まり parts.js だけを使う、サイドバーの 1 つの作り)。
// 使う部品: SidebarFrame・Spaces・Tabs・NewTab・Bookmarks。PaneBar・BookmarkButton は使わない(情報を絞るため。使わない部品があってよい)。
// - render(parts, view): 部品の props と、サイドバーだけが持つ一時的な見た目(開いているメニュー・編集中の欄)→ HTML の文字列。
//   DOM を使わないので node のテストで描ける(決まりに無いものを読めばそこで止まる)
// - mount(el, host): 画面に付ける。クリック・右クリック・ドラッグ・キーを部品の callback にする
// 頼みごとはどれも data-call="部品.callback" と data-args(JSON の配列)で書き、parts の call を通す。
(function (root, factory) {
  const m = factory();
  if (typeof module === 'object' && module.exports) module.exports = m;
  else root.PreludeSidebar = m;
})(typeof self !== 'undefined' ? self : this, function () {
  'use strict';

  const esc = (s) => String(s).replace(/[&<>"']/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' })[c]);
  const args = (...a) => esc(JSON.stringify(a));
  const call = (fn, ...a) => `data-call="${fn}" data-args="${args(...a)}"`;

  const PATHS = {
    globe: '<circle cx="12" cy="12" r="9"/><path d="M3 12h18M12 3a14 14 0 0 1 0 18M12 3a14 14 0 0 0 0 18"/>',
    folder: '<path d="M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z"/>',
    terminal: '<rect x="3" y="4" width="18" height="16" rx="2"/><path d="M7 9l3 3-3 3M13 15h4"/>',
    sparkle: '<path d="M12 3l1.8 5.2L19 10l-5.2 1.8L12 17l-1.8-5.2L5 10l5.2-1.8zM18.5 16l.7 1.8 1.8.7-1.8.7-.7 1.8-.7-1.8-1.8-.7 1.8-.7z"/>',
    grid: '<rect x="4" y="4" width="6.5" height="6.5" rx="1.5"/><rect x="13.5" y="4" width="6.5" height="6.5" rx="1.5"/><rect x="4" y="13.5" width="6.5" height="6.5" rx="1.5"/><rect x="13.5" y="13.5" width="6.5" height="6.5" rx="1.5"/>',
    layout: '<rect x="3" y="4" width="18" height="16" rx="2"/><path d="M12 4v16M12 12h9"/>',
    splitRow: '<rect x="3" y="4" width="18" height="16" rx="2"/><path d="M12 4v16"/><path d="M16.5 10v4M14.5 12h4"/>',
    splitCol: '<rect x="3" y="4" width="18" height="16" rx="2"/><path d="M3 12h18"/><path d="M12 14.5v4M10 16.5h4"/>',
    x: '<path d="M7 7l10 10M17 7L7 17"/>',
    plus: '<path d="M12 5v14M5 12h14"/>',
    star: '<path d="M12 3.5l2.6 5.3 5.9.9-4.3 4.1 1 5.8L12 16.8l-5.2 2.8 1-5.8-4.3-4.1 5.9-.9z"/>',
    vol: '<path d="M4 9h4l5-4v14l-5-4H4z"/><path d="M16.5 9a4 4 0 0 1 0 6"/>',
    mute: '<path d="M4 9h4l5-4v14l-5-4H4z"/><path d="M16 9.5l5 5M21 9.5l-5 5"/>',
    chev: '<path d="M9 6l6 6-6 6"/>',
    reopen: '<path d="M4 12a8 8 0 1 0 2.4-5.7M4 4.5V9h4.5"/>',
    side: '<rect x="3" y="4" width="18" height="16" rx="2"/><path d="M9 4v16"/>',
    updown: '<path d="M8 9l4-4 4 4M8 15l4 4 4-4"/>',
    link: '<path d="M10 14a4 4 0 0 0 5.7 0l3-3a4 4 0 0 0-5.7-5.7l-1 1M14 10a4 4 0 0 0-5.7 0l-3 3a4 4 0 0 0 5.7 5.7l1-1"/>',
    trash: '<path d="M4 7h16M10 11v6M14 11v6M6 7l1 13h10l1-13M9 7V4h6v3"/>',
    copy: '<rect x="8" y="8" width="12" height="12" rx="2"/><path d="M16 8V6a2 2 0 0 0-2-2H6a2 2 0 0 0-2 2v8a2 2 0 0 0 2 2h2"/>',
    hash: '<path d="M5 9h14M5 15h14M10 4L8 20M16 4l-2 16"/>',
    pencil: '<path d="M4 20h4L19 9l-4-4L4 16z"/>',
    open: '<path d="M14 4h6v6M20 4l-9 9M18 14v5a1 1 0 0 1-1 1H5a1 1 0 0 1-1-1V7a1 1 0 0 1 1-1h5"/>',
    folderPlus: '<path d="M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z"/><path d="M12 10.5v5M9.5 13h5"/>',
    below: '<path d="M5 6h14M5 11h14M12 14v6M9 17l3 3 3-3"/>',
    save: '<path d="M6 4h12v16l-6-4-6 4z"/>',
  };
  const svg = (name) => `<svg class="i" viewBox="0 0 24 24" aria-hidden="true">${PATHS[name] || PATHS.grid}</svg>`;
  const KIND_ICON = { web: 'globe', files: 'folder', console: 'terminal', cli: 'terminal', agent: 'sparkle', app: 'grid', layout: 'layout' };
  const ico = (src, kind) => `<span class="ico">${src ? `<img src="${esc(src)}" alt="">` : svg(KIND_ICON[kind] || 'grid')}</span>`;

  function editBox(view, key, value) {
    if (!view.edit || view.edit.key !== key) return null;
    return `<input class="edit" data-edit-input="${esc(key)}" value="${esc(view.edit.value ?? value)}" spellcheck="false">`;
  }

  // 見出し: 上に開閉、その下にスペース
  function head(p) {
    const f = p.SidebarFrame;
    const sp = p.Spaces;
    return `<div class="top">
      <button class="iconbtn" ${call('SidebarFrame.toggleCompact')} title="${f.compact ? '広げる' : '細くする'}">${svg('side')}</button>
    </div>
    <div class="head">
      <button class="space" data-menu="space" title="スペース">
        <span class="dot${f.online ? '' : ' off'}"></span><span class="name">${esc(sp.current || '(スペース無し)')}</span>${svg('updown')}
      </button>
    </div>`;
  }

  function tabs(p, view) {
    const t = p.Tabs;
    const panesOf = (id) => t.panes.filter((x) => x.tab === id);
    const rows = t.items
      .map((tab) => {
        const edit = editBox(view, `renumber:${tab.id}`, String(tab.num));
        const own = panesOf(tab.id);
        return `<div class="row tab${tab.active ? ' active' : ''}" draggable="true" data-drag="tab:${tab.id}" data-drop="tab:${tab.id}"
            ${call('Tabs.activate', tab.id)} data-ctx="tab:${tab.id}" title="${esc(tab.title)}">
            <span class="num">${edit ? '' : tab.num}</span>${ico(tab.icon, tab.kind)}
            ${edit || `<span class="t">${esc(tab.title)}</span>`}
            ${tab.bookmark ? `<span class="sub" title="ブックマークから">${svg('save')}</span>` : ''}
            <button class="iconbtn x" ${call('Tabs.close', tab.id)} title="閉じる">${svg('x')}</button>
          </div>${own
            .map(
              (pn) => `<div class="row pane${pn.active ? ' active' : ''}" ${call('Tabs.focusPane', pn.id)} title="${esc(pn.title)}">
                ${ico(pn.icon, pn.kind)}<span class="t">${esc(pn.title)}</span></div>`,
            )
            .join('')}`;
      })
      .join('');
    return `<div class="sec"><span class="lbl">タブ</span>
        <button class="iconbtn" ${call('Tabs.reopen')} ${t.canReopen ? '' : 'disabled'} title="閉じたタブを開き直す">${svg('reopen')}</button>
        <button class="iconbtn" data-menu="newtab" title="新しいタブ">${svg('plus')}</button>
      </div>${rows || '<div class="row"><span class="t" style="color:var(--dim)">タブはありません</span></div>'}`;
  }

  function bookmarks(p, view) {
    const b = p.Bookmarks;
    const row = (bm, inner) => {
      const edit = editBox(view, `bm:${bm.path}`, bm.title);
      return `<div class="row bm${inner ? ' inner' : ''}" draggable="true" data-drag="bm:${esc(bm.path)}" data-drop="bm:${esc(bm.path)}"
          ${call('Bookmarks.open', bm.path)} data-ctx="bm:${esc(bm.path)}" title="${esc(bm.title)}">
          <span class="num">${bm.num || ''}</span>${ico(bm.icon, bm.kind)}
          ${edit || `<span class="t">${esc(bm.title)}</span>`}
          ${bm.open ? '<span class="opendot" title="開いている"></span>' : ''}
        </div>`;
    };
    const top = b.items.filter((x) => !x.folder).map((x) => row(x, false)).join('');
    const folders = b.folders
      .map((f) => {
        const edit = editBox(view, `folder:${f.path}`, f.title);
        const inside = f.expanded ? b.items.filter((x) => x.folder === f.path).map((x) => row(x, true)).join('') : '';
        return `<div class="row folder${f.expanded ? ' expanded' : ''}" data-drop="folder:${esc(f.path)}"
            ${call('Bookmarks.setExpanded', f.path, !f.expanded)} data-ctx="folder:${esc(f.path)}" title="${esc(f.title)}">
            <span class="num chev">${svg('chev')}</span>${ico('', 'files')}
            ${edit || `<span class="t">${esc(f.title)}</span>`}
            ${f.hasOpen && !f.expanded ? '<span class="opendot"></span>' : ''}
          </div>${inside}`;
      })
      .join('');
    const newFolder = editBox(view, 'folder-new', '');
    return `<div class="sec" data-drop="root:"><span class="lbl">ブックマーク</span>
        <button class="iconbtn" data-edit="folder-new" title="フォルダを作る">${svg('folderPlus')}</button>
      </div>${newFolder ? `<div class="row">${ico('', 'files')}${newFolder}</div>` : ''}${top}${folders}`;
  }

  function foot(p) {
    const f = p.SidebarFrame;
    const lv = ['起動だけ', '作業領域だけ', '並べる', '読む', '追う'][f.windowsLevel] || '';
    const text = f.online ? `窓: ${lv}` : f.status || 'コアにつながっていません';
    return `<div class="foot"><span class="dot${f.online ? '' : ' off'}"></span><span class="t" title="${esc(text)}">${esc(text)}</span></div>`;
  }

  function menu(p, view) {
    const m = view.menu;
    if (!m) return '';
    const item = (icon, label, attrs, cls = '') => `<button class="${cls}" ${attrs}>${svg(icon)}<span>${esc(label)}</span></button>`;
    let body = '';
    if (m.kind === 'space') {
      body = p.Spaces.all.map((s) => item(s === p.Spaces.current ? 'chev' : 'grid', s, call('Spaces.switchTo', s))).join('') + '<hr>' + item('plus', '新しいスペース', 'data-edit="space-new"');
      if (view.edit && view.edit.key === 'space-new') body += `<div style="padding:4px 6px">${editBox(view, 'space-new', '')}</div>`;
    } else if (m.kind === 'newtab') {
      body = '<div class="ttl">新しいタブ</div>' + p.NewTab.kinds.map((k) => item(KIND_ICON[k.kind] || 'grid', k.name, call('NewTab.open', k.kind))).join('');
    } else if (m.kind === 'tab') {
      const id = Number(m.target);
      body =
        item('copy', '複製', call('Tabs.duplicate', id)) +
        item('hash', '番号を変える', `data-edit="renumber:${id}"`) +
        '<hr>' +
        item('below', 'これより下を閉じる', call('Tabs.closeBelow', id)) +
        item('x', '閉じる', call('Tabs.close', id), 'danger');
    } else if (m.kind === 'bm') {
      const path = m.target;
      const folders = p.Bookmarks.folders;
      const bm = p.Bookmarks.items.find((x) => x.path === path);
      body =
        item('open', '開く', call('Bookmarks.open', path)) +
        item('pencil', '名前を変える', `data-edit="bm:${esc(path)}"`) +
        item('folder', '入っているフォルダを見る', call('Bookmarks.reveal', path)) +
        (folders.length || (bm && bm.folder) ? '<hr><div class="ttl">移す</div>' : '') +
        (bm && bm.folder ? item('chev', 'スペースの直下', call('Bookmarks.moveTo', path, '')) : '') +
        folders.filter((f) => !bm || f.path !== bm.folder).map((f) => item('chev', f.title, call('Bookmarks.moveTo', path, f.path))).join('') +
        '<hr>' +
        item('trash', 'ゴミ箱へ', call('Bookmarks.trash', path), 'danger');
    } else if (m.kind === 'folder') {
      const path = m.target;
      body =
        item('open', '中を全部開く', call('Bookmarks.openAll', path)) +
        item('pencil', '名前を変える', `data-edit="folder:${esc(path)}"`) +
        '<hr>' +
        item('trash', 'フォルダを消す(中身は直下へ)', call('Bookmarks.removeFolder', path), 'danger');
    }
    return `<div class="menu" style="left:${m.x}px;top:${m.y}px">${body}</div>`;
  }

  function render(p, view = {}) {
    const f = p.SidebarFrame;
    return `<div class="sb${f.compact ? ' compact' : ''}">
      ${head(p)}
      <div class="scroll">${tabs(p, view)}${bookmarks(p, view)}</div>
      ${foot(p)}
    </div>${menu(p, view)}`;
  }

  // 編集の欄で Enter → 部品の callback
  function submitEdit(p, key, value) {
    const v = value.trim();
    if (!v) return;
    const [kind, ...rest] = key.split(':');
    const target = rest.join(':');
    if (kind === 'bm') p.Bookmarks.call('rename', target, v);
    else if (kind === 'folder') p.Bookmarks.call('renameFolder', target, v);
    else if (kind === 'folder-new') p.Bookmarks.call('createFolder', v);
    else if (kind === 'space-new') p.Spaces.call('create', v);
    else if (kind === 'renumber' && /^\d+$/.test(v)) p.Tabs.call('renumber', Number(target), Number(v));
  }

  // host: { parts() → 今の parts、onChange(fn) → 変わったら fn }
  function mount(el, host) {
    const view = { menu: null, edit: null };
    let drag = null;
    const draw = () => {
      const scroll = el.querySelector('.scroll');
      const top = scroll ? scroll.scrollTop : 0;
      el.innerHTML = render(host.parts(), view);
      const s2 = el.querySelector('.scroll');
      if (s2) s2.scrollTop = top;
      const m = el.querySelector('.menu');
      if (m) {
        const r = m.getBoundingClientRect();
        if (r.right > innerWidth - 6) m.style.left = `${Math.max(6, innerWidth - r.width - 6)}px`;
        if (r.bottom > innerHeight - 6) m.style.top = `${Math.max(6, innerHeight - r.height - 6)}px`;
      }
      const inp = el.querySelector('[data-edit-input]');
      if (inp && document.activeElement !== inp) {
        inp.focus();
        inp.select();
      }
    };
    host.onChange(() => {
      if (!view.edit) draw(); // 編集中は描き直さない(打っている字が消えないように)
    });
    const close = () => {
      view.menu = null;
      view.edit = null;
      draw();
    };
    const openMenu = (kind, target, x, y) => {
      view.menu = { kind, target, x, y };
      view.edit = null;
      draw();
    };

    el.addEventListener('click', (e) => {
      const t = e.target;
      if (t.closest('[data-edit-input]')) return;
      const ed = t.closest('[data-edit]');
      if (ed) {
        e.stopPropagation();
        view.edit = { key: ed.dataset.edit, value: null };
        if (ed.dataset.edit !== 'space-new') view.menu = null;
        draw();
        return;
      }
      const mb = t.closest('[data-menu]');
      if (mb) {
        const r = mb.getBoundingClientRect();
        if (view.menu && view.menu.kind === mb.dataset.menu) return close();
        return openMenu(mb.dataset.menu, '', Math.min(r.left, innerWidth - 200), r.bottom + 4);
      }
      const c = t.closest('[data-call]');
      if (c && !c.disabled) {
        e.stopPropagation();
        const [part, cb] = c.dataset.call.split('.');
        const a = JSON.parse(c.dataset.args || '[]');
        view.menu = null;
        view.edit = null;
        host.parts()[part].call(cb, ...a);
        draw();
        return;
      }
      if (view.menu || view.edit) close();
    });
    el.addEventListener('contextmenu', (e) => {
      const r = e.target.closest('[data-ctx]');
      e.preventDefault();
      if (!r) return;
      const [kind, ...rest] = r.dataset.ctx.split(':');
      openMenu(kind, rest.join(':'), e.clientX, e.clientY);
    });
    el.addEventListener('keydown', (e) => {
      const inp = e.target.closest('[data-edit-input]');
      if (e.key === 'Escape') return close();
      if (inp && e.key === 'Enter' && !e.isComposing) {
        const key = inp.dataset.editInput;
        const v = inp.value;
        view.edit = null;
        view.menu = null;
        submitEdit(host.parts(), key, v);
        draw();
      }
    });
    el.addEventListener('input', (e) => {
      if (view.edit && e.target.closest('[data-edit-input]')) view.edit.value = e.target.value;
    });

    // ドラッグ: タブの並べ替え、ブックマークの並べ替え・フォルダへ移す
    const clearDrop = () => el.querySelectorAll('.drop-before,.drop-into,.dragging').forEach((n) => n.classList.remove('drop-before', 'drop-into', 'dragging'));
    const accepts = (d, target) => {
      if (!d || !target) return null;
      const [dk] = d.split(':');
      const [tk] = target.split(':');
      if (dk === 'tab' && tk === 'tab') return 'before';
      if (dk === 'bm' && tk === 'bm') return 'before';
      if (dk === 'bm' && (tk === 'folder' || tk === 'root')) return 'into';
      return null;
    };
    el.addEventListener('dragstart', (e) => {
      const r = e.target.closest('[data-drag]');
      if (!r) return;
      drag = r.dataset.drag;
      e.dataTransfer.effectAllowed = 'move';
      e.dataTransfer.setData('text/plain', drag);
      requestAnimationFrame(() => r.classList.add('dragging'));
    });
    el.addEventListener('dragover', (e) => {
      const r = e.target.closest('[data-drop]');
      const how = r && accepts(drag, r.dataset.drop);
      if (!how) return;
      e.preventDefault();
      clearDrop();
      r.classList.add(how === 'before' ? 'drop-before' : 'drop-into');
    });
    el.addEventListener('dragend', () => {
      drag = null;
      clearDrop();
    });
    el.addEventListener('drop', (e) => {
      const r = e.target.closest('[data-drop]');
      const d = drag;
      drag = null;
      clearDrop();
      if (!r || !accepts(d, r.dataset.drop)) return;
      e.preventDefault();
      const p = host.parts();
      const [dk, ...dr] = d.split(':');
      const [tk, ...tr] = r.dataset.drop.split(':');
      const from = dr.join(':');
      const to = tr.join(':');
      if (dk === 'tab') {
        const ids = p.Tabs.items.map((t) => t.id);
        const fi = ids.indexOf(Number(from));
        let ti = ids.indexOf(Number(to));
        if (fi < 0 || ti < 0 || fi === ti) return;
        if (fi < ti) ti -= 1; // 自分を抜いた後の位置
        p.Tabs.call('moveTo', Number(from), ti);
      } else if (tk === 'bm') {
        if (from === to) return;
        const items = p.Bookmarks.items;
        const a = items.find((x) => x.path === from);
        const b = items.find((x) => x.path === to);
        if (!a || !b) return;
        if (a.folder === b.folder) p.Bookmarks.call('reorder', from, to);
        else p.Bookmarks.call('moveTo', from, b.folder);
      } else {
        p.Bookmarks.call('moveTo', from, to);
      }
      draw();
    });
    draw();
    return { draw, view };
  }

  return { render, mount, submitEdit, PATHS };
});
