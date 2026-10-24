'use strict';
// 通知センター(バーの「通知」をクリック)。履歴・おやすみモード・全消去。app.js より先に読み込まれる。

let notifUI = null;
let notifSeenAtOpen = 0;

function closeNotifications() {
  if (!notifUI) return;
  notifUI.bd.remove();
  notifUI.box.remove();
  notifUI = null;
  suspend(-1);
}

function fmtTime(t) {
  const d = new Date(t);
  const p = (n) => String(n).padStart(2, '0');
  const today = new Date();
  const day = d.toDateString() === today.toDateString() ? '' : `${d.getMonth() + 1}/${d.getDate()} `;
  return `${day}${p(d.getHours())}:${p(d.getMinutes())}`;
}

function paintNotifications() {
  if (!notifUI) return;
  const n = bar.notify;
  const kids = [
    h('div', { class: 'nhead' },
      h('span', { class: 'qgrow', style: { fontWeight: 600, textAlign: 'left', justifyContent: 'flex-start' } }, 'NOTIFICATIONS'),
      h('button', { class: 'textbtn' + (n.dnd ? ' primary' : ''), title: 'Hide non-critical popups (still kept in history)', onclick: () => api.notify.dnd(!n.dnd) }, 'DND'),
      h('button', { class: 'textbtn', onclick: () => api.notify.clear() }, 'Clear all')),
  ];
  if (!n.serving) kids.push(h('div', { class: 'qh' }, 'Notifications are still handled by another server (plasmashell). PRELUDE takes over once plasmashell stops.'));
  if (!n.history.length) kids.push(h('div', { class: 'empty' }, 'No notifications'));
  n.history.forEach((x, i) => kids.push(h('div', { class: 'nitem' + (i < notifSeenAtOpen ? ' new' : '') },
    h('div', { class: 'ntx' },
      h('div', { class: 'nmeta' }, `${x.app || 'Notification'} · ${fmtTime(x.time)}`),
      h('div', { class: 'nsum' }, x.summary),
      x.body ? h('div', { class: 'nbody' }, x.body) : null),
    h('button', { class: 'ibtn nx', title: 'Dismiss', onclick: () => api.notify.remove(x.id) }, ico('close')))));
  const top = notifUI.box.scrollTop;
  notifUI.box.replaceChildren(...kids);
  notifUI.box.scrollTop = top;
}

function openNotifications() {
  if (notifUI) { closeNotifications(); return; }
  const bd = h('div', { class: 'backdrop', onpointerdown: closeNotifications });
  const box = h('div', { class: 'qpanel' });
  overlay.append(bd, box);
  notifUI = { bd, box };
  suspend(+1);
  notifSeenAtOpen = bar.notify.unread; // 開く前の未読数 = 先頭から数えて「新着」の件数
  api.notify.seen();
  paintNotifications();
}
