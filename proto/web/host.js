// 画面の中のホスト: コアの様子を持ち、つなぎ(glue.js)で部品の props にし、サイドバーを描く。
// コアへの道(transport)は 2 つ: 偽のコア(fake.js。ファイルを開くだけで動く)と、橋(bridge.js。本物のコアのソケットへ)。
// 本番では、この役は Rust の外側(WebKitGTK の窓・layer-shell)が受け持ち、道はソケットになる。
(function () {
  'use strict';
  const { make } = window.PreludeParts;
  const { values, steps } = window.PreludeGlue;

  const store = {
    get(k, d) {
      try {
        const v = localStorage.getItem(`prelude-proto:${k}`);
        return v == null ? d : JSON.parse(v);
      } catch {
        return d;
      }
    },
    set(k, v) {
      try {
        localStorage.setItem(`prelude-proto:${k}`, JSON.stringify(v));
      } catch {}
    },
  };

  // 偽のコア
  function fakeTransport() {
    const core = window.PreludeFake.create();
    window.preludeFakeCore = core; // 見た目の確かめ(stage)から段を変えるため
    let listener = () => {};
    return {
      name: 'fake',
      start(onSnap) {
        listener = onSnap;
        onSnap(core.snapshot());
      },
      async run(cmd, args) {
        const r = core.run(cmd, args);
        listener(core.snapshot());
        return r;
      },
      refresh() {
        listener(core.snapshot());
      },
      iconUrl: (p) => p,
    };
  }

  // 橋(bridge.js): /events で様子を受け、/cmd で命令を送る
  function bridgeTransport() {
    return {
      name: 'bridge',
      start(onSnap) {
        const es = new EventSource('events');
        es.onmessage = (e) => onSnap(JSON.parse(e.data));
        es.onerror = () => onSnap({ online: false, status: '橋につながっていません(bridge.js)', state: null });
      },
      async run(cmd, args) {
        const r = await fetch('cmd', { method: 'POST', headers: { 'content-type': 'application/json' }, body: JSON.stringify({ cmd, args }) });
        const j = await r.json();
        if (j.error) throw new Error(j.error.message || JSON.stringify(j.error));
        return j.result;
      },
      refresh() {},
      iconUrl: (p) => (p.startsWith('data:') ? p : `icon?path=${encodeURIComponent(p)}`),
    };
  }

  function start(el, opts = {}) {
    const transport = opts.transport || (location.protocol.startsWith('http') && !opts.fake ? bridgeTransport() : fakeTransport());
    let core = { online: false, status: 'つないでいます…', state: null };
    // splitMode は分割のボタンの動き(本番では settings.toml)、screen は画面の形(自動の分割の向きに使う)
    const ui = { compact: store.get('compact', false), expanded: store.get('expanded', {}), splitMode: store.get('splitMode', 'auto'), screen: { w: screen.width || 16, h: screen.height || 9 } };
    let parts = null;
    const listeners = [];
    const changed = () => {
      parts = null;
      listeners.forEach((f) => f());
    };
    const toast = opts.toast || ((m) => console.log(m));

    async function send(part, cb, args) {
      for (const s of steps(part, cb, args, core)) {
        if (s.ui === 'compact') {
          ui.compact = !ui.compact;
          store.set('compact', ui.compact);
          changed();
        } else if (s.ui === 'expand') {
          ui.expanded = { ...ui.expanded, [s.path]: s.on };
          store.set('expanded', ui.expanded);
          changed();
        } else if (s.ui === 'hide') {
          toast('サイドバーを隠す(本番では layer-shell の面を隠す)');
        } else if (s.launcher) {
          const a = s.launcher;
          toast(`ランチャーを開く: prelude-launcher${a.split ? ` --split ${a.split}${a.before ? ' --before' : ''}` : ''}${a.pane ? ` --pane ${a.pane}` : ''}${a.url ? ` --url ${a.url}` : ''}`);
        } else if (s.cmd) {
          try {
            await transport.run(s.cmd, s.args);
          } catch (e) {
            toast(`${s.cmd}: ${e.message}`);
          }
        }
      }
    }

    const host = {
      parts() {
        if (!parts) parts = make(values(core, ui, transport.iconUrl), send);
        return parts;
      },
      onChange(f) {
        listeners.push(f);
      },
      transport,
    };
    // 見た目の確かめ(stage)から分割のボタンの動きを変える
    window.addEventListener('message', (e) => {
      const m = (e.data || {}).splitMode;
      if (m !== 'auto' && m !== 'pick') return;
      ui.splitMode = m;
      store.set('splitMode', m);
      changed();
    });
    transport.start((snap) => {
      core = snap;
      changed();
    });
    window.PreludeSidebar.mount(el, host);
    return host;
  }

  // 置き方の変数(窓として浮いているときの縁の線など)。見た目はサイドバーが 1 つだけ持ち、テーマは無い。見た目の確かめ(stage)からは postMessage で届く
  function applyLook(look) {
    const r = document.documentElement;
    for (const [k, v] of Object.entries(look.vars || {})) r.style.setProperty(`--${k}`, v);
  }
  window.addEventListener('message', (e) => {
    const d = e.data || {};
    if (d.look) applyLook(d.look);
    if (d.level != null && window.preludeFakeCore) {
      window.preludeFakeCore.setLevel(d.level);
      window.preludeHost && window.preludeHost.transport.refresh();
    }
  });

  window.PreludeHost = { start, applyLook };
})();
