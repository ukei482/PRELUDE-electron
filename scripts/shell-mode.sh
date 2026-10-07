#!/usr/bin/env bash
# PRELUDE をデスクトップシェルとして使うか、plasmashell に戻すかを切り替える。
#
#   shell-mode.sh status        今の状態を表示
#   shell-mode.sh on  [--now]   plasmashell を止め(mask)、PRELUDE を次回ログインから自動起動
#   shell-mode.sh off [--now]   元に戻す(unmask して plasmashell を起動、PRELUDE の自動起動を解除)
#
# --now を付けると、ログインし直さずその場で切り替える。
# 画面が操作不能になったら TTY(Ctrl+Alt+F3)でログインして `shell-mode.sh off --now` を実行する。
set -euo pipefail

APPDIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
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

install_units() {
  mkdir -p "$UNIT_DIR"
  for u in "${SHELL_UNITS[@]}"; do
    sed "s|@APPDIR@|$APPDIR|g" "$APPDIR/systemd/$u" > "$UNIT_DIR/$u"
  done
  sc daemon-reload
}

case "${1:-status}" in
  status)
    echo "plasmashell : $(sc is-enabled "$PLASMA" 2>&1 || true) / $(sc is-active "$PLASMA" 2>&1 || true)"
    echo "prelude-shell: $(sc is-enabled prelude-shell.service 2>&1 || true) / $(sc is-active prelude-shell.service 2>&1 || true)"
    ;;
  on)
    need_systemd_boot
    install_units
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
  *)
    echo "usage: $0 {status|on [--now]|off [--now]}" >&2
    exit 2
    ;;
esac
