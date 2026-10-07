'use strict';
// オーバーレイ窓のページ。通知のトースト一覧を描いて、内容の高さを main へ知らせる(0なら窓を隠す)。
const api = window.overlayApi;
const kind = new URLSearchParams(location.search).get('kind');
const root = document.getElementById('toasts');

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

api.theme().then((a) => {
  const r = document.documentElement.style;
  for (const k of ['bg', 'panel', 'line', 'text', 'dim', 'accent', 'danger']) if (a[k]) r.setProperty('--' + k, a[k]);
  if (a.fontSize) r.setProperty('--fs', a.fontSize + 'px');
}).finally(() => api.toasts().then(render));
api.onToasts(render);
