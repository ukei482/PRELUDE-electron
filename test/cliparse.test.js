'use strict';
// PRELUDE コマンドの行の読み取り(src/cliparse.js)のテスト。npm test で動かす
const test = require('node:test');
const assert = require('node:assert/strict');
const { cliSplit, cliPath, cliLooksPath } = require('../src/cliparse.js');

test('空白で区切り、引用符の中は1つにまとめる', () => {
  assert.deepEqual(cliSplit('  open   firefox '), ['open', 'firefox']);
  assert.deepEqual(cliSplit('folder "My Docs/a b"'), ['folder', 'My Docs/a b']);
  assert.deepEqual(cliSplit("web 'a \"b\"'"), ['web', 'a "b"']);
  assert.deepEqual(cliSplit('x "a\\"b" ""'), ['x', 'a"b', '']);
  assert.deepEqual(cliSplit(''), []);
});

test('パスはホームと今のフォルダから絶対パスにする', () => {
  assert.equal(cliPath('~', '/tmp', '/home/u'), '/home/u');
  assert.equal(cliPath('~/a/../b', '/tmp', '/home/u'), '/home/u/b');
  assert.equal(cliPath('.', '/tmp/x', '/home/u'), '/tmp/x');
  assert.equal(cliPath('../y', '/tmp/x', '/home/u'), '/tmp/y');
  assert.equal(cliPath('/etc/', '/tmp', '/home/u'), '/etc');
  assert.equal(cliPath('', '', '/home/u'), '/home/u');
  assert.equal(cliPath('../../..', '/a', '/home/u'), '/');
});

test('パスとして扱う引数', () => {
  for (const s of ['/', '~', '~/a', '.', '..', './a', '../a']) assert.ok(cliLooksPath(s), s);
  for (const s of ['firefox', '~user', '.bashrc', 'a/b', 'example.com']) assert.ok(!cliLooksPath(s), s);
});
