#!/usr/bin/env bash
# NixOS で実行する: Kubuntu のホームのデータを、NixOS のホームへ読むだけでコピーする(Kubuntu 側は変えない)。
# 作り直せるもの(キャッシュ・ビルド結果)と Kubuntu 固有のものは持ってこない。NixOS の方が新しいファイルは残す
set -euo pipefail
SRC=/run/media/kubuntu
[ "$(id -u)" != 0 ] || { echo "sudo なしで実行してください" >&2; exit 1; }
sudo mkdir -p "$SRC"
findmnt "$SRC" >/dev/null || sudo mount -o ro /dev/disk/by-label/kubuntu_2604 "$SRC"
rsync -aHAX --update --info=progress2 \
  --exclude='/.cache/' --exclude='/.npm/' --exclude='/.cargo/registry/' --exclude='/.cargo/git/' --exclude='/.rustup/' \
  --exclude='/nixos-vm/' --exclude='/.local/share/prelude/releases/' --exclude='/.local/share/Trash/' \
  --exclude='/pCloudDrive/' --exclude='/.pcloud/' --exclude='target/' --exclude='node_modules/' \
  --exclude='/PRELUDE-electron/.claude/worktrees/' --exclude='/.nix-profile' --exclude='/.nix-defexpr' --exclude='/.nix-channels' \
  "$SRC/home/$USER/" "$HOME/"
echo "コピーしました。Kubuntu 側は変えていません"
