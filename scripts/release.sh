#!/usr/bin/env bash
# PRELUDE のリリース(タグごとの配置)を管理する。常用する版(current)と開発中の作業フォルダを分けるための仕組み。
#
#   release.sh create <tag>   タグの版を $ROOT/releases/<tag> に作る(git worktree + npm ci + 構文チェック)。切り替えはしない
#   release.sh switch <tag>   current を <tag> にする(今の current は previous になる)
#   release.sh rollback       current と previous を入れ替える(1つ前の版に戻す)
#   release.sh status         current / previous / 一覧を表示
#   release.sh prune [N]      新しい順に N 個(既定3)と current/previous 以外を削除
#   release.sh install-bin    フォールバック用スクリプトを $ROOT/bin に置く(shell-mode.sh on でも行う)
#
# 配置: $ROOT = ${PRELUDE_ROOT:-~/.local/share/prelude}
#   releases/<tag>/   git worktree(その版のコード + node_modules)
#   current, previous 版へのシンボリックリンク(systemd のユニットは current を起動する)
#   bin/              新しい版が壊れていても動く、単独のフォールバックスクリプト
# 切り替えたあとの反映は、サービスの再起動(systemctl --user restart prelude-shell)。
set -euo pipefail

REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
ROOT="${PRELUDE_ROOT:-${XDG_DATA_HOME:-$HOME/.local/share}/prelude}"
REL="$ROOT/releases"

die() { echo "エラー: $*" >&2; exit 1; }
valid_name() { [[ "$1" =~ ^[A-Za-z0-9][A-Za-z0-9._-]*$ ]] || die "不正なタグ名です: $1"; }
link_target() { [[ -L "$ROOT/$1" ]] && readlink "$ROOT/$1" || true; }
tag_of() { [[ -n "$1" ]] && basename "$1" || echo "-"; }

# リンクを原子的に張り替える(途中の状態を見せない)
point() { ln -sfn "$2" "$ROOT/.$1.tmp" && mv -T "$ROOT/.$1.tmp" "$ROOT/$1"; }

cmd_install_bin() {
  mkdir -p "$ROOT/bin"
  install -m 755 "$REPO/scripts/prelude-fallback.sh" "$ROOT/bin/prelude-fallback.sh"
}

cmd_create() {
  local tag="${1:-}"; [[ -n "$tag" ]] || die "使い方: release.sh create <tag>"
  valid_name "$tag"
  git -C "$REPO" rev-parse -q --verify "refs/tags/$tag^{commit}" >/dev/null || die "タグ $tag がありません(git tag で確認)"
  [[ ! -e "$REL/$tag" ]] || die "$REL/$tag は既にあります"
  mkdir -p "$REL"
  git -C "$REPO" worktree add --detach "$REL/$tag" "refs/tags/$tag" >/dev/null
  local ok=1
  # Electron は npm ci だけでは実行ファイルを取得しない(キャッシュがあれば1〜2秒)ので、明示的に取得する
  ( cd "$REL/$tag" && npm ci --no-audit --no-fund >/dev/null 2>&1 && node node_modules/electron/install.js >/dev/null 2>&1 ) || ok=0
  if (( ok )); then
    # 起動前に分かる誤りは、ここで止める
    for f in "$REL/$tag"/main.js "$REL/$tag"/preload*.js "$REL/$tag"/sys/*.js "$REL/$tag"/src/*.js; do
      node --check "$f" 2>/dev/null || { echo "構文エラー: $f" >&2; ok=0; }
    done
    [[ -x "$REL/$tag/node_modules/electron/dist/electron" ]] || { echo "electron が入っていません" >&2; ok=0; }
  fi
  if (( ! ok )); then
    git -C "$REPO" worktree remove --force "$REL/$tag" >/dev/null 2>&1 || true
    die "$tag の作成に失敗しました(削除しました)"
  fi
  cmd_install_bin
  echo "作成しました: $REL/$tag(切り替えは: release.sh switch $tag)"
}

cmd_switch() {
  local tag="${1:-}"; [[ -n "$tag" ]] || die "使い方: release.sh switch <tag>"
  valid_name "$tag"
  [[ -x "$REL/$tag/node_modules/electron/dist/electron" ]] || die "$tag はまだ作られていません(release.sh create $tag)"
  local cur; cur="$(link_target current)"
  if [[ "$cur" != "$REL/$tag" ]]; then
    [[ -n "$cur" ]] && point previous "$cur"
    point current "$REL/$tag"
  fi
  echo "current = $tag / previous = $(tag_of "$(link_target previous)")"
}

cmd_rollback() {
  local cur prev; cur="$(link_target current)"; prev="$(link_target previous)"
  [[ -n "$prev" && -d "$prev" ]] || die "戻せる版(previous)がありません"
  point previous "$cur"
  point current "$prev"
  echo "current = $(tag_of "$prev") / previous = $(tag_of "$cur")"
}

cmd_status() {
  echo "ROOT     : $ROOT"
  echo "current  : $(tag_of "$(link_target current)")"
  echo "previous : $(tag_of "$(link_target previous)")"
  echo "releases : $(ls "$REL" 2>/dev/null | tr '\n' ' ')"
}

cmd_prune() {
  local keep="${1:-3}" n=0 cur prev; cur="$(link_target current)"; prev="$(link_target previous)"
  [[ "$keep" =~ ^[0-9]+$ ]] || die "N は数字で指定してください"
  while IFS= read -r d; do
    d="$REL/$d"
    n=$((n + 1))
    (( n <= keep )) && continue
    [[ "$d" == "$cur" || "$d" == "$prev" ]] && continue
    git -C "$REPO" worktree remove --force "$d" >/dev/null 2>&1 || rm -rf "$d"
    echo "削除: $d"
  done < <(ls -t "$REL" 2>/dev/null)
  git -C "$REPO" worktree prune
}

case "${1:-status}" in
  create) shift; cmd_create "$@" ;;
  switch) shift; cmd_switch "$@" ;;
  rollback) cmd_rollback ;;
  status) cmd_status ;;
  prune) shift; cmd_prune "$@" ;;
  install-bin) cmd_install_bin ;;
  *) sed -n '2,17p' "${BASH_SOURCE[0]}"; exit 2 ;;
esac
