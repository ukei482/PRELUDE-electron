'use strict';
// PRELUDE コマンドの起動画面(cli/banner.js)のテスト。npm test で動かす
const test = require('node:test');
const assert = require('node:assert/strict');
const { bannerText, width } = require('../cli/banner.js');

const info = { version: '1.2.3', user: 'u', host: 'h', os: 'Kubuntu', kernel: '7.0', uptime: '1h 2m', shell: 'bash 5.3', term: 'xterm 80x24', cpu: 'cpu (8)', memory: '1.0 / 8.0 GiB' };

test('どの行も端末の幅に収まる', () => {
  for (const cols of [40, 60, 80, 120, 200]) {
    for (const line of bannerText(info, cols).split('\n')) assert.ok(width(line) <= cols, `${cols}: ${line}`);
  }
});

test('広ければロゴの右に PRELUDE と情報を並べる', () => {
  const lines = bannerText(info, 120).split('\n');
  assert.ok(lines.some((l) => l.startsWith('█████  ██') && l.includes('PRELUDE')));
  assert.ok(lines.some((l) => l.includes('v1.2.3')));
});

test('幅は色の指定を除き、全角を2と数える', () => {
  assert.equal(width('\x1b[7mab\x1b[0m'), 2);
  assert.equal(width('あa'), 3);
});
