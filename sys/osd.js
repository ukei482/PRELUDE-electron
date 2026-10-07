'use strict';
// OSD(音量・明るさが変わったときに画面中央下へ小さく出す表示)。plasmashell が止まると KDE 標準の OSD が出なくなるので、代わりに出す。
// 音量は status.js の変化通知、明るさは powerdevil の brightnessChanged シグナルを見るので、キー操作でも他のアプリからの変更でも出る。
// 明るさの安全策: 0%(真っ暗)のまま一定時間たったら自動で戻す(画面が真っ暗で操作不能になるのを防ぐ)。
const bus = require('./bus');
const overlay = require('./overlay');
const quick = require('./quick');

const SHOW_MS = 1500;
const LOW = 0; // この%以下(四捨五入して0)を「真っ暗」とみなす。1〜2%は暗いだけで見えるので対象外
const LOW_WAIT = 8000;
const RESTORE_TO = 20;
const SCREEN = 'org.kde.Solid.PowerManagement';
const BR_PATH = '/org/kde/Solid/PowerManagement/Actions/BrightnessControl';
const BR_IFACE = 'org.kde.Solid.PowerManagement.Actions.BrightnessControl';

let hideTimer = null;
let quietUntil = 0;

function show(kind, percent, muted = false, note = '') {
  if (!overlay.supported || Date.now() < quietUntil) return;
  overlay.send('osd', 'osd:show', { kind, percent, muted, note });
  clearTimeout(hideTimer);
  hideTimer = setTimeout(() => overlay.send('osd', 'osd:show', null), note ? 4000 : SHOW_MS);
}

// PRELUDE自身のスライダー操作などで変わったときは、表示を抑える
const quiet = (ms = 1500) => { quietUntil = Date.now() + ms; };

// 暗すぎる状態が続いたら戻す。current() は今の%、restore() は戻す処理
function makeLowGuard({ current, restore, wait = LOW_WAIT, low = LOW }) {
  let t = null;
  return (percent) => {
    clearTimeout(t);
    t = null;
    if (percent > low) return;
    t = setTimeout(async () => {
      t = null;
      const p = await current();
      if (p != null && p <= low) await restore();
    }, wait);
  };
}

async function init() {
  const b = bus.get();
  if (!b) return;
  try {
    const obj = await b.getProxyObject(SCREEN, BR_PATH);
    const br = obj.getInterface(BR_IFACE);
    let max = Number(await br.brightnessMax()) || 0;
    br.on('brightnessMaxChanged', (v) => { max = Number(v) || max; });
    const pct = (v) => (max > 0 ? Math.round((Number(v) / max) * 100) : null);
    const guard = makeLowGuard({
      current: async () => pct(await br.brightness()),
      restore: async () => {
        await quick.doOp('brightness', RESTORE_TO);
        show('brightness', RESTORE_TO, false, '暗すぎたので明るさを戻しました');
      },
    });
    br.on('brightnessChanged', (v) => {
      const p = pct(v);
      if (p == null) return;
      show('brightness', p);
      guard(p);
    });
  } catch (e) { console.error('brightness watch failed:', e.message); }
}

// 緊急用: 明るさを50%に戻す(グローバルショートカットから)
const resetBrightness = async () => { await quick.doOp('brightness', 50); };

module.exports = { init, show, quiet, resetBrightness, makeLowGuard };
