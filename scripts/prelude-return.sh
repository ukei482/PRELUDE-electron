#!/usr/bin/env bash
# PRELUDE シェルから plasmashell(Kubuntu のデスクトップ)へ、段階を踏んで安全に戻る。
# 画面の「Kubuntuに戻る」ボタン(sys/session.js)と `shell-mode.sh return` から呼ばれる。
# リポジトリや current の版には依存しない単独のスクリプト(新しい版が壊れていても動くように、$ROOT/bin に置く)。
#
#   prelude-return.sh          戻る
#   prelude-return.sh --check  事前確認だけ行い、何も変えない
#
# 手順(失敗したら、その時点で PRELUDE に戻して終わる):
#   0. 事前確認: plasmashell を起動できるか。ここで駄目なら何も触らない
#   1. PRELUDE を止める。終了処理(取り込みアプリを穏やかに閉じる)を待つ。強制終了は最後の手段
#   2. キーリング(KWallet)が応答するか確かめる。固まっていれば再起動する
#      (応答しないまま plasmashell に戻ると、Claude Desktop などがウォレット待ちで起動できなくなる)
#   3. plasmashell を起動し、トレイと通知の名前を取るまで待つ
#   4. 成功したら PRELUDE の自動起動を解除する
#
# 自分は一時ユニット(systemd-run)で動く。prelude-shell.service の中で動くと、止めた時に一緒に消えてしまうため。
set -u

ROOT="${PRELUDE_ROOT:-${XDG_DATA_HOME:-$HOME/.local/share}/prelude}"
STATE="${XDG_STATE_HOME:-$HOME/.local/state}/prelude"
PLASMA=plasma-plasmashell.service
SHELL_UNIT=prelude-shell.service
LOG="$STATE/return.log"
FLAG="$STATE/returning"   # prelude-fallback.sh が見て、この間は自動復旧を動かさない
sc() { systemctl --user "$@"; }
log() { echo "$(date '+%F %T') $*" | tee -a "$LOG"; logger -t prelude-return -- "$*" 2>/dev/null || true; }
note() { notify-send -a PRELUDE -i dialog-information "$1" "${2:-}" 2>/dev/null || true; }

mkdir -p "$STATE"

# 名前 $1 の持ち主のプロセス名(居なければ空)
owner_comm() {
  local pid; pid="$(busctl --user status "$1" 2>/dev/null | sed -n 's/^PID=//p')"
  [[ -n "$pid" ]] && cat "/proc/$pid/comm" 2>/dev/null
}

check() {
  sc cat "$PLASMA" >/dev/null 2>&1 || { log "$PLASMA が見つかりません(systemd 起動の Plasma ではありません)"; return 1; }
  command -v plasmashell >/dev/null || { log "plasmashell が見つかりません"; return 1; }
  [[ "$(sc is-enabled "$PLASMA" 2>&1)" != "not-found" ]] || { log "$PLASMA が有効化できません"; return 1; }
  return 0
}

if [[ "${1:-}" == "--check" ]]; then
  check && echo "OK: 戻れる状態です"
  exit $?
fi

# 一時ユニットで動いていなければ、そうしてから動き直す(prelude-shell.service の中から呼ばれても安全にする)
if [[ -z "${PRELUDE_RETURN_DETACHED:-}" ]]; then
  check || exit 1
  exec systemd-run --user --collect --quiet --unit=prelude-return \
    --description="PRELUDE から Kubuntu に戻る" \
    --setenv=PRELUDE_RETURN_DETACHED=1 --setenv=PRELUDE_ROOT="$ROOT" \
    "$(readlink -f "$0")"
fi

: > "$LOG"
log "開始"
check || { note "Kubuntuに戻れません" "詳細: $LOG"; exit 1; }
touch "$FLAG"
trap 'rm -f "$FLAG"' EXIT

# 失敗時: PRELUDE を元通りにして終わる(plasmashell は止めたままにする)
revert() {
  log "失敗: $1 → PRELUDE を維持します"
  sc stop "$PLASMA" 2>/dev/null
  sc mask "$PLASMA" 2>/dev/null
  sc reset-failed "$SHELL_UNIT" 2>/dev/null
  sc start "$SHELL_UNIT" 2>/dev/null
  note "Kubuntuに戻れませんでした" "$1(PRELUDE を続けます。詳細: $LOG)"
  exit 1
}

# 1. PRELUDE を止める。unmask は止めたあと(止める前に plasmashell が動ける状態にすると、通知などの名前を取り合う)
log "1/4 PRELUDE を終了します"
sc stop "$SHELL_UNIT" || log "stop が失敗(強制終了された可能性)"
for _ in $(seq 1 30); do
  [[ "$(sc is-active "$SHELL_UNIT" 2>&1)" == "inactive" ]] && break
  sleep 0.5
done
[[ "$(sc is-active "$SHELL_UNIT" 2>&1)" == "inactive" || "$(sc is-active "$SHELL_UNIT" 2>&1)" == "failed" ]] || revert "PRELUDE が終了しませんでした"
sc reset-failed "$SHELL_UNIT" 2>/dev/null

# 2. キーリング。動いているのに応答しないなら再起動する(起動していなければ、必要になった時に正しく起動するので触らない)
log "2/4 キーリングを確認します"
for name in org.kde.kwalletd6; do
  if busctl --user status "$name" >/dev/null 2>&1; then
    if timeout 5 busctl --user --timeout=3 call "$name" /modules/kwalletd6 org.kde.KWallet isEnabled >/dev/null 2>&1; then
      log "  $name: 正常"
    else
      pid="$(busctl --user status "$name" 2>/dev/null | sed -n 's/^PID=//p')"
      log "  $name(pid $pid)が応答しないため再起動します"
      [[ -n "$pid" ]] && kill "$pid" 2>/dev/null
      sleep 1
    fi
  fi
done

# 3. plasmashell
log "3/4 plasmashell を起動します"
sc unmask "$PLASMA" || revert "plasmashell を有効にできませんでした"
sc reset-failed "$PLASMA" 2>/dev/null
sc start "$PLASMA" || revert "plasmashell を起動できませんでした"
ok=0
for _ in $(seq 1 60); do
  # 通知サーバの名前と、トレイのホストを plasmashell が持てば、シェルとして働いている
  if [[ "$(owner_comm org.freedesktop.Notifications)" == "plasmashell" ]] && busctl --user list 2>/dev/null | grep -q '^org.kde.StatusNotifierHost-'; then ok=1; break; fi
  [[ "$(sc is-active "$PLASMA" 2>&1)" == "active" ]] || break
  sleep 0.5
done
(( ok )) || revert "plasmashell が準備できませんでした"

# 4. 次回ログインも plasmashell にする
sc disable "$SHELL_UNIT" 2>/dev/null
echo "$(date '+%F %T') 「Kubuntuに戻る」で plasmashell に戻しました" > "$STATE/last-rollback"
log "4/4 完了"
note "Kubuntuに戻りました" "PRELUDE をまた使うには: scripts/shell-mode.sh on --now"
exit 0
