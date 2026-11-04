#!/bin/bash
# アプリの「開く」「保存」のダイアログを PRELUDE のファイラにする/KDE に戻す(docs/redesign-rust.md の判断 5-a)。
# on: ポータルの宣言を置き(sudo)、portals.conf で FileChooser を prelude にする。off: portals.conf を消して戻す
# ダイアログを受けるのは prelude-core なので、on の間はコアを動かしておく
set -eu
HERE="$(cd "$(dirname "$0")/.." && pwd)"
CONF="${XDG_CONFIG_HOME:-$HOME/.config}/xdg-desktop-portal/portals.conf"
PORTAL=/usr/share/xdg-desktop-portal/portals/prelude.portal
MARK="# PRELUDE scripts/portal.sh"

case "${1:-}" in
  on)
    if [ -e "$CONF" ] && ! grep -qF "$MARK" "$CONF"; then
      echo "$CONF は自分で書いた設定なので、変えません" >&2
      exit 1
    fi
    sudo install -m 644 "$HERE/portal/prelude.portal" "$PORTAL"
    mkdir -p "$(dirname "$CONF")"
    printf '%s\n[preferred]\ndefault=kde\norg.freedesktop.impl.portal.FileChooser=prelude\norg.freedesktop.impl.portal.Settings=kde;gtk;\norg.freedesktop.impl.portal.Secret=kwallet\n' "$MARK" > "$CONF"
    ;;
  off)
    if [ -e "$CONF" ] && grep -qF "$MARK" "$CONF"; then rm "$CONF"; fi
    if [ -e "$PORTAL" ]; then sudo rm "$PORTAL"; fi
    ;;
  *)
    echo "usage: portal.sh on|off" >&2
    exit 2
    ;;
esac
systemctl --user restart xdg-desktop-portal
echo "ダイアログを開いているアプリは、開き直すと切り替わります"
