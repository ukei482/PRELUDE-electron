# NixOS の設定(最小)。開発に要るものだけを書き、足りないアプリは environment.systemPackages に足していく
{ pkgs, ... }:
{
  system.stateVersion = "26.05";
  nix.settings.experimental-features = [ "nix-command" "flakes" ];
  nixpkgs.config.allowUnfree = true; # Claude・Discord・Chrome など

  # Claude のデスクトップアプリが取ってくる claude-code(普通の Linux 向けのバイナリ)を動かす
  programs.nix-ld.enable = true;

  # 起動。Kubuntu と並べている間は、GRUB が両方を出す。
  # EFI 区画が 300MB しかないので、カーネルはルートの /boot に置く(systemd-boot は EFI 区画に置くので使わない)
  boot.loader.efi.canTouchEfiVariables = true;
  boot.loader.efi.efiSysMountPoint = "/boot/efi";
  boot.loader.grub = {
    enable = true;
    device = "nodev";
    efiSupport = true;
    useOSProber = true;
    configurationLimit = 10;
  };

  # メモリが 7.6GB と少ないので、メモリの圧縮でしのぐ
  zramSwap = { enable = true; memoryPercent = 50; };

  networking.hostName = "ukeiyamada-cflv11";
  networking.networkmanager.enable = true;
  time.timeZone = "Asia/Tokyo";

  # 日本語
  i18n.defaultLocale = "ja_JP.UTF-8";
  console.keyMap = "jp106";
  services.xserver.xkb.layout = "jp";
  i18n.inputMethod = {
    enable = true;
    type = "fcitx5";
    fcitx5.waylandFrontend = true;
    fcitx5.addons = [ pkgs.fcitx5-mozc ];
  };
  fonts.packages = [ pkgs.noto-fonts-cjk-sans pkgs.noto-fonts-color-emoji ];

  # GPU のドライバ(/run/opengl-driver)。GPUI・Electron が使う
  hardware.graphics.enable = true;

  # 画面(今と同じ KDE Plasma の Wayland)
  services.desktopManager.plasma6.enable = true;
  services.displayManager.sddm.enable = true;
  services.displayManager.sddm.wayland.enable = true;

  environment.systemPackages = with pkgs; [
    (callPackage ./claude-desktop.nix { })
    chromium
    claude-code
    discord
    gh
    git
    google-chrome
    kdePackages.dolphin
    kdePackages.kate
    kdePackages.konsole
    krita
    libreoffice
    mypaint
    pcloud
    rsync
    zed-editor
    zoom-us
  ];

  # パスワードはここに書かない。入れた後に `nixos-enter --root /mnt -c 'passwd ukeiyamada'` で決める
  users.users.ukeiyamada = {
    isNormalUser = true;
    extraGroups = [ "wheel" "networkmanager" "video" ];
  };
}
