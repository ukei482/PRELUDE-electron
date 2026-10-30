'use strict';
// ブックマークのファイル(.prelude)とスペースの設定(space.toml)の、文字列 ⇔ 中身(オブジェクト)。メインプロセスで使う。
// 中身の形は画面側(src/bmfile.js)が決める。Rust 版(crates/prelude-format)も同じ形を読み書きする。
// 書くのは TOML(docs/redesign-rust.md の判断 5)。0.11 までの JSON の .prelude も読める(書き直すと TOML になる)。
const TOML = require('smol-toml');

// 文字列 → 中身。読めなければ Error。TOML の表は普通のオブジェクトにする(smol-toml は prototype の無いオブジェクトを返す。日時は文字列になる)
function bmDecode(text) {
  const s = String(text);
  try { return s.trimStart().startsWith('{') ? JSON.parse(s) : JSON.parse(JSON.stringify(TOML.parse(s))); } catch { throw new Error('Not a bookmark file'); }
}

// 中身 → 文字列(TOML)。undefined・null の項目は書かない
function bmEncode(data) {
  return TOML.stringify(JSON.parse(JSON.stringify(data || {}, (_k, v) => (v === null ? undefined : v))));
}

module.exports = { bmDecode, bmEncode };
