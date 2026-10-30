'use strict';
// PRELUDE コマンド(src/cli.js)の行の読み取り。DOM を使わない計算だけなので、test/ から Node でも読み込める。
(function (root) {
  // 1行を空白で区切る。"..." と '...' で囲めば空白を含められる("..." の中は \ で次の1文字をそのまま入れる)
  function cliSplit(line) {
    const out = [];
    let cur = null, q = null;
    const s = String(line || '');
    for (let i = 0; i < s.length; i++) {
      const c = s[i];
      if (q) {
        if (c === q) q = null;
        else if (c === '\\' && q === '"' && i + 1 < s.length) cur += s[++i];
        else cur += c;
      } else if (c === '"' || c === "'") { q = c; cur ??= ''; }
      else if (/\s/.test(c)) { if (cur != null) out.push(cur); cur = null; }
      else cur = (cur ?? '') + c;
    }
    if (cur != null) out.push(cur);
    return out;
  }

  // パスを絶対パスにする。~ はホーム、相対パスはコマンドを打ったシェルの今のフォルダ(cwd)から。. と .. もたどる
  function cliPath(p, cwd, home) {
    p = String(p || '.');
    if (p === '~' || p.startsWith('~/')) p = home + p.slice(1);
    else if (!p.startsWith('/')) p = (cwd || home) + '/' + p;
    const parts = [];
    for (const seg of p.split('/')) {
      if (!seg || seg === '.') continue;
      if (seg === '..') parts.pop(); else parts.push(seg);
    }
    return '/' + parts.join('/');
  }

  // パスとして扱う引数か(/ ~ . で始まる)
  const cliLooksPath = (s) => /^(\/|~(\/|$)|\.\.?(\/|$))/.test(String(s || ''));

  const exported = { cliSplit, cliPath, cliLooksPath };
  if (typeof module === 'object' && module.exports) module.exports = exported;
  else Object.assign(root, exported);
})(this);
