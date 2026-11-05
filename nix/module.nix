# PRELUDE の NixOS モジュール(docs/redesign-rust.md の判断 6 の段階 6・判断 9)。
# PRELUDE は自分の層(コア・コマンド・ダイアログ)だけを持ち、バー・通知などは rice に任せる。ここで出すのは、つなぐための断片:
#   - コアの systemd の user サービス(prelude-core.service)
#   - GPUI の UI(サイドバー・ランチャー・ファイラ)
#   - sway の設定の断片(/etc/sway/config.d/prelude.conf): コアの起動・ランチャーのキー・窓の決まり
#   - アプリのダイアログのポータル(sway のセッションだけ。Plasma のダイアログは変えない)
# バーには `prelude watch --bar` をつなぐ(waybar の custom モジュール。docs/rice.md)
{ config, lib, pkgs, ... }:
let
  cfg = config.services.prelude;
  im = config.i18n.inputMethod;
  systemctl = "${config.systemd.package}/bin/systemctl";
  # sway のセッションの始まりと終わり。セッションの環境(WAYLAND_DISPLAY・SWAYSOCK・PATH・入力メソッドなど)を systemd に渡し、
  # 前のセッションのコアが残っていても入れ替わるように restart する。コアは SWAYSOCK を見て sway の裏側を使う。
  # sway が終わったらコアを止め、SWAYSOCK を消す(user のサービスの環境はログアウトしても残り、Plasma のアプリにも渡ってしまう)
  swaySession = pkgs.writeShellScript "prelude-sway-session" ''
    ${pkgs.dbus}/bin/dbus-update-activation-environment --systemd --all
    ${systemctl} --user restart prelude-core.service
    ${config.programs.sway.package}/bin/swaymsg -t subscribe '["shutdown"]' >/dev/null 2>&1
    ${systemctl} --user stop prelude-core.service
    ${systemctl} --user unset-environment SWAYSOCK I3SOCK
  '';
  swayConf = ''
    # PRELUDE(services.prelude が置く。nix/module.nix)
    # コアを sway と一緒に起動し、sway と一緒に止める
    exec ${swaySession}

    # タイトルバーは出さない(ペインの題名はサイドバーに出る)。残したいときは sway.extraConfig で上書きする
    default_border pixel 2
    default_floating_border pixel 2

    # キー配列。sway は services.xserver.xkb を読まない
    input type:keyboard xkb_layout "${config.services.xserver.xkb.layout}"
    ${lib.optionalString (im.enable && im.type == "fcitx5") ''

      # 入力メソッド。sway は xdg の自動起動を動かさない
      exec ${im.package}/bin/fcitx5 -d --replace''}

    ${lib.optionalString config.security.pam.services.login.kwallet.enable ''
      # 鍵の保管庫(gh・ブラウザなどのログイン情報)。ログインで開いた KWallet に、セッションをつなぐ。
      # Plasma では xdg の自動起動がしている。これが無いと org.freedesktop.secrets が現れない
      exec ${pkgs.kdePackages.kwallet-pam}/libexec/pam_kwallet_init''}

    # ランチャー
    bindsym --no-warn $mod+d exec ${cfg.launcher}
    ${lib.optionalString (cfg.ui.enable && cfg.ui.sidebar) ''

      # サイドバー(画面の左端に貼り付く。コアが動いていなければ、つながるまで待つ)
      exec ${cfg.ui.package}/bin/prelude-sidebar''}

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
      default = if cfg.ui.enable then "prelude-launcher" else "prelude items | ${pkgs.fuzzel}/bin/fuzzel --dmenu | prelude open";
      defaultText = lib.literalExpression ''"prelude-launcher"(UI を入れないときは fuzzel)'';
      description = "$mod+d で開くランチャー";
    };

    ui = {
      enable = lib.mkEnableOption "GPUI の UI(サイドバー・ランチャー・ファイラ)。ビルドが重い";
      package = lib.mkOption {
        type = lib.types.package;
        description = "prelude-sidebar・prelude-launcher・prelude-filer の入ったパッケージ(flake の packages.prelude-ui)";
      };
      sidebar = lib.mkOption {
        type = lib.types.bool;
        default = true;
        description = "sway と一緒にサイドバーを起動する。自分のバーだけで使うなら外す";
      };
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
    environment.systemPackages = [ cfg.package pkgs.chromium ] ++ lib.optional cfg.ui.enable cfg.ui.package;

    systemd.user.services.prelude-core = {
      description = "PRELUDE のコア";
      # ふだんは sway の断片が起動する。Plasma では Electron 版が動くので、ここでは起動しない
      partOf = [ "graphical-session.target" ];
      # NixOS が既定で付ける PATH(coreutils など)を外し、セッションの PATH を受け継ぐ。
      # コアが起動するアプリ・Chromium は PATH から探す
      path = lib.mkForce [ ];
      # 反映でコアを再起動しない。コアは止まるときに起動したアプリを閉じるので、sway の中の nixos-rebuild で作業が消えてしまう。
      # 新しいコアは次の sway のセッションから動く
      restartIfChanged = false;
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
