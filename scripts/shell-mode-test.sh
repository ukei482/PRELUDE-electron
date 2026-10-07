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
ExecStart=$APP/node_modules/electron/dist/electron $APP --no-sandbox --user-data-dir=$TMP/ud
EOF
$SC daemon-reload

echo "== TEST1: on --now"
"$APP/scripts/shell-mode.sh" on --now; sleep 9; st
echo "  PRELUDE processes: $(pgrep -f "user-data-dir=$TMP/ud" | wc -l)"
echo "  org.freedesktop.Notifications owners: $(busctl --user list 2>/dev/null | grep -c 'org.freedesktop.Notifications')" \
     "| org.prelude.Shell: $(busctl --user list 2>/dev/null | grep -c 'org.prelude.Shell')"
$SC status prelude-shell --no-pager 2>&1 | sed -n 1,6p

echo "== TEST1b: clean stop, then off --now"
# 正常な停止は「失敗」扱いにならない(失敗だと OnFailure で余計な復旧が走る)。reset-failed の前に結果を読む
$SC stop prelude-shell.service; sleep 4
echo "  prelude-shell result after clean stop: $($SC show prelude-shell -p Result --value 2>&1) (期待値: success)"
echo "  OnFailure triggered during clean stop: $(journalctl --user -u prelude-shell --since '-15sec' --no-pager 2>/dev/null | grep -c 'Triggering OnFailure') (期待値: 0)"
"$APP/scripts/shell-mode.sh" off --now; sleep 3; st

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
