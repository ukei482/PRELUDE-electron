'use strict';
// セッションバスへの接続と、PRELUDE自身のD-Busサービス(org.prelude.Shell)。
// KWinスクリプトからの報告など「呼ばれる側」になる連絡は、すべてここで受ける。
const dbus = require('dbus-next');

const { instanceTag } = require('./util');

const IFACE = 'org.prelude.Shell';
// サービス名。別プロファイルの PRELUDE(デバッグ用)は印を付けた別名を取り、シェルとして動いている PRELUDE と並んで動ける
// (KWin スクリプトの報告も、それぞれ自分の名前に送る)。名前の要素は数字で始められないので d を前に付ける
const NAME = instanceTag ? `${IFACE}.d${instanceTag.slice(1)}` : IFACE;
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
    // 切断した後に届いたメソッド呼び出しへ dbus-next が返事を送ろうとすると、例外(stream is closed)が投げられて
    // 誰にも捕まらない。終了処理中にこれが起きると Electron のエラーダイアログで止まり、PRELUDE が終われなくなるので、切断後の送信は捨てる
    const send = bus.send.bind(bus);
    const b = bus;
    bus.send = (msg) => { if (b.closed) return; try { send(msg); } catch (e) { console.error('session bus send failed', e.message); } };
    bus.export(PATH, new ShellInterface(IFACE));
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
function shutdown() {
  if (bus) bus.closed = true;
  try { bus?.disconnect(); } catch {}
  bus = null;
}

module.exports = { NAME, PATH, init, onReport, get, shutdown };
