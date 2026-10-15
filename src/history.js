'use strict';
// 開いたファイル・入ったフォルダの履歴と、「最近＋よく使う」の点数(frecency)。
// 点数は時間とともに減る(半減期 FREQ.HALF)。開いた項目だけでなく、その上のフォルダにも少しずつ足すので、
// 「よく使うものが中にあるフォルダ」も点が高くなる。フィールド表示はこれで、カードの中の行き先になりそうな行に印を付け、先読みする。
// 記録はこの PC の中だけ(localStorage。PRELUDE の窓どうしで共有)。パスと点数・時刻のほかは残さない。
// app.js より先に読み込まれる。files.js・field.js から呼ばれる。

const FREQ = { HALF: 14 * 24 * 3600e3, MAX: 4000, UP: 0.6, FILE: 1, DIR: 0.5 };
let freqMap = null; // パス → [点数, 時刻(ms)]
let freqTimer = 0;

function freqStore() {
  if (!freqMap) {
    freqMap = new Map();
    try { for (const [p, s, t] of JSON.parse(localStorage.getItem('freqHistory') || '[]')) freqMap.set(p, [s, t]); } catch {}
  }
  return freqMap;
}
const freqDecay = (s, t, now) => s * Math.pow(0.5, (now - t) / FREQ.HALF);

function freqSave() {
  clearTimeout(freqTimer);
  freqTimer = setTimeout(() => {
    const now = Date.now();
    let rows = [...freqStore()].map(([p, [s, t]]) => [p, freqDecay(s, t, now), now]);
    if (rows.length > FREQ.MAX) { // 点の低いものから捨てる
      rows.sort((a, b) => b[1] - a[1]);
      rows = rows.slice(0, FREQ.MAX);
      freqMap = new Map(rows.map(([p, s, t]) => [p, [s, t]]));
    }
    try { localStorage.setItem('freqHistory', JSON.stringify(rows.map(([p, s, t]) => [p, +s.toFixed(4), t]))); } catch {}
  }, 1000);
}

// full を開いた(kind: 'file' | 'dir')。上のフォルダにも、遠いほど小さく足す(/ には足さない)
function freqRecord(full, kind) {
  if (!full) return;
  const m = freqStore(), now = Date.now();
  let w = kind === 'dir' ? FREQ.DIR : FREQ.FILE;
  for (let p = String(full); p && p !== '/'; p = p.replace(/\/+[^/]+\/*$/, '')) {
    const o = m.get(p);
    m.set(p, [(o ? freqDecay(o[0], o[1], now) : 0) + w, now]);
    w *= FREQ.UP;
    if (w < 0.01) break;
  }
  freqSave();
}

function freqScore(full, now = Date.now()) {
  const o = freqStore().get(full);
  return o ? freqDecay(o[0], o[1], now) : 0;
}

function freqClear() {
  freqMap = new Map();
  clearTimeout(freqTimer);
  try { localStorage.removeItem('freqHistory'); } catch {}
}
