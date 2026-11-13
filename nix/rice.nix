# この PC の rice(docs/rice.md)。PRELUDE の外にある、好みで決める部分。
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
    # 左: 今のタブの題名と、今のペインの操作(Electron 版のペインの見出しのボタン)。右: 押すと通知とクイック設定(swaync)
    modules-left = [ "custom/prelude" "custom/split-row" "custom/split-col" "custom/close-pane" ];
    modules-right = [ "network" "battery" "pulseaudio" "tray" "custom/notification" "clock" ];
    "custom/prelude" = {
      exec = "prelude watch --bar";
      return-type = "json";
      format = "{}";
      max-length = 100;
      on-click = "prelude-launcher";
    };
    # 分割は、ランチャーを分割の向きで開いて、出すもの(Web・フォルダ・アプリなど)を選ぶ
    "custom/split-row" = {
      format = "[ ◫ ]";
      tooltip-format = "Split left/right (choose what to open)";
      on-click = "prelude-launcher --split row";
    };
    "custom/split-col" = {
      format = "[ ⊟ ]";
      tooltip-format = "Split top/bottom (choose what to open)";
      on-click = "prelude-launcher --split col";
    };
    "custom/close-pane" = {
      format = "[ × ]";
      tooltip-format = "Close the current pane";
      on-click = "prelude pane close";
    };
    network = {
      format-wifi = "[ WIFI {essid} ]";
      format-ethernet = "[ ETHERNET ]";
      format-disconnected = "[ OFFLINE ]";
      tooltip-format = "{ifname} {ipaddr}";
      on-click = swayncPanel;
    };
    battery = {
      format = "[ BAT {capacity}% ]";
      format-charging = "[ BAT {capacity}% CHG ]";
      format-full = "[ BAT FULL ]";
      states.warning = 15;
      on-click = swayncPanel;
    };
    pulseaudio = {
      format = "[ VOL {volume}% ]";
      format-muted = "[ MUTE ]";
      on-click = swayncPanel;
      on-click-right = "${pkgs.wireplumber}/bin/wpctl set-mute @DEFAULT_AUDIO_SINK@ toggle";
    };
    tray.spacing = 8;
    # 未読の通知があれば * を出す(Electron 版の時計の先頭の *)。通知を止めていれば DND
    "custom/notification" = {
      exec = "${swaync}/bin/swaync-client -swb";
      return-type = "json";
      format = "{icon}";
      format-icons = {
        notification = "[ * ]";
        none = "";
        dnd-notification = "[ DND * ]";
        dnd-none = "[ DND ]";
        inhibited-notification = "[ * ]";
        inhibited-none = "";
        dnd-inhibited-notification = "[ DND * ]";
        dnd-inhibited-none = "[ DND ]";
      };
      tooltip = false;
      on-click = swayncPanel;
      on-click-right = "${swaync}/bin/swaync-client -d -sw";
    };
    clock = {
      format = "[ {:%a %m/%d %H:%M} ]";
      locale = "C";
      tooltip = false;
      on-click = swayncPanel;
    };
  };

  # 通知とクイック設定(Electron 版の通知センター・クイック設定)。バーの Wi-Fi・電池・音量・時計を押すと開く。
  # 通知は × で 1 つずつ、Clear All で全部消せる。Wi-Fi の一覧はトレイの nm-applet、Bluetooth の機器はトレイの blueman から
  swaync = pkgs.swaynotificationcenter;
  swayncPanel = "${swaync}/bin/swaync-client -t -sw";
  sh = cmd: "${pkgs.bash}/bin/sh -c ${lib.escapeShellArg cmd}";
  # パネルを閉じてから動かす(範囲を選ぶスクリーンショット・ロックの画面がパネルの下にならないように)
  closeThen = cmd: sh "${swaync}/bin/swaync-client -cp; sleep 0.3; ${cmd}";
  shot = geometry: closeThen ''mkdir -p "$HOME/Pictures" && f="$HOME/Pictures/screenshot-$(date +%Y%m%d-%H%M%S).png" && ${pkgs.grim}/bin/grim ${geometry} "$f" && ${pkgs.wl-clipboard}/bin/wl-copy < "$f"'';
  swayncConfig = {
    "$schema" = "${swaync}/etc/xdg/swaync/configSchema.json";
    positionX = "right";
    positionY = "top";
    control-center-width = 420;
    notification-window-width = 420;
    timeout = 10;
    timeout-low = 5;
    timeout-critical = 0;
    hide-on-clear = false;
    text-empty = "No notifications";
    widgets = [ "title" "dnd" "notifications" "mpris" "volume" "backlight" "label#quick" "buttons-grid#quick" "label#session" "buttons-grid#session" ];
    widget-config = {
      title = { text = "NOTIFICATIONS"; clear-all-button = true; button-text = "Clear all"; };
      dnd.text = "Do not disturb";
      mpris = { image-size = 48; autohide = true; };
      volume.label = "VOL";
      backlight = { label = "BRT"; device = "intel_backlight"; min = 1; };
      "label#quick" = { text = "QUICK"; max-lines = 1; };
      "buttons-grid#quick" = {
        buttons-per-row = 3;
        actions = [
          {
            label = "Wi-Fi";
            type = "toggle";
            command = sh ''[ "$SWAYNC_TOGGLE_STATE" = true ] && nmcli radio wifi on || nmcli radio wifi off'';
            update-command = sh ''[ "$(nmcli radio wifi)" = enabled ] && echo true || echo false'';
          }
          {
            label = "Bluetooth";
            type = "toggle";
            command = sh ''[ "$SWAYNC_TOGGLE_STATE" = true ] && bluetoothctl power on || bluetoothctl power off'';
            update-command = sh ''bluetoothctl show 2>/dev/null | grep -q "Powered: yes" && echo true || echo false'';
          }
          { label = "Networks"; command = closeThen "${pkgs.networkmanagerapplet}/bin/nm-connection-editor"; }
          { label = "Shot area"; command = shot ''-g "$(${pkgs.slurp}/bin/slurp)"''; }
          { label = "Shot screen"; command = shot ""; }
          { label = "Settings"; command = closeThen "prelude new settings"; }
        ];
      };
      "label#session" = { text = "SESSION"; max-lines = 1; };
      "buttons-grid#session" = {
        buttons-per-row = 3;
        actions = [
          { label = "Lock"; command = closeThen "${pkgs.swaylock}/bin/swaylock -f -c 524832"; }
          { label = "Sleep"; command = closeThen "systemctl suspend"; }
          { label = "Log out"; command = "swaymsg exit"; }
          { label = "Restart"; command = "systemctl reboot"; }
          { label = "Shut down"; command = "systemctl poweroff"; }
        ];
      };
    };
  };
in
{
  fonts.packages = [ fonts ];

  environment.systemPackages = [ pkgs.waybar swaync pkgs.networkmanagerapplet ];
  environment.etc."xdg/waybar/config".text = builtins.toJSON waybar;
  environment.etc."xdg/waybar/style.css".source = ./rice/waybar.css;
  environment.etc."xdg/swaync/config.json".text = builtins.toJSON swayncConfig;
  # swaync は既定の見た目を先に読むので、ここは PRELUDE の色に上書きする分だけ
  environment.etc."xdg/swaync/style.css".source = ./rice/swaync.css;

  environment.etc."sway/config.d/rice.conf".text = ''
    # この PC の rice(nix/rice.nix)

    # タイトルバーは出さない(ペインの題名はサイドバーに出る)
    default_border pixel 2
    default_floating_border pixel 2

    # sway の素のバーの代わりに waybar を出す
    bar bar-0 mode invisible
    exec ${pkgs.waybar}/bin/waybar
    # 通知とクイック設定(mako の代わり。× で消せて、履歴が残る)
    exec ${swaync}/bin/swaync

    # Wi-Fi の一覧(トレイ)。sway は xdg の自動起動を動かさない
    exec ${pkgs.networkmanagerapplet}/bin/nm-applet --indicator

    # Bluetooth のトレイ。sway は xdg の自動起動を動かさない
    exec ${pkgs.blueman}/bin/blueman-applet

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
