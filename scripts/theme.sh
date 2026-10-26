#!/usr/bin/env bash
# PRELUDE のロック画面・ログイン画面・起動の表示を入れる/外す。テーマの中身は theme/。
#
#   theme.sh status          今の状態を表示
#   theme.sh lock on|off     ロック画面(kscreenlocker)。sudo 不要。次のログインから効く
#   theme.sh lock test       ロック画面をテストモードで開く(本当のパスワードで閉じる)
#   theme.sh sddm on|off     ログイン画面(SDDM のテーマ)。sudo が要る
#   theme.sh sddm test       ログイン画面をテストモードの窓で開く(ログインはしない。窓を閉じて終わる)
#   theme.sh boot on|off     起動時に Kubuntu のロゴを出さず、本物の起動ログを流す(quiet splash を外す)。sudo が要る
#
# ロック画面の仕組み: kscreenlocker_greet はシェルのパッケージ(plasma/shells/<名前>/contents/lockscreen)を読む。
# 名前は plasmashellrc の [Shell] ShellPackage か環境変数 PLASMA_DEFAULT_SHELL で決まる。
# plasmashellrc に書くと plasmashell(Kubuntu に戻ったとき・PRELUDE が落ちて自動復旧したとき)まで
# このパッケージを読んでデスクトップが出なくなるので、greeter を起動する KWin のユニットにだけ環境変数を足す。
set -euo pipefail

APPDIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
ROOT="${PRELUDE_ROOT:-${XDG_DATA_HOME:-$HOME/.local/share}/prelude}"
SHELL_PKG=org.prelude.desktop
SHELLS_DIR="${XDG_DATA_HOME:-$HOME/.local/share}/plasma/shells"
KWIN_DROPIN="${XDG_CONFIG_HOME:-$HOME/.config}/systemd/user/plasma-kwin_wayland.service.d/prelude-lockscreen.conf"
GREETER=/usr/lib/x86_64-linux-gnu/libexec/kscreenlocker_greet
SDDM_THEME=/usr/share/sddm/themes/prelude
SDDM_CONF=/etc/sddm.conf.d/zz-prelude.conf # 20-kubuntu.conf の Current より後に読ませる
GRUB_CONF=/etc/default/grub.d/zz-prelude.cfg

# テーマを置いてあるフォルダ。リリース(current)に theme/ があればそれ(更新で一緒に新しくなる)、無ければこの作業フォルダ
theme_dir() {
  if [[ -d "$ROOT/current/theme/shell/$SHELL_PKG" ]]; then echo "$ROOT/current/theme"; else echo "$APPDIR/theme"; fi
}

lock_on() {
  mkdir -p "$SHELLS_DIR" "$(dirname "$KWIN_DROPIN")"
  ln -sfn "$(theme_dir)/shell/$SHELL_PKG" "$SHELLS_DIR/$SHELL_PKG"
  printf '[Service]\nEnvironment=PLASMA_DEFAULT_SHELL=%s\n' "$SHELL_PKG" > "$KWIN_DROPIN"
  systemctl --user daemon-reload
  echo "ロック画面: PRELUDE(パッケージ: $(readlink "$SHELLS_DIR/$SHELL_PKG"))。次のログインから効きます"
}

lock_off() {
  rm -f "$KWIN_DROPIN" "$SHELLS_DIR/$SHELL_PKG"
  rmdir "$(dirname "$KWIN_DROPIN")" 2>/dev/null || true
  systemctl --user daemon-reload
  echo "ロック画面: KDE の標準に戻しました。次のログインから効きます"
}

sddm_on() {
  local src; src="$(theme_dir)/sddm/prelude"
  sudo rm -rf "$SDDM_THEME"
  sudo cp -rL "$src" "$SDDM_THEME" # Prompt.qml と書体は theme/common へのリンクなので実体をコピーする(sddm ユーザーはホームを読めない)
  printf '[Theme]\nCurrent=prelude\n' | sudo tee "$SDDM_CONF" >/dev/null
  echo "ログイン画面: PRELUDE。次の起動(またはログアウト)から効きます"
}

sddm_off() {
  sudo rm -f "$SDDM_CONF"
  sudo rm -rf "$SDDM_THEME"
  echo "ログイン画面: Kubuntu の標準に戻しました"
}

boot_on() {
  sudo mkdir -p "$(dirname "$GRUB_CONF")"
  printf '# PRELUDE: 起動時にロゴを出さず、本物の起動ログを流す(scripts/theme.sh boot off で戻す)\nGRUB_CMDLINE_LINUX_DEFAULT=""\n' | sudo tee "$GRUB_CONF" >/dev/null
  sudo update-grub
  echo "起動: 本物の起動ログを流します。次の起動から効きます"
}

boot_off() {
  sudo rm -f "$GRUB_CONF"
  sudo update-grub
  echo "起動: Kubuntu のロゴ(quiet splash)に戻しました"
}

status() {
  if [[ -f "$KWIN_DROPIN" ]]; then echo "lock: on ($(readlink "$SHELLS_DIR/$SHELL_PKG" 2>/dev/null || echo 'パッケージのリンクが無い'))"; else echo "lock: off"; fi
  if [[ -f "$SDDM_CONF" ]]; then echo "sddm: on"; else echo "sddm: off"; fi
  if [[ -f "$GRUB_CONF" ]]; then echo "boot: on"; else echo "boot: off"; fi
}

case "${1:-status}:${2:-}" in
  status:*) status ;;
  lock:on) lock_on ;;
  lock:off) lock_off ;;
  lock:test)
    mkdir -p "$SHELLS_DIR"
    [[ -e "$SHELLS_DIR/$SHELL_PKG" ]] || ln -sfn "$(theme_dir)/shell/$SHELL_PKG" "$SHELLS_DIR/$SHELL_PKG"
    exec "$GREETER" --testing --shell "$SHELL_PKG"
    ;;
  sddm:on) sddm_on ;;
  sddm:off) sddm_off ;;
  sddm:test) exec sddm-greeter-qt6 --test-mode --theme "$(theme_dir)/sddm/prelude" ;;
  boot:on) boot_on ;;
  boot:off) boot_off ;;
  *) sed -n '2,10p' "${BASH_SOURCE[0]}"; exit 2 ;;
esac
