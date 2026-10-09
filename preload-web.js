'use strict';
// Web ペインの準備スクリプト(ページとは別の世界で動き、ページからは見えない)。
// 修飾キー付きのリンクのクリックだけを PRELUDE に知らせる。Alt+クリック: Chromium では「リンク先を保存」になるが、
// PRELUDE では設定「Alt+クリックしたリンク」に従う(既定は分割。Arc と同じ)
const { ipcRenderer } = require('electron');

window.addEventListener('click', (e) => {
  if (!e.altKey || e.button !== 0 || e.ctrlKey || e.metaKey || e.shiftKey) return;
  const a = e.target instanceof Element ? e.target.closest('a[href]') : null;
  if (!a || !/^https?:/i.test(a.href)) return;
  e.preventDefault();
  e.stopImmediatePropagation();
  ipcRenderer.send('web:link', a.href, 'alt');
}, true);
