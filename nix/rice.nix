# この PC の rice(docs/redesign-rust.md の判断 10)。PRELUDE の外にある、好みで決める部分。
# PRELUDE に要るものは nix/module.nix に置き、見た目・バー・通知・キー配列などはここに置く。
# sway は /etc/sway/config.d の断片を名前の順に読むので、prelude.conf の後に rice.conf が効く。
{ config, lib, pkgs, ... }:
let
  im = config.i18n.inputMethod;
  # Electron 版と同じ書体(src/fonts)。GPUI の UI は実行ファイルに持っているが、waybar などは OS から探す
  fonts = pkgs.runCommand "prelude-fonts" { } ''
    install -Dm644 -t $out/share/fonts/truetype ${../src/fonts}/*.ttf
  '';
  waybar = {
    layer = "top";
    position = "top";
    height = 40;
    spacing = 14;
    modules-left = [ "custom/prelude" ];
    modules-right = [ "network" "battery" "pulseaudio" "tray" "clock" ];
    "custom/prelude" = {
      exec = "prelude watch --bar";
      return-type = "json";
      format = "{}";
      max-length = 100;
    };
    network = {
      format-wifi = "[ WIFI {essid} ]";
      format-ethernet = "[ ETHERNET ]";
      format-disconnected = "[ OFFLINE ]";
      tooltip-format = "{ifname} {ipaddr}";
    };
    battery = {
      format = "[ BAT {capacity}% ]";
      format-charging = "[ BAT {capacity}% CHG ]";
      format-full = "[ BAT FULL ]";
      states.warning = 15;
    };
    pulseaudio = {
      format = "[ VOL {volume}% ]";
      format-muted = "[ MUTE ]";
      on-click = "${pkgs.wireplumber}/bin/wpctl set-mute @DEFAULT_AUDIO_SINK@ toggle";
    };
    tray.spacing = 8;
    clock = {
      format = "[ {:%a %m/%d %H:%M} ]";
      locale = "C";
      tooltip = false;
    };
  };
in
{
  fonts.packages = [ fonts ];

  environment.systemPackages = [ pkgs.waybar pkgs.mako ];
  environment.etc."xdg/waybar/config".text = builtins.toJSON waybar;
  environment.etc."xdg/waybar/style.css".source = ./rice/waybar.css;

  environment.etc."sway/config.d/rice.conf".text = ''
    # この PC の rice(nix/rice.nix)

    # タイトルバーは出さない(ペインの題名はサイドバーに出る)
    default_border pixel 2
    default_floating_border pixel 2

    # sway の素のバーの代わりに waybar を出す
    bar bar-0 mode invisible
    exec ${pkgs.waybar}/bin/waybar
    exec ${pkgs.mako}/bin/mako

    # キー配列。sway は services.xserver.xkb を読まない
    input type:keyboard xkb_layout "${config.services.xserver.xkb.layout}"
    ${lib.optionalString (im.enable && im.type == "fcitx5") ''

      # 入力メソッド。sway は xdg の自動起動を動かさない
      exec ${im.package}/bin/fcitx5 -d --replace''}
    ${lib.optionalString config.security.pam.services.login.kwallet.enable ''

      # 鍵の保管庫(gh・ブラウザなどのログイン情報)。ログインで開いた KWallet に、セッションをつなぐ。
      # Plasma では xdg の自動起動がしている。これが無いと org.freedesktop.secrets が現れない
      exec ${pkgs.kdePackages.kwallet-pam}/libexec/pam_kwallet_init''}
  '';
}
