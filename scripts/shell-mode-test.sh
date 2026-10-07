#!/usr/bin/env bash
# shell-mode.sh の実機テスト。plasmashell を一時的に止めて戻し、PRELUDE が落ち続けたときの自動復旧も確かめる。
# 終了時(途中で失敗しても)必ず plasmashell を元に戻し、テスト用に入れたユニットを削除する。
#
#   TEST1: on --now で plasmashell が止まり PRELUDE が起動する → off --now で戻る
#   TEST2: PRELUDE が起動直後に落ち続ける → OnFailure で plasmashell が自動復旧する
#
# テスト中はパネル・通知・トレイが消える。動作中の PRELUDE には触れない(別のデータフォルダで起動する)。
# 画面が操作不能になったら TTY(Ctrl+Alt+F3)で `scripts/shell-mode.sh off --now` を実行する。
set -u

APP="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TMP="$(mktemp -d)"
UD="${XDG_CONFIG_HOME:-$HOME/.config}/systemd/user"
DROP="$UD/prelude-shell.service.d"
SC="systemctl --user"

st() {
  echo "  plasmashell: enabled=$($SC is-enabled plasma-plasmashell 2>&1) active=$($SC is-active plasma-plasmashell 2>&1)" \
       "| prelude-shell: active=$($SC is-active prelude-shell 2>&1)"
}

cleanup() {
  echo "== CLEANUP"
  "$APP/scripts/shell-mode.sh" off --now
  rm -rf "$DROP" "$UD/prelude-shell.service" "$UD/prelude-shell-fallback.service" "$TMP"
  $SC daemon-reload
  $SC reset-failed prelude-shell.service prelude-shell-fallback.service 2>/dev/null
  st
}
trap cleanup EXIT

mkdir -p "$DROP" "$TMP/ud"
echo '{"behavior":{"startFullscreen":false}}' > "$TMP/ud/config.json"

echo "== BEFORE"; st
"$APP/scripts/shell-mode.sh" on >/dev/null   # ユニットを入れるだけ(--now なし)

# テスト用: 動作中の PRELUDE と衝突しないよう別のデータフォルダで起動する
cat > "$DROP/test.conf" <<EOF
[Service]
ExecStart=
ExecStart=$APP/node_modules/electron/dist/electron $APP --no-sandbox --user-data-dir=$TMP/ud --remote-debugging-port=9666
EOF
$SC daemon-reload

echo "== TEST1: on --now"
"$APP/scripts/shell-mode.sh" on --now; sleep 9; st
echo "  PRELUDE processes: $(pgrep -f "user-data-dir=$TMP/ud" | wc -l)"
echo "  org.freedesktop.Notifications owners: $(busctl --user list 2>/dev/null | grep -c 'org.freedesktop.Notifications')" \
     "| org.prelude.Shell: $(busctl --user list 2>/dev/null | grep -c 'org.prelude.Shell')"
$SC status prelude-shell --no-pager 2>&1 | sed -n 1,6p

echo "== TEST1a: notification server takes over from plasmashell"
echo "  owner of org.freedesktop.Notifications: $(busctl --user status org.freedesktop.Notifications 2>/dev/null | grep -E '^Comm=' | tr '\n' ' ')"
notify-send -a prelude-test -t 20000 "PRELUDEテスト" "plasmashell なしで届く"; sleep 2
node - <<'JS'
const http = require('http');
http.get('http://127.0.0.1:9666/json', (r) => { let d = ''; r.on('data', (c) => { d += c; }); r.on('end', async () => {
  const ev = async (re, expr) => {
    const t = JSON.parse(d).find((x) => x.type === 'page' && re.test(x.url));
    if (!t) return '(ウィンドウなし)';
    const ws = new WebSocket(t.webSocketDebuggerUrl);
    await new Promise((ok) => { ws.onopen = ok; });
    return new Promise((ok) => { ws.onmessage = (m) => { ok(JSON.parse(m.data).result.result.value); ws.close(); };
      ws.send(JSON.stringify({ id: 1, method: 'Runtime.evaluate', params: { expression: expr, returnByValue: true } })); });
  };
  console.log('  toast text:', await ev(/overlay\.html/, "document.getElementById('toasts').innerText.replace(/\\n/g, ' | ')"));
  console.log('  notify state (serving):', await ev(/index\.html/, 'bar.notify.serving'));
  process.exit(0);
}); }).on('error', (e) => { console.log('  CDP error:', e.message); process.exit(0); });
JS

echo "== TEST1b: clean stop, then off --now"
# 正常な停止は「失敗」扱いにならない(失敗だと OnFailure で余計な復旧が走る)。reset-failed の前に結果を読む
$SC stop prelude-shell.service; sleep 4
echo "  prelude-shell result after clean stop: $($SC show prelude-shell -p Result --value 2>&1) (期待値: success)"
echo "  OnFailure triggered during clean stop: $(journalctl --user -u prelude-shell --since '-15sec' --no-pager 2>/dev/null | grep -c 'Triggering OnFailure') (期待値: 0)"
"$APP/scripts/shell-mode.sh" off --now; sleep 3; st
echo "  owner of org.freedesktop.Notifications after restore: $(busctl --user status org.freedesktop.Notifications 2>/dev/null | grep -E '^Comm=' | tr '\n' ' ')"

echo "== TEST2: fallback (PRELUDE keeps failing)"
cat > "$DROP/test.conf" <<EOF
[Service]
ExecStart=
ExecStart=/bin/false
EOF
$SC daemon-reload
"$APP/scripts/shell-mode.sh" on --now
for i in $(seq 1 15); do
  sleep 2
  a=$($SC is-active plasma-plasmashell 2>&1)
  echo "  t=$((i * 2))s plasmashell=$a prelude-shell=$($SC is-active prelude-shell 2>&1)"
  [ "$a" = active ] && break
done
st
journalctl --user -u prelude-shell-fallback --no-pager -n 5 2>&1 | tail -5
