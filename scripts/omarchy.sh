#!/bin/bash
# PRELUDE を Omarchy(Arch + Hyprland)に載せる/外す。docs/omarchy.md
#
#   scripts/omarchy.sh install [--no-build]   コア・コマンドを ~/.local/bin に入れ、バーの部品・メニュー・キー・自動起動を足す
#   scripts/omarchy.sh uninstall              足したものを外す(~/.config/prelude の設定・スペースは残す)
#   scripts/omarchy.sh status                 入っているもの
#
# 足すもの(どれも印の付いた塊なので、何度流しても 1 つだけ。外すときは塊ごと消す):
#   ~/.local/bin/prelude, prelude-core, prelude-omarchy-launcher, prelude-omarchy-newtab, prelude-omarchy-bookmark
#   ~/.config/omarchy/plugins/prelude.bar/        バーの部品(スペースと今のタブ。押すとサイドバー)。Omarchy の作業領域の部品は外す(タブ = 作業領域なので)
#   ~/.config/omarchy/extensions/omarchy-menu.jsonc  メニューの「PRELUDE」
#   ~/.config/hypr/bindings.lua                   Super+Alt+P でランチャー、Super+Alt+Space で新しいタブ(空いた作業領域で Omarchy のアプリの一覧)
#   ~/.config/hypr/autostart.lua                  ログインのときにコアを起動
#   ~/.config/prelude/settings.toml               profile = "omarchy"(無いときだけ作る)
set -euo pipefail

ROOT=$(cd "$(dirname "$0")/.." && pwd)
SRC="$ROOT/omarchy"
BIN="$HOME/.local/bin"
CFG="${XDG_CONFIG_HOME:-$HOME/.config}"
PLUGIN_ID="prelude.bar"
PLUGIN_DIR="$CFG/omarchy/plugins/$PLUGIN_ID"
MENU="$CFG/omarchy/extensions/omarchy-menu.jsonc"
BINDINGS="$CFG/hypr/bindings.lua"
AUTOSTART="$CFG/hypr/autostart.lua"
HYPRCONF="$CFG/hypr/hyprland.conf"
BEGIN="PRELUDE >>> (scripts/omarchy.sh)"
END="PRELUDE <<<"
KEY="SUPER + ALT + P"
NEWTAB_KEY="SUPER + ALT + SPACE"

say() { printf '\033[1m%s\033[0m\n' "$*"; }
warn() { printf 'omarchy.sh: %s\n' "$*" >&2; }
have() { command -v "$1" >/dev/null 2>&1; }

# 印の付いた塊を消す(comment はその言語の行の注釈: -- や #、JSONC は //)
drop_block() {
  local file=$1 comment=$2
  [[ -f $file ]] || return 0
  awk -v b="$comment $BEGIN" -v e="$comment $END" '
    index($0, b) == 1 { skip = 1; next }
    skip && index($0, e) == 1 { skip = 0; next }
    !skip { print }
  ' "$file" >"$file.prelude-tmp" && mv "$file.prelude-tmp" "$file"
}

# 塊を末尾に足す(前の塊は消してから)
put_block() {
  local file=$1 comment=$2 body=$3
  mkdir -p "$(dirname "$file")"
  touch "$file"
  drop_block "$file" "$comment"
  { printf '%s %s\n' "$comment" "$BEGIN"; printf '%s\n' "$body"; printf '%s %s\n' "$comment" "$END"; } >>"$file"
}

# JSONC のいちばん外の { } の中、最後の } の前に塊を入れる(Omarchy は行の注釈と末尾のカンマを許す)
put_menu() {
  mkdir -p "$(dirname "$MENU")"
  [[ -s $MENU ]] || printf '{\n}\n' >"$MENU"
  drop_menu
  local body
  body=$(sed -e '1d' -e '$d' -e '/^  \/\//d' "$SRC/menu.jsonc")
  BODY=$body awk -v b="  // $BEGIN" -v e="  // $END" '
    { lines[NR] = $0 }
    END {
      last = 0
      for (i = NR; i > 0; i--) if (lines[i] ~ /^[[:space:]]*}[[:space:]]*$/) { last = i; break }
      for (i = 1; i <= NR; i++) {
        if (i == last) {
          # 前の項目の後ろにカンマが無ければ足す
          for (j = i - 1; j > 0; j--) if (lines[j] !~ /^[[:space:]]*(\/\/.*)?$/) break
          if (j > 0 && lines[j] !~ /[,{][[:space:]]*$/) lines[j] = lines[j] ","
        }
      }
      for (i = 1; i <= NR; i++) {
        if (i == last) { print b; print ENVIRON["BODY"]; print e }
        print lines[i]
      }
    }
  ' "$MENU" >"$MENU.prelude-tmp" && mv "$MENU.prelude-tmp" "$MENU"
}

drop_menu() {
  [[ -f $MENU ]] || return 0
  awk -v b="// $BEGIN" -v e="// $END" '
    { t = $0; sub(/^[[:space:]]+/, "", t) }
    index(t, b) == 1 { skip = 1; next }
    skip && index(t, e) == 1 { skip = 0; next }
    !skip { print }
  ' "$MENU" >"$MENU.prelude-tmp" && mv "$MENU.prelude-tmp" "$MENU"
}

build() {
  have cargo || { warn "cargo がありません(sudo pacman -S rust)。--no-build なら ~/.local/bin の prelude・prelude-core をそのまま使います"; exit 1; }
  say "コアとコマンドをビルドする"
  (cd "$ROOT" && cargo build --release -p prelude-core -p prelude-cli)
  mkdir -p "$BIN"
  install -m 755 "$ROOT/target/release/prelude-core" "$ROOT/target/release/prelude" "$BIN/"
}

install_all() {
  local nobuild=${1:-}
  if [[ -z ${HYPRLAND_INSTANCE_SIGNATURE:-} ]]; then warn "Hyprland の中ではないようです。ファイルは足しますが、反映はログインし直してから"; fi
  [[ $nobuild == --no-build ]] || build
  have prelude || [[ -x $BIN/prelude ]] || { warn "prelude コマンドがありません"; exit 1; }
  case ":$PATH:" in *":$BIN:"*) ;; *) warn "$BIN が PATH にありません(Omarchy は既定で入っています)" ;; esac
  install -m 755 "$SRC/prelude-omarchy-launcher" "$SRC/prelude-omarchy-newtab" "$SRC/prelude-omarchy-bookmark" "$BIN/"

  # 設定: おすすめの組み合わせを omarchy に(使用者が書いた設定は触らない)
  mkdir -p "$CFG/prelude"
  if [[ ! -f $CFG/prelude/settings.toml ]]; then
    say "設定を作る(profile = \"omarchy\")"
    "$BIN/prelude" config set profile omarchy || warn "profile を設定できませんでした(prelude config set profile omarchy)"
  fi
  "$BIN/prelude" skill install >/dev/null 2>&1 || true

  # バーの部品。Omarchy は部品の中のシンボリックリンクを許さないので、複写する(入れ直すときは上書き)
  if [[ -d $CFG/omarchy || -n ${OMARCHY_PATH:-} ]] || have omarchy; then
    say "バーに PRELUDE を足す"
    rm -rf "$PLUGIN_DIR"
    mkdir -p "$PLUGIN_DIR"
    cp "$SRC"/plugin/*.json "$SRC"/plugin/*.qml "$SRC"/plugin/*.js "$PLUGIN_DIR/"
    if have omarchy-shell; then omarchy-shell shell rescanPlugins >/dev/null 2>&1 || true; fi
    if have omarchy; then
      # Omarchy の作業領域の部品は、PRELUDE のタブ(バーの部品・サイドバー)と同じものを出すので外す。uninstall で戻す
      omarchy plugin disable omarchy.workspaces </dev/null >/dev/null 2>&1 || true
      omarchy plugin enable "$PLUGIN_ID" left --after omarchy.menu </dev/null ||
        warn "バーに並べられませんでした。omarchy plugin enable $PLUGIN_ID left を手で流してください"
    fi
    say "メニューに PRELUDE を足す"
    put_menu
  else
    warn "Omarchy の設定(~/.config/omarchy)がありません。バーとメニューは飛ばします"
  fi

  # Hyprland: キーと自動起動。Omarchy 4 は Lua の設定、前の版は hyprland.conf
  if [[ -f $BINDINGS || -f $AUTOSTART ]]; then
    say "Hyprland に $KEY とコアの自動起動を足す"
    put_block "$BINDINGS" "--" "o.bind(\"$KEY\", \"PRELUDE\", \"prelude-omarchy-launcher\")
o.rebind(\"$NEWTAB_KEY\", \"PRELUDE new tab\", \"prelude-omarchy-newtab\")"
    put_block "$AUTOSTART" "--" 'o.launch_on_start("prelude-core")'
  elif [[ -f $HYPRCONF ]]; then
    say "Hyprland(hyprland.conf)に Super+Alt+P とコアの自動起動を足す"
    put_block "$HYPRCONF" "#" $'bind = SUPER ALT, P, exec, prelude-omarchy-launcher\nunbind = SUPER ALT, SPACE\nbind = SUPER ALT, SPACE, exec, prelude-omarchy-newtab\nexec-once = prelude-core'
  else
    warn "Hyprland の設定が見つかりません。キーと自動起動は飛ばします"
  fi

  # 今のセッションでもコアを起動する
  if [[ -n ${HYPRLAND_INSTANCE_SIGNATURE:-} ]] && ! pgrep -x prelude-core >/dev/null; then
    say "コアを起動する"
    if have uwsm-app; then setsid uwsm-app -- prelude-core >/dev/null 2>&1 &
    else setsid "$BIN/prelude-core" >/dev/null 2>&1 &
    fi
  fi
  say "できました。バーの P を押すとサイドバー、$NEWTAB_KEY で新しいタブ、$KEY でランチャー、右クリックで PRELUDE のメニュー(部品が古いままなら omarchy restart shell)"
}

uninstall_all() {
  if have omarchy && [[ -d $PLUGIN_DIR ]]; then omarchy plugin disable "$PLUGIN_ID" </dev/null || true; fi
  if have omarchy; then omarchy plugin enable omarchy.workspaces left --after omarchy.menu </dev/null >/dev/null 2>&1 || true; fi
  rm -rf "$PLUGIN_DIR"
  if have omarchy-shell; then omarchy-shell shell rescanPlugins >/dev/null 2>&1 || true; fi
  drop_menu
  drop_block "$BINDINGS" "--"
  drop_block "$AUTOSTART" "--"
  drop_block "$HYPRCONF" "#"
  rm -f "$BIN/prelude-omarchy-launcher" "$BIN/prelude-omarchy-newtab" "$BIN/prelude-omarchy-bookmark"
  say "外しました(コア・コマンド・~/.config/prelude は残しています。止めるなら pkill -x prelude-core)"
}

status() {
  local f
  for f in "$BIN/prelude" "$BIN/prelude-core" "$BIN/prelude-omarchy-launcher" "$BIN/prelude-omarchy-newtab" "$BIN/prelude-omarchy-bookmark" "$PLUGIN_DIR/manifest.json"; do
    [[ -e $f ]] && echo "ok  $f" || echo "--  $f"
  done
  for f in "$MENU" "$BINDINGS" "$AUTOSTART" "$HYPRCONF"; do
    grep -qF "$BEGIN" "$f" 2>/dev/null && echo "ok  $f" || echo "--  $f"
  done
  pgrep -x prelude-core >/dev/null && echo "ok  prelude-core が動いている" || echo "--  prelude-core は止まっている"
  if have prelude; then prelude status 2>/dev/null | head -5 || true; fi
}

case "${1:-}" in
  install) install_all "${2:-}" ;;
  uninstall) uninstall_all ;;
  status) status ;;
  *) sed -n '2,8p' "$0" | sed 's/^# \{0,1\}//'; exit 1 ;;
esac
