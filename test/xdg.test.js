'use strict';
const test = require('node:test');
const assert = require('node:assert');
const fs = require('fs');
const os = require('os');
const path = require('path');
const xdg = require('../sys/xdg');

test('XDG_DATA_DIRS: 順を保ち、重複と空を除く。無ければ決まりの既定', () => {
  assert.deepStrictEqual(xdg.dataDirs({ XDG_DATA_DIRS: '/a::/b:/a' }), ['/a', '/b']);
  assert.deepStrictEqual(xdg.dataDirs({}), ['/usr/local/share', '/usr/share']);
  assert.strictEqual(xdg.dataHome({}, '/home/u'), '/home/u/.local/share');
  assert.strictEqual(xdg.dataHome({ XDG_DATA_HOME: '/d' }, '/home/u'), '/d');
});

test('which: 名前の順 → PATH の順。実行できないファイルは飛ばす。extra も探す', () => {
  const t = fs.mkdtempSync(path.join(os.tmpdir(), 'prelude-xdg-'));
  const a = path.join(t, 'a'), b = path.join(t, 'b');
  fs.mkdirSync(a); fs.mkdirSync(b);
  const mk = (f, mode) => { fs.writeFileSync(f, ''); fs.chmodSync(f, mode); };
  mk(path.join(a, 'second'), 0o755);
  mk(path.join(b, 'first'), 0o755);
  mk(path.join(a, 'noexec'), 0o644);
  const env = { PATH: [a, b].join(path.delimiter) };
  try {
    assert.strictEqual(xdg.which(['first', 'second'], { env }), path.join(b, 'first'));
    assert.strictEqual(xdg.which(['noexec', 'second'], { env }), path.join(a, 'second'));
    assert.strictEqual(xdg.which('first', { env: {}, extra: [b] }), path.join(b, 'first'));
    assert.strictEqual(xdg.which(['missing'], { env }), null);
  } finally { fs.rmSync(t, { recursive: true, force: true }); }
});
