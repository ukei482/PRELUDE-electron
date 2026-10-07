'use strict';
// セッションバスへの接続と、PRELUDE自身のD-Busサービス(org.prelude.Shell)。
// KWinスクリプトからの報告など「呼ばれる側」になる連絡は、すべてここで受ける。
const dbus = require('dbus-next');

const NAME = 'org.prelude.Shell';
const PATH = '/org/prelude/Shell';

let bus = null;
const handlers = new Map(); // メソッド名 -> (arg) => void

// KWinスクリプトの callDBus('org.prelude.Shell', PATH, 'org.prelude.Shell', 'Report', kind, json) を受ける
class ShellInterface extends dbus.interface.Interface {
  Report(kind, json) {
    const h = handlers.get(kind);
    if (!h) return;
    let data;
    try { data = JSON.parse(json); } catch { return; }
    try { h(data); } catch (e) { console.error('bus handler failed', kind, e); }
  }
}
ShellInterface.configureMembers({
  methods: { Report: { inSignature: 'ss', outSignature: '' } },
});

// 失敗しても例外は出さず false を返す(バスが無い環境ではシェル機能なしで動く)
async function init() {
  if (bus) return true;
  try {
    bus = dbus.sessionBus();
    bus.on('error', (e) => console.error('session bus error', e.message));
    bus.export(PATH, new ShellInterface(NAME));
    const reply = await bus.requestName(NAME, 0);
    if (reply !== dbus.RequestNameReply.PRIMARY_OWNER) throw new Error('name already owned');
    return true;
  } catch (e) {
    console.error('D-Bus unavailable:', e.message);
    try { bus?.disconnect(); } catch {}
    bus = null;
    return false;
  }
}

// kind ごとの受信処理を登録する(kind は Report の第1引数)
const onReport = (kind, cb) => { handlers.set(kind, cb); };

const get = () => bus;
function shutdown() { try { bus?.disconnect(); } catch {} bus = null; }

module.exports = { NAME, PATH, init, onReport, get, shutdown };
