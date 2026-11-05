# PRELUDE の NixOS モジュール(docs/redesign-rust.md の判断 6 の段階 6・判断 9)。
# PRELUDE は自分の層(コア・コマンド・ダイアログ)だけを持ち、バー・通知などは rice に任せる。ここで出すのは、つなぐための断片:
#   - コアの systemd の user サービス(prelude-core.service)
#   - sway の設定の断片(/etc/sway/config.d/prelude.conf): コアの起動・ランチャーのキー・窓の決まり
#   - アプリのダイアログのポータル(sway のセッションだけ。Plasma のダイアログは変えない)
# バーには `prelude watch --bar` をつなぐ(waybar の custom モジュール。docs/rice.md)
{ config, lib, pkgs, ... }:
let
  cfg = config.services.prelude;
  swayConf = ''
    # PRELUDE(services.prelude が置く。nix/module.nix)
    # コアを sway と一緒に起動する。セッションの環境(WAYLAND_DISPLAY・SWAYSOCK・入力メソッドなど)を systemd に渡し、
    # 前のセッションのコアが残っていても入れ替わるように restart する。コアは SWAYSOCK を見て sway の裏側を使う
    exec ${pkgs.dbus}/bin/dbus-update-activation-environment --systemd --all && systemctl --user restart prelude-core.service

    # ランチャー
    bindsym --no-warn $mod+d exec ${cfg.launcher}

    # PRELUDE のタブ = 作業領域(名前はタブの番号)。$mod+数字(sway の既定のキー)でタブを切り替える

    # 親のある窓(ダイアログ・ログインのポップアップ・確認の窓)は浮かせる。タブのペインにはしない(判断 12)
    for_window [window_type="dialog"] floating enable
    for_window [window_role="dialog"] floating enable
    for_window [window_role="pop-up"] floating enable
    for_window [window_role="bubble"] floating enable

    ${cfg.sway.extraConfig}
  '';
in
{
  options.services.prelude = {
    enable = lib.mkEnableOption "PRELUDE(コア・コマンド・sway の設定の断片)";

    package = lib.mkOption {
      type = lib.types.package;
      description = "prelude-core と prelude の入ったパッケージ(flake の packages.prelude)";
    };

    launcher = lib.mkOption {
      type = lib.types.str;
      default = "prelude items | ${pkgs.fuzzel}/bin/fuzzel --dmenu | prelude open";
      example = "prelude-launcher";
      description = "$mod+d で開くランチャー。GPUI のランチャー(ui/prelude-launcher)を入れたら、それに替える";
    };

    sway = {
      enable = lib.mkOption {
        type = lib.types.bool;
        default = config.programs.sway.enable;
        defaultText = lib.literalExpression "config.programs.sway.enable";
        description = "sway の設定の断片(/etc/sway/config.d/prelude.conf)を置く。~/.config/sway/config を自分で書いたときは、そこに `include /etc/sway/config.d/*` が要る";
      };
      extraConfig = lib.mkOption {
        type = lib.types.lines;
        default = "";
        description = "断片の最後に足す sway の設定(窓ごとの例外など)";
      };
    };

    portal.enable = lib.mkOption {
      type = lib.types.bool;
      default = true;
      description = "sway のセッションで、アプリの「開く」「保存」のダイアログを PRELUDE にする(最初のフォルダがタブの paths になる)";
    };
  };

  config = lib.mkIf cfg.enable {
    # Web のペインは Chromium そのもの(判断 3-b。無いときの扱いは作らない)
    environment.systemPackages = [ cfg.package pkgs.chromium ];

    systemd.user.services.prelude-core = {
      description = "PRELUDE のコア";
      # ふだんは sway の断片が起動する。Plasma では Electron 版が動くので、ここでは起動しない
      partOf = [ "graphical-session.target" ];
      serviceConfig = {
        ExecStart = "${cfg.package}/bin/prelude-core";
        Restart = "on-failure";
        RestartSec = 2;
        # 止めるときに起動したアプリを閉じるのはコア自身。落ちたときは、アプリを道連れにしない(保存していない作業があるかもしれない)
        KillMode = "process";
      };
    };

    environment.etc."sway/config.d/prelude.conf" = lib.mkIf cfg.sway.enable { text = swayConf; };

    xdg.portal = lib.mkIf (cfg.portal.enable && cfg.sway.enable) {
      enable = true;
      extraPortals = [ cfg.package pkgs.xdg-desktop-portal-gtk ];
      config.sway = {
        "org.freedesktop.impl.portal.FileChooser" = [ "prelude" ];
      };
    };
  };
}
