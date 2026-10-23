'use strict';
// フィールド表示のカードの配置(src/fieldlayout.js)のテスト。npm test で動かす
const test = require('node:test');
const assert = require('node:assert/strict');
const {
  FIELD_GAPS, fieldHits, fieldFreeY, fieldSpan, fieldOrderRange, fieldPush, fieldOrderY, fieldPlaceKid, fieldSettleLayout,
} = require('../src/fieldlayout.js');

const { GAP, HGAP, VGAP } = FIELD_GAPS;
const W = 320;

// カードを作る。parent を渡すと、その子として row の行につなぐ
function card({ parent = null, row = 0, x = 0, y = 0, w = W, h = 100, placed = true } = {}) {
  const c = { parent, row, x, y, w, h, placed, kids: [] };
  if (parent) parent.kids.push(c);
  return c;
}
// 親の右の列に置いた子(置いた状態)
const kid = (parent, row, y, h = 100) => card({ parent, row, x: parent.x + parent.w + GAP, y, h });
// 木をたどったカードの一覧(field.js の fieldCollect と同じ順)
function all(root) {
  const out = [];
  const walk = (c) => { out.push(c); c.kids.forEach(walk); };
  walk(root);
  return out;
}
// 置いてあるカードどうしが、空きを含めて重なっていない
function assertNoOverlap(cards) {
  const placed = cards.filter((k) => k.placed);
  for (const a of placed) for (const b of placed) {
    if (a !== b) assert.ok(!fieldHits(a, a.x, a.y, b), `重なっている: (${a.x},${a.y}) と (${b.x},${b.y})`);
  }
}
// 兄弟の枝が、親のカードの行の順に上から並んでいる
function assertSiblingOrder(parent) {
  const kids = parent.kids.filter((k) => k.placed).sort((a, b) => a.row - b.row);
  for (let i = 1; i < kids.length; i++) {
    assert.ok(fieldSpan(kids[i - 1]).bot + VGAP <= fieldSpan(kids[i]).top, `行 ${kids[i - 1].row} の枝が行 ${kids[i].row} の枝より下にある`);
  }
}

test('fieldHits: 空き(HGAP / VGAP)より近ければ重なりとみなす', () => {
  const a = { w: 100, h: 100 };
  const b = { x: 0, y: 0, w: 100, h: 100 };
  assert.equal(fieldHits(a, 0, 0, b), true);
  assert.equal(fieldHits(a, 0, 100 + VGAP - 1, b), true);
  assert.equal(fieldHits(a, 0, 100 + VGAP, b), false);
  assert.equal(fieldHits(a, 100 + HGAP - 1, 0, b), true);
  assert.equal(fieldHits(a, 100 + HGAP, 0, b), false);
  assert.equal(fieldHits(a, 0, -100 - VGAP, b), false);
});

test('fieldFreeY: 空いていれば望んだ y のまま', () => {
  const k = card({ y: 500 });
  const c = card({ placed: false });
  assert.equal(fieldFreeY([k, c], c, 0, 0), 0);
});

test('fieldFreeY: 重なる時は、上下のうち近い方の空きへ', () => {
  const k = card({ y: 0, h: 100 });
  const c = card({ h: 50, placed: false });
  // 下寄りに望めば下へ、上寄りなら上へ
  assert.equal(fieldFreeY([k, c], c, 0, 80), 100 + VGAP);
  assert.equal(fieldFreeY([k, c], c, 0, 10), -50 - VGAP);
});

test('fieldFreeY: 置いていないカードと自分自身は避けない', () => {
  const ghost = card({ y: 0, placed: false });
  const c = card({ y: 0 });
  assert.equal(fieldFreeY([ghost, c], c, 0, 0), 0);
});

test('fieldFreeY: 範囲 lo / hi の中に収め、空きが無ければ null', () => {
  const c = card({ h: 100, placed: false });
  assert.equal(fieldFreeY([c], c, 0, -500, 0, 1000), 0);
  assert.equal(fieldFreeY([c], c, 0, 5000, 0, 1000), 900);
  assert.equal(fieldFreeY([c], c, 0, 0, 0, 50), null);
});

test('fieldSpan: 置いてあるカードだけで、枝の上端と下端を出す', () => {
  const p = card();
  const a = kid(p, 1, -50, 100);
  kid(a, 0, 200, 300);
  card({ parent: a, row: 1, y: 9999, placed: false });
  assert.deepEqual(fieldSpan(a), { top: -50, bot: 500 });
  assert.deepEqual(fieldSpan(card({ placed: false })), { top: Infinity, bot: -Infinity });
});

test('fieldOrderRange: 上の行の兄弟の枝より下、下の行の兄弟の枝より上', () => {
  const p = card({ h: 500 });
  kid(p, 1, 0, 100);
  const later = kid(p, 8, 400, 100);
  const grand = kid(later, 0, 600, 100);
  const c = card({ parent: p, row: 4, placed: false });
  const r = fieldOrderRange(c);
  assert.equal(r.lo, 100 + VGAP);
  assert.equal(r.hi, 400 - VGAP);
  assert.deepEqual(new Set(r.after), new Set([later, grand]));
});

test('fieldOrderRange: 祖先の兄弟の枝も範囲に入れる', () => {
  const root = card({ h: 500 });
  const a = kid(root, 1, 0, 100);
  kid(root, 5, 300, 100); // a より下の行の、a の兄弟
  const c = card({ parent: a, row: 0, placed: false });
  assert.equal(fieldOrderRange(c).hi, 300 - VGAP);
});

test('fieldPush: 正の分だけ下げ、0 以下なら動かさない', () => {
  const a = card({ y: 10 }), b = card({ y: 20 });
  fieldPush([a, b], 5);
  assert.deepEqual([a.y, b.y], [15, 25]);
  fieldPush([a, b], -5);
  fieldPush([a, b], 0);
  assert.deepEqual([a.y, b.y], [15, 25]);
});

test('fieldPlaceKid: 親の右、親の行にそろえた y に置く', () => {
  const p = card({ x: 0, y: 0 });
  const c = card({ parent: p, row: 3, placed: false });
  fieldPlaceKid(all(p), c, 77);
  assert.equal(c.x, W + GAP);
  assert.equal(c.y, 77);
  assert.equal(c.placed, true);
});

test('fieldPlaceKid: 新しいカードを開いても、置いてあるカードは動かさない', () => {
  const p = card({ h: 500 });
  const a = kid(p, 1, 0, 100);
  const b = kid(p, 9, 600, 100);
  const c = card({ parent: p, row: 5, h: 100, placed: false });
  fieldPlaceKid(all(p), c, 300);
  assert.deepEqual([a.y, b.y, c.y], [0, 600, 300]);
  assertNoOverlap(all(p));
});

test('fieldPlaceKid: 後から開いた上の行のフォルダのカードは、下の行のカードより上に置く', () => {
  // 0.4.8 で直した: 一番近い空きに置くと、上の行のフォルダのカードが下に来ることがあった
  const p = card({ h: 500 });
  const lower = kid(p, 10, 100, 100);
  const c = card({ parent: p, row: 1, h: 400, placed: false });
  fieldPlaceKid(all(p), c, 20);
  assert.equal(lower.y, 100, '下の行のカードは動かさない');
  assertSiblingOrder(p);
  assertNoOverlap(all(p));
});

test('fieldPlaceKid: 兄弟の枝の間に空きが無い時だけ、下の兄弟の枝を足りない分だけ下げる', () => {
  const p = card({ h: 500 });
  const upper = kid(p, 1, 0, 100);
  const lower = kid(p, 9, 150, 100);
  const lowerKid = kid(lower, 0, 150, 100);
  const c = card({ parent: p, row: 5, h: 200, placed: false });
  fieldPlaceKid(all(p), c, 125);
  assert.equal(upper.y, 0, '上の枝は動かさない');
  assert.equal(c.y, 100 + VGAP, '上の兄弟の枝のすぐ下');
  assert.equal(lower.y, c.y + c.h + VGAP, '下の枝は足りない分だけ下がる');
  assert.equal(lowerKid.y, lower.y, '下の枝の子孫も一緒に下がる');
  assertSiblingOrder(p);
  assertNoOverlap(all(p));
});

test('fieldOrderY: Ctrl+↑↓ で中身を替えたカードを、行の順を守る場所へ置き直す', () => {
  const p = card({ h: 500 });
  kid(p, 2, 0, 100);
  kid(p, 6, 300, 100);
  // 行 8 のフォルダに替わったカード(場所は前の行のまま、行 6 の枝より上にいる)
  const c = kid(p, 8, 150, 100);
  c.y = fieldOrderY(all(p), c, 200);
  assertSiblingOrder(p);
  assertNoOverlap(all(p));
});

test('fieldSettleLayout: 置いていないカードには何もしない', () => {
  const p = card({ placed: false });
  const k = kid(p, 0, 0);
  fieldSettleLayout(all(p), p);
  assert.equal(k.x, W + GAP);
});

test('fieldSettleLayout: 横に広がって子にかかったら、子の枝を右へずらす', () => {
  const p = card({ h: 500 });
  const k = kid(p, 0, 0);
  const g = kid(k, 0, 0);
  p.w = W + 200;
  fieldSettleLayout(all(p), p);
  assert.equal(k.x, p.w + GAP);
  assert.equal(g.x, k.x + k.w + GAP, '孫も一緒にずれる');
  assertNoOverlap(all(p));
});

test('fieldSettleLayout: 縦に伸びて下の兄弟の枝にかかったら、その枝をまとめて下げる', () => {
  const p = card({ h: 500 });
  const a = kid(p, 1, 0, 100);
  const b = kid(p, 5, 200, 100);
  const bKid = kid(b, 0, 250, 100);
  a.h = 300;
  fieldSettleLayout(all(p), a);
  assert.equal(a.y, 0, '伸びたカードは動かない');
  assert.equal(b.y, 300 + VGAP);
  assert.equal(bKid.y, 250 + (b.y - 200), '下の枝の子孫も同じだけ下がる');
  assertSiblingOrder(p);
  assertNoOverlap(all(p));
});

test('fieldSettleLayout: 兄弟でないカードと重なったら、相手を近くの空きへ移す', () => {
  const p = card({ h: 500 });
  const a = kid(p, 1, 0, 100);
  // a の子孫でも兄弟の枝でもない、同じ列にある別の木のカード
  const other = card({ x: a.x, y: 200, h: 100 });
  const cards = [...all(p), other];
  a.h = 250;
  fieldSettleLayout(cards, a);
  assertNoOverlap(cards);
});

test('fieldSettleLayout: 縮んだだけなら、ほかのカードは動かさない', () => {
  const p = card({ h: 500 });
  const a = kid(p, 1, 0, 300);
  const b = kid(p, 5, 400, 100);
  a.h = 100;
  fieldSettleLayout(all(p), a);
  assert.equal(b.y, 400);
});
