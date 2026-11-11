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
});
chrome.tabs.onRemoved.addListener((tab) => send({ ev: 'removed', tab }));

chrome.downloads.onCreated.addListener((d) => send({ ev: 'download', id: d.id, url: d.url, filename: d.filename, state: d.state }));
chrome.downloads.onChanged.addListener(async (c) => {
  if (!c.state && !c.filename) return;
  const [d] = await chrome.downloads.search({ id: c.id });
  if (d) send({ ev: 'download', id: d.id, url: d.url, filename: d.filename, state: d.state });
});

connect();
