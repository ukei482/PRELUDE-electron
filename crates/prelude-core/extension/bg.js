// Web のペインとコアをつなぐ拡張機能(docs/redesign-rust.md の「Web のペイン」)。
// 事実を知らせて、頼まれたことをするだけ。どの窓を閉じてタブにするかは、コア(web.rs)が決める。
let port = null;

const send = (m) => {
  try {
    port?.postMessage(m);
  } catch {
    // つなぎ直す途中。次の知らせからまた届く
  }
};

const windowType = async (id) => {
  try {
    return (await chrome.windows.get(id)).type;
  } catch {
    return '';
  }
};

const report = async (t) =>
  send({ ev: 'tab', tab: t.id, url: t.pendingUrl || t.url || '', title: t.title || '', win: await windowType(t.windowId), opener: t.openerTabId ?? null });

const commands = {
  go: (m) => chrome.tabs.update(m.tab, { url: m.url }),
  back: (m) => chrome.tabs.goBack(m.tab),
  forward: (m) => chrome.tabs.goForward(m.tab),
  reload: (m) => chrome.tabs.reload(m.tab),
  close: (m) => chrome.tabs.remove(m.tab),
  mute: (m) => chrome.tabs.update(m.tab, { muted: !!m.on }),
};

// ファビコン: Chromium が覚えているもの(_favicon。permissions の favicon)を 32px の PNG で取り、base64 で送る。
// コアが保存して、ペインのアイコンにする。同じタブの同じページは 2 度送らない
const sentIcon = new Map();
const sendIcon = async (t) => {
  const url = t.url || '';
  if (!/^https?:/.test(url) || sentIcon.get(t.id) === url) return;
  sentIcon.set(t.id, url);
  try {
    const r = await fetch(chrome.runtime.getURL(`/_favicon/?pageUrl=${encodeURIComponent(url)}&size=32`));
    const bytes = new Uint8Array(await r.arrayBuffer());
    let bin = '';
    for (let i = 0; i < bytes.length; i += 0x8000) bin += String.fromCharCode(...bytes.subarray(i, i + 0x8000));
    send({ ev: 'icon', tab: t.id, url, data: btoa(bin) });
  } catch {
    sentIcon.delete(t.id);
  }
};

const onCommand = async (m) => {
  try {
    await commands[m.cmd]?.(m);
  } catch (e) {
    send({ ev: 'error', cmd: m.cmd, msg: String(e) });
  }
};

const connect = () => {
  port = chrome.runtime.connectNative('org.prelude.web');
  port.onMessage.addListener(onCommand);
  port.onDisconnect.addListener(() => {
    port = null;
    setTimeout(connect, 2000);
  });
  // 拡張機能より先に開いた窓も知らせる
  chrome.tabs.query({}).then((tabs) => tabs.forEach(report));
};

chrome.tabs.onCreated.addListener(report);
chrome.tabs.onUpdated.addListener((_id, change, tab) => {
  if (change.url || change.title) report(tab);
  if (change.favIconUrl || change.status === 'complete') sendIcon(tab);
});
chrome.tabs.onRemoved.addListener((tab) => {
  sentIcon.delete(tab);
  send({ ev: 'removed', tab });
});

chrome.downloads.onCreated.addListener((d) => send({ ev: 'download', id: d.id, url: d.url, filename: d.filename, state: d.state }));
chrome.downloads.onChanged.addListener(async (c) => {
  if (!c.state && !c.filename) return;
  const [d] = await chrome.downloads.search({ id: c.id });
  if (d) send({ ev: 'download', id: d.id, url: d.url, filename: d.filename, state: d.state });
});

connect();
