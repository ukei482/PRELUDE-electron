#!/usr/bin/env bash
# PRELUDE の起動・停止・切り替えをまとめた入口。中身は既存のスクリプト(shell-mode.sh / release.sh)と electron の起動。
#
#   prelude.sh start [-f]       作業フォルダの PRELUDE を起動(既定は端末から切り離す。-f で手前に出してログを表示)
#   prelude.sh debug [port]     デバッグ起動: 別プロファイル + PRELUDE_DEBUG=1(取り込みのログは /tmp/prelude-embed.log)。
#                               port を付けると DevTools のリモートデバッグも開く
#   prelude.sh stop             手で起動した PRELUDE を終了(取り込んだアプリも穏やかに閉じる)。シェルとして動いている時は return を使う
#   prelude.sh restart          stop してから start
#   prelude.sh status           シェルモード・動いている PRELUDE・リリースの状態
#   prelude.sh log [embed]      シェルのログを追う(journalctl)。embed なら取り込みのデバッグログ
#
#   prelude.sh shell on [--now] PRELUDE をデスクトップシェルにする(= shell-mode.sh on)
#   prelude.sh shell off [--now] plasmashell に戻す(確認なしで即座に入れ替え。= shell-mode.sh off)
#   prelude.sh return           Kubuntu に安全に戻る(画面の「Kubuntuに戻る」と同じ。= shell-mode.sh return)
#   prelude.sh release ...      リリース管理(= release.sh。create / switch / rollback / status / prune)
#   prelude.sh install          依存関係を入れる(npm ci。node_modules が無いと start/debug が自動で行う)
#
# 画面が操作不能になったら TTY(Ctrl+Alt+F3)でログインして `prelude.sh shell off --now`。
set -euo pipefail

APPDIR="$(cd "$(dirname "$(readlink -f "${BASH_SOURCE[0]}")")/.." && pwd)"
STATE="${XDG_STATE_HOME:-$HOME/.local/state}/prelude"
ELECTRON="$APPDIR/node_modules/electron/dist/electron"
BUS=org.prelude.Shell
UNIT=prelude-shell.service

die() { echo "prelude: $*" >&2; exit 1; }

ensure_deps() {
  [[ -x "$ELECTRON" ]] && return
  echo "依存関係を入れます(npm ci)…"
  (cd "$APPDIR" && npm ci)
}

# org.prelude.Shell を持っている PRELUDE の PID(居なければ空)
running_pid() { busctl --user status "$BUS" 2>/dev/null | sed -n 's/^PID=//p' || true; }

# その PID が prelude-shell.service 本体か
is_service() {
  local main; main="$(systemctl --user show -p MainPID --value "$UNIT" 2>/dev/null || true)"
  [[ -n "$1" && "$1" == "$main" ]]
}

# 起動前の確認: 既に動いていれば、二重起動にせず知らせる(Electron の単一インスタンスで、後から起動した方はすぐ終わる)
check_not_running() {
  local pid; pid="$(running_pid)"
  [[ -z "$pid" ]] && return
  if is_service "$pid"; then die "PRELUDE はデスクトップシェルとして動いています(pid $pid)。止めるなら: prelude.sh return"; fi
  die "PRELUDE は既に動いています(pid $pid)。再起動するなら: prelude.sh restart"
}

launch() { # launch <前面?> <ログファイル> <electron の引数...>
  local fg="$1" logf="$2"; shift 2
  if [[ "$fg" == 1 ]]; then exec "$ELECTRON" "$@"; fi
  mkdir -p "$STATE"
  # 端末を閉じても残るよう、セッションごと切り離す
  setsid nohup "$ELECTRON" "$@" >"$logf" 2>&1 </dev/null &
  echo "起動しました(pid $!、ログ: $logf)"
}

cmd_start() {
  local fg=0
  [[ "${1:-}" == "-f" ]] && fg=1
  ensure_deps
  check_not_running
  launch "$fg" "$STATE/dev.log" "$APPDIR" --no-sandbox
}

cmd_debug() {
  local port="${1:-}"
  ensure_deps
  check_not_running
  # 普段のプロファイル(タブ・設定)を汚さないよう、デバッグ用の別プロファイルで動かす
  local args=("$APPDIR" --no-sandbox "--user-data-dir=$STATE/debug-profile")
  # 注意: リモートデバッグのポートは、PRELUDE から起動した Chromium に引き継がれて残ることがある(終了後もポートが塞がる)
  [[ -n "$port" ]] && args+=("--remote-debugging-port=$port")
  export PRELUDE_DEBUG=1
  launch 0 "$STATE/debug.log" "${args[@]}"
  echo "取り込みのログ: tail -f /tmp/prelude-embed.log"
  [[ -n "$port" ]] && echo "DevTools: http://localhost:$port/json"
  return 0
}

cmd_stop() {
  local pid; pid="$(running_pid)"
  [[ -z "$pid" ]] && { echo "PRELUDE は動いていません"; return 0; }
  is_service "$pid" && die "PRELUDE はデスクトップシェルとして動いています。Kubuntu に戻るなら: prelude.sh return"
  # SIGTERM = 通常の終了(取り込んだアプリを最大8秒待って閉じる)。20秒で終わらなければ強制終了
  kill -TERM "$pid"
  for _ in $(seq 1 40); do kill -0 "$pid" 2>/dev/null || { echo "終了しました"; return 0; }; sleep 0.5; done
  echo "終了しないため強制終了します" >&2
  kill -KILL "$pid" 2>/dev/null || true
}

cmd_status() {
  "$APPDIR/scripts/shell-mode.sh" status
  local pid; pid="$(running_pid)"
  if [[ -z "$pid" ]]; then echo "動いている PRELUDE: なし"
  elif is_service "$pid"; then echo "動いている PRELUDE: pid $pid(デスクトップシェル)"
  else echo "動いている PRELUDE: pid $pid($(tr '\0' ' ' </proc/"$pid"/cmdline | grep -o -- '--user-data-dir=[^ ]*' || echo '通常のプロファイル'))"
  fi
  "$APPDIR/scripts/release.sh" status 2>/dev/null || true
}

cmd_log() {
  if [[ "${1:-}" == "embed" ]]; then exec tail -f /tmp/prelude-embed.log; fi
  exec journalctl --user -u "$UNIT" -u prelude-shell-fallback.service -f
}

case "${1:-start}" in
  start) shift || true; cmd_start "$@" ;;
  debug) shift; cmd_debug "$@" ;;
  stop) cmd_stop ;;
  restart) cmd_stop; sleep 1; shift; cmd_start "$@" ;;
  status) cmd_status ;;
  log) shift; cmd_log "$@" ;;
  shell) shift; exec "$APPDIR/scripts/shell-mode.sh" "$@" ;;
  return) exec "$APPDIR/scripts/shell-mode.sh" return ;;
  release) shift; exec "$APPDIR/scripts/release.sh" "$@" ;;
  install) (cd "$APPDIR" && npm ci) ;;
  -h|--help|help) sed -n '2,20p' "$0" | sed 's/^# \{0,1\}//' ;;
  *) echo "不明なコマンド: $1(prelude.sh help で一覧)" >&2; exit 2 ;;
esac
