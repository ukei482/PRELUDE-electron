#!/usr/bin/env bash
# prelude-shell.service が起動直後に落ち続けたときの復旧(OnFailure から呼ばれる)。
# リポジトリや current の版には依存しない単独のスクリプト(新しい版が壊れていても動くように、$ROOT/bin に置く)。
#   1. 1つ前の版(previous)があれば、そこへ戻して起動し直す(戻した版が落ちたら、次は previous が無いので 2.)
#   2. 戻せる版が無ければ、plasmashell に戻す
set -u

ROOT="${PRELUDE_ROOT:-${XDG_DATA_HOME:-$HOME/.local/share}/prelude}"
STATE="${XDG_STATE_HOME:-$HOME/.local/state}/prelude"
sc() { systemctl --user "$@"; }
log() { echo "$*"; logger -t prelude-fallback -- "$*" 2>/dev/null || true; }

mkdir -p "$STATE"
cur="$(readlink "$ROOT/current" 2>/dev/null || true)"
prev="$(readlink "$ROOT/previous" 2>/dev/null || true)"

if [[ -n "$prev" && -d "$prev" && "$prev" != "$cur" ]]; then
  # 失敗した版は previous に残さない(戻した版が落ちたとき、同じ版へ行ったり来たりしないため)
  ln -sfn "$prev" "$ROOT/.current.tmp" && mv -T "$ROOT/.current.tmp" "$ROOT/current"
  rm -f "$ROOT/previous"
  echo "$(date '+%F %T') $(basename "$cur") が起動できなかったため、$(basename "$prev") に戻しました" > "$STATE/last-rollback"
  log "rolled back $(basename "$cur") -> $(basename "$prev")"
  sc reset-failed prelude-shell.service
  sc start prelude-shell.service
  exit 0
fi

echo "$(date '+%F %T') 戻せる版が無いため、plasmashell に戻しました" > "$STATE/last-rollback"
log "no previous release; restoring plasmashell"
sc unmask plasma-plasmashell.service
sc disable prelude-shell.service 2>/dev/null || true
sc stop prelude-shell.service 2>/dev/null || true
sc reset-failed prelude-shell.service 2>/dev/null || true
sc start plasma-plasmashell.service
