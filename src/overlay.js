'use strict';
// オーバーレイ窓のページ。通知のトースト一覧を描いて、内容の高さを main へ知らせる(0なら窓を隠す)。
const api = window.overlayApi;
const kind = new URLSearchParams(location.search).get('kind');
const root = document.getElementById('toasts');
const osdRoot = document.getElementById('osd');

function el(tag, cls, text) {
  const e = document.createElement(tag);
  if (cls) e.className = cls;
  if (text != null) e.textContent = text; // 通知の文字は textContent だけで描く(HTMLとして解釈しない)
  return e;
}

function render(list) {
  root.replaceChildren(...list.map((n) => {
    const t = el('div', 'toast' + (n.urgency >= 2 ? ' critical' : ''));
    // クリックで既定のアクション(あれば)を実行して閉じる。無ければ閉じるだけ
    t.addEventListener('click', () => {
      if (n.actions.some((a) => a.key === 'default')) api.action(n.id, 'default'); else api.dismiss(n.id);
    });
    if (n.icon) { const i = el('img'); i.src = n.icon; t.append(i); }
    const tx = el('div', 'tx');
    if (n.app) tx.append(el('div', 'app', n.app));
    tx.append(el('div', 'sum', n.summary));
    if (n.body) tx.append(el('div', 'body', n.body));
    const acts = n.actions.filter((a) => a.key !== 'default' && a.label);
    if (acts.length) {
      const row = el('div', 'acts');
      for (const a of acts) {
        const b = el('button', null, a.label);
        b.addEventListener('click', (e) => { e.stopPropagation(); api.action(n.id, a.key); });
        row.append(b);
      }
      tx.append(row);
    }
    const x = el('button', 'x', '×');
    x.title = '閉じる';
    x.addEventListener('click', (e) => { e.stopPropagation(); api.dismiss(n.id); });
    t.append(tx, x);
    return t;
  }));
  api.size(kind, list.length ? Math.ceil(root.getBoundingClientRect().height) + 4 : 0);
}

// OSD(音量・明るさ)。null が来たら隠す
function renderOsd(p) {
  if (!p) { osdRoot.style.display = 'none'; osdRoot.replaceChildren(); api.size(kind, 0); return; }
  const label = p.kind === 'volume' ? (p.muted ? 'ミュート' : '音量') : '明るさ';
  const card = el('div', 'osdcard' + (p.muted ? ' muted' : ''));
  const row = el('div', 'row');
  row.append(el('span', 'lbl', label), el('span', 'val', p.muted ? '' : `${p.percent}%`));
  const fill = el('div', 'fill');
  fill.style.width = `${Math.max(0, Math.min(100, p.muted ? 0 : p.percent))}%`;
  const track = el('div', 'track');
  track.append(fill);
  card.append(row, track);
  if (p.note) card.append(el('div', 'note', p.note));
  osdRoot.replaceChildren(card);
  osdRoot.style.display = 'block';
  api.size(kind, Math.ceil(osdRoot.getBoundingClientRect().height) + 4);
}

if (kind === 'osd') { root.style.display = 'none'; api.onOsd(renderOsd); }

api.theme().then((a) => {
  const r = document.documentElement.style;
  for (const k of ['bg', 'panel', 'line', 'text', 'dim', 'accent', 'danger']) if (a[k]) r.setProperty('--' + k, a[k]);
  if (a.fontSize) r.setProperty('--fs', a.fontSize + 'px');
}).finally(() => { if (kind === 'notify') api.toasts().then(render); });
if (kind === 'notify') api.onToasts(render);
