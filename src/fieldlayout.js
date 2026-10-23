'use strict';
// フィールド表示のカードの配置(field.js から切り出した計算だけの部分。DOM・設定・描画には触らない)。
// カードは { x, y, w, h, placed, parent, row, kids } を持つ。cards: 置いてあるかを問わない、フィールドのカードすべて。
// 画面では field.js より先に読み込まれ、関数をそのまま共通の名前として出す。テスト(test/fieldlayout.test.js)では require で読む。
//
// 新しいカードは、親のカードのそのフォルダの行に見出しをそろえた位置に置く。ほかのカードと重なる時は、一番近い空き(上か下)に置く。
// 兄弟の枝どうし(各段で)は、親のカードの行の順に上から並べる。
// 置いたカードはその後動かさない(開く・閉じるで、ほかのカードの場所が変わらず、場所で覚えられる)。
// ただし兄弟の枝の間に新しいカードの入る空きが無い時と、カードが伸びて下の兄弟の枝にかかった時は、下の兄弟の枝を足りない分だけ下げる。
// Ctrl+↑↓ で中身を替えたカードは、新しく開いたのと同じ決め方で置き直す。
// カードの大きさが変わって(表示の設定・読み直し)、それでもほかのカードと重なった時だけ、重なった相手を近くの空きへ移す。
(function (root) {
  // GAP: 親と子のカードの横の間。HGAP / VGAP: 重ならないとみなすのに要る、横・縦の空き
  const FIELD_GAPS = { GAP: 90, HGAP: 24, VGAP: 18 };
  const { GAP, HGAP, VGAP } = FIELD_GAPS;
  const clampY = (v, lo, hi) => Math.min(hi, Math.max(lo, v));

  // a を (ax, ay) に置いた時に b と重なるか(空きを含めて)
  const fieldHits = (a, ax, ay, b) => ax < b.x + b.w + HGAP && b.x < ax + a.w + HGAP && ay < b.y + b.h + VGAP && b.y < ay + a.h + VGAP;

  // x の位置で、y に一番近い、ほかのカードと重ならない y。lo / hi: カードの上端・下端が入るべき範囲(省略は制限なし)。範囲の中に空きが無ければ null
  function fieldFreeY(cards, c, x, y, lo = -Infinity, hi = Infinity) {
    const others = cards.filter((k) => k !== c && k.placed);
    const fits = (yy) => yy >= lo && yy + c.h <= hi && !others.some((k) => fieldHits(c, x, yy, k));
    const want = clampY(y, lo, hi - c.h);
    if (fits(want)) return want;
    const cand = [lo, hi - c.h];
    for (const k of others) cand.push(k.y + k.h + VGAP, k.y - c.h - VGAP);
    return cand.filter((yy) => Number.isFinite(yy) && fits(yy)).sort((a, b) => Math.abs(a - y) - Math.abs(b - y))[0] ?? null;
  }

  // 枝(カードとその子孫の、置いてあるカード)の上端と下端
  function fieldSpan(c) {
    let top = Infinity, bot = -Infinity;
    const walk = (q) => { if (q.placed) { top = Math.min(top, q.y); bot = Math.max(bot, q.y + q.h); } q.kids.forEach(walk); };
    walk(c);
    return { top, bot };
  }

  // 新しいカード c を置いてよい y の範囲。c と各祖先について、親の中で上の行の兄弟の枝より下(lo)、下の行の兄弟の枝より上(hi)。
  // after: 下の行の兄弟の枝のカード(空きが足りない時に押し下げる)
  function fieldOrderRange(c) {
    let lo = -Infinity, hi = Infinity;
    const after = [];
    for (let a = c; a.parent; a = a.parent) {
      for (const s of a.parent.kids) {
        if (s === a) continue;
        const sp = fieldSpan(s);
        if (sp.top === Infinity) continue;
        if (s.row < a.row) { lo = Math.max(lo, sp.bot + VGAP); continue; }
        hi = Math.min(hi, sp.top - VGAP);
        const walk = (q) => { if (q.placed) after.push(q); q.kids.forEach(walk); };
        walk(s);
      }
    }
    return { lo, hi, after };
  }

  const fieldPush = (cards, d) => { if (d > 0) for (const k of cards) k.y += d; };

  // c の y を決める。want: 親のカードの行に見出しをそろえた y。枝の上下の順(親のカードの行の順)を守れる範囲の中で、一番近い空きに置く。
  // 範囲に入りきらない時だけ、上の兄弟の枝のすぐ下に置き、下の兄弟の枝をまとめて足りない分だけ下げる
  function fieldOrderY(cards, c, want) {
    const { lo, hi, after } = fieldOrderRange(c);
    const y = fieldFreeY(cards, c, c.x, want, lo, hi);
    if (y != null) return y;
    const yy = Number.isFinite(lo) ? lo : Number.isFinite(hi) ? hi - c.h : want;
    fieldPush(after, yy + c.h - hi);
    return yy;
  }

  // 親 c.parent の右に、新しいカード c を置く(x と y を決めて placed にする)
  function fieldPlaceKid(cards, c, want) {
    const p = c.parent;
    c.x = p.x + p.w + GAP;
    c.y = fieldOrderY(cards, c, want);
    c.placed = true;
  }

  // c の大きさが変わった後。広がって子のカードにかかったら、その枝を右へずらす。
  // 下へ伸びて下の兄弟の枝(各段)にかかったら、その枝をまとめて下げる(上下の順を保つ)。それでも重なるカードは近くの空きへ移す
  function fieldSettleLayout(cards, c) {
    if (!c.placed) return;
    const moved = new Set([c]);
    const need = c.x + c.w + GAP;
    for (const k of c.kids) {
      if (!k.placed || k.x >= need) continue;
      const d = need - k.x;
      const walk = (q) => { q.x += d; moved.add(q); q.kids.forEach(walk); };
      walk(k);
    }
    const { hi, after } = fieldOrderRange(c);
    fieldPush(after, fieldSpan(c).bot - hi);
    for (const m of moved) for (const k of cards) {
      if (!k.placed || moved.has(k) || !fieldHits(m, m.x, m.y, k)) continue;
      k.y = fieldFreeY(cards, k, k.x, k.y) ?? k.y;
    }
  }

  const exported = { FIELD_GAPS, fieldHits, fieldFreeY, fieldSpan, fieldOrderRange, fieldPush, fieldOrderY, fieldPlaceKid, fieldSettleLayout };
  if (typeof module === 'object' && module.exports) module.exports = exported;
  else Object.assign(root, exported);
})(this);
