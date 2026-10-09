#!/usr/bin/env bash
# PRELUDE をデスクトップシェルとして使うか、plasmashell に戻すかを切り替える。
#
#   shell-mode.sh status        今の状態を表示
#   shell-mode.sh on  [--now]   plasmashell を止め(mask)、PRELUDE を次回ログインから自動起動
#                               起動するコードは、リリース(scripts/release.sh の current)があればそれ、無ければこの作業フォルダ
#   shell-mode.sh return       PRELUDE を穏やかに終了し、plasmashell が準備できるのを確かめて戻る(失敗したら PRELUDE のまま)。
#                               画面の「Kubuntuに戻る」と同じ。`off --now` は確認なしで即座に入れ替える
#   shell-mode.sh off [--now]   元に戻す(unmask して plasmashell を起動、PRELUDE の自動起動を解除)
#
# --now を付けると、ログインし直さずその場で切り替える。
# 画面が操作不能になったら TTY(Ctrl+Alt+F3)でログインして `shell-mode.sh off --now` を実行する。
set -euo pipefail

APPDIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
ROOT="${PRELUDE_ROOT:-${XDG_DATA_HOME:-$HOME/.local/share}/prelude}"
UNIT_DIR="${XDG_CONFIG_HOME:-$HOME/.config}/systemd/user"
SHELL_UNITS=(prelude-shell.service prelude-shell-fallback.service)
PLASMA=plasma-plasmashell.service

sc() { systemctl --user "$@"; }

need_systemd_boot() {
  # Plasma 6 の systemd 起動でなければ、plasmashell をユニットとして止められない
  if ! sc cat "$PLASMA" >/dev/null 2>&1; then
    echo "$PLASMA が見つかりません。この Plasma は systemd 起動ではないため、この方法では切り替えられません。" >&2
    exit 1
  fi
}

# サービスが起動するコードの場所。リリース(current)があればそれ、無ければこの作業フォルダ
run_dir() {
  if [[ -L "$ROOT/current" && -x "$ROOT/current/node_modules/electron/dist/electron" ]]; then echo "$ROOT/current"; else echo "$APPDIR"; fi
}

# サービス以外で動いている PRELUDE の本体プロセス(手で起動したもの、固まって残っているもの)。
# Electron は同じプロファイルで2つ目を起動するとすぐ終わるので、これが居るとサービスの PRELUDE は起動できない
manual_prelude_pids() {
  local main; main="$(sc show -p MainPID --value prelude-shell.service 2>/dev/null || true)"
  local pid args
  for pid in $(pgrep -f 'node_modules/electron/dist/electron' || true); do
    args="$(tr '\0' ' ' </proc/"$pid"/cmdline 2>/dev/null)" || continue
    [[ "$args" == *--type=* ]] && continue                      # Chromium の子プロセス
    [[ "$args" == *"$APPDIR"* || "$args" == *"$ROOT/"* ]] || continue # PRELUDE 以外の Electron アプリ
    [[ "$args" == *--user-data-dir=* ]] && continue             # 別プロファイル(prelude.sh debug など)は衝突しない
    [[ "$pid" == "$main" ]] && continue
    echo "$pid"
  done
}

# サービスが起動するコードが、この作業フォルダのコードと違えば知らせる(リリースを切り替え忘れて、古い版がシェルになるのを防ぐ)
warn_release() {
  local dir; dir="$(run_dir)"
  [[ "$dir" == "$APPDIR" ]] && return
  local cur work
  cur="$(git -C "$dir" rev-parse --short HEAD 2>/dev/null || echo '?')"
  work="$(git -C "$APPDIR" rev-parse --short HEAD 2>/dev/null || echo '?')"
  if [[ "$cur" != "$work" ]]; then
    echo "注意: シェルとして起動するのはリリース $(basename "$(readlink -f "$dir")")($cur)で、作業フォルダ($work)とは違うコードです。" >&2
    echo "      作業フォルダの変更を使うなら: scripts/release.sh create <tag> → scripts/release.sh switch <tag>" >&2
  fi
}

install_units() {
  mkdir -p "$UNIT_DIR"
  "$APPDIR/scripts/release.sh" install-bin   # 新しい版が壊れていても動くフォールバックを $ROOT/bin に置く
  local dir; dir="$(run_dir)"
  for u in "${SHELL_UNITS[@]}"; do
    sed -e "s|@APPDIR@|$dir|g" -e "s|@ROOT@|$ROOT|g" "$APPDIR/systemd/$u" > "$UNIT_DIR/$u"
  done
  sc daemon-reload
  echo "起動するコード: $dir"
}

case "${1:-status}" in
  status)
    echo "plasmashell : $(sc is-enabled "$PLASMA" 2>&1 || true) / $(sc is-active "$PLASMA" 2>&1 || true)"
    echo "prelude-shell: $(sc is-enabled prelude-shell.service 2>&1 || true) / $(sc is-active prelude-shell.service 2>&1 || true)"
    ;;
  on)
    need_systemd_boot
    if [[ "${2:-}" == "--now" ]]; then
      pids="$(manual_prelude_pids)"
      if [[ -n "$pids" ]]; then
        echo "手で起動した PRELUDE が動いています(pid: $(echo $pids))。このままではサービスの PRELUDE が二重起動として終了します。" >&2
        echo "先に終了してください: scripts/prelude.sh stop(固まっているなら kill $(echo $pids))" >&2
        exit 1
      fi
    fi
    install_units
    warn_release
    sc enable prelude-shell.service
    sc mask "$PLASMA"
    if [[ "${2:-}" == "--now" ]]; then
      sc stop "$PLASMA"
      sc start prelude-shell.service
    fi
    echo "シェルモード ON(元に戻す: $0 off --now)"
    ;;
  off)
    need_systemd_boot
    sc unmask "$PLASMA"
    sc disable prelude-shell.service 2>/dev/null || true
    if [[ "${2:-}" == "--now" ]]; then
      sc stop prelude-shell.service 2>/dev/null || true
      sc reset-failed prelude-shell.service 2>/dev/null || true
      sc start "$PLASMA"
    fi
    echo "シェルモード OFF(plasmashell に戻しました)"
    ;;
  return)
    need_systemd_boot
    "$APPDIR/scripts/release.sh" install-bin
    exec "$ROOT/bin/prelude-return.sh" "${2:-}"
    ;;
  *)
    echo "usage: $0 {status|on [--now]|off [--now]|return}" >&2
    exit 2
    ;;
esac
