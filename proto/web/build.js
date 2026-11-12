#!/usr/bin/env node
// 見る用の 1 枚の HTML(preview.html)を作る: 台(stage.html)に、サイドバー(sidebar.html と CSS・JS を埋め込んだもの)を iframe の srcdoc で入れる。
// ファイル 1 つで開け、偽のコアで動く。  node proto/web/build.js
'use strict';
const fs = require('node:fs');
const path = require('node:path');
const D = __dirname;
const read = (f) => fs.readFileSync(path.join(D, f), 'utf8');

let side = read('sidebar.html')
  .replace(/<link rel="stylesheet" href="([^"]+)">/g, (_, f) => `<style>\n${read(f)}</style>`)
  .replace(/<script src="([^"]+)"><\/script>/g, (_, f) => `<script>\n${read(f).replace(/<\/script/gi, '<\\/script')}</script>`);
const srcdoc = side.replace(/&/g, '&amp;').replace(/"/g, '&quot;');
const stage = read('stage.html').replace('src="sidebar.html?fake"', `srcdoc="${srcdoc}"`);
if (stage === read('stage.html')) throw new Error('stage.html の iframe が見つからない');
fs.writeFileSync(path.join(D, 'preview.html'), stage);
console.log(`preview.html (${Math.round(stage.length / 1024)} KB)`);
