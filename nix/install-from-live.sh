#!/usr/bin/env bash
# Kubuntu のライブ USB から、NixOS を今の Kubuntu の横に入れる(docs/redesign-rust.md の判断 9)。
# 動いている Kubuntu のディスクは縮められないので、ライブ USB で起動して実行する。
#   sudo bash install-from-live.sh shrink    Kubuntu の区画を縮めて、NixOS の区画を作る
#   sudo bash install-from-live.sh install   NixOS を入れる
set -euo pipefail

DISK=/dev/nvme0n1
ESP=${DISK}p1
KUBUNTU=${DISK}p2
NIXOS=${DISK}p3
# Kubuntu の区画の終わり。使っているのは 72GB ほど
END=130GiB
FS_SHRINK=120G
FLAKE_DIR="$(cd "$(dirname "$0")/.." && pwd)"

die() { echo "止めました: $*" >&2; exit 1; }
[ "$(id -u)" = 0 ] || die "sudo で実行してください"
[ -b "$KUBUNTU" ] || die "$KUBUNTU がありません"
# 今の Kubuntu の上では動かさない
[ "$(findmnt -no SOURCE /)" != "$KUBUNTU" ] || die "動いている Kubuntu の上です。ライブ USB で起動してください"
findmnt -S "$KUBUNTU" >/dev/null && die "$KUBUNTU がマウントされています。外してください"
[ "$(blkid -o value -s LABEL "$KUBUNTU")" = kubuntu_2604 ] || die "$KUBUNTU のラベルが kubuntu_2604 ではありません"

confirm() {
  echo "$1"
  read -r -p "進めるなら yes と入力: " a
  [ "$a" = yes ] || die "取りやめました"
}

case "${1:-}" in
  shrink)
    [ ! -b "$NIXOS" ] || die "$NIXOS はもうあります"
    parted -s "$DISK" unit GiB print
    confirm "$KUBUNTU を $END までに縮め、残りに NixOS の区画 $NIXOS を作ります"
    e2fsck -f -y "$KUBUNTU"
    # 区画より先にファイルシステムを小さくする
    resize2fs "$KUBUNTU" "$FS_SHRINK"
    printf 'Yes\n' | parted ---pretend-input-tty "$DISK" resizepart 2 "$END"
    resize2fs "$KUBUNTU"
    e2fsck -f -y "$KUBUNTU"
    parted -s "$DISK" mkpart nixos ext4 "$END" 100%
    partprobe "$DISK"
    udevadm settle
    mkfs.ext4 -q -L nixos "$NIXOS"
    parted -s "$DISK" unit GiB print
    echo "できました。次は install です"
    ;;
  install)
    [ "$(blkid -o value -s LABEL "$NIXOS")" = nixos ] || die "$NIXOS がありません。先に shrink を"
    confirm "$NIXOS に NixOS を入れ、起動の順番の先頭を NixOS にします(Kubuntu は GRUB の一覧から起動できます)"
    mount "$NIXOS" /mnt
    mkdir -p /mnt/boot/efi /mnt/etc/nixos
    mount "$ESP" /mnt/boot/efi
    # ライブ環境に Nix を入れる(ダウンロードは /mnt に置いて、メモリを使い切らないようにする)
    if ! command -v nix >/dev/null && [ ! -x /nix/var/nix/profiles/default/bin/nix ]; then
      sh <(curl --proto '=https' --tlsv1.2 -L https://nixos.org/nix/install) --daemon --yes
    fi
    export PATH=/nix/var/nix/profiles/default/bin:$PATH
    N="nix --extra-experimental-features nix-command --extra-experimental-features flakes"
    cp -r "$FLAKE_DIR/flake.nix" "$FLAKE_DIR/flake.lock" "$FLAKE_DIR/nix" /mnt/etc/nixos/
    TOOLS="$($N build --no-link --print-out-paths --inputs-from path:/mnt/etc/nixos nixpkgs#nixos-install-tools)"
    "$TOOLS/bin/nixos-generate-config" --root /mnt --show-hardware-config > /mnt/etc/nixos/nix/hardware-configuration.nix
    PATH="$TOOLS/bin:$PATH" nixos-install --no-root-passwd --no-channel-copy --flake "path:/mnt/etc/nixos#prelude"
    echo "使用者のパスワードを決めてください"
    PATH="$TOOLS/bin:$PATH" nixos-enter --root /mnt -c 'passwd ukeiyamada'
    umount /mnt/boot/efi /mnt
    echo "できました。USB を抜いて再起動してください"
    ;;
  *)
    echo "usage: sudo bash install-from-live.sh shrink|install" >&2
    exit 2
    ;;
esac
