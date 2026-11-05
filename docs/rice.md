# rice とつなぐ(Rust 版・NixOS)

判断 10 で、バー・通知・OSD・トレイ・ロック・壁紙などのウィジェット類は rice の部品に任せることにした。PRELUDE が持つのは自分の層(スペース・ブックマーク・タブ・paths)だけで、つなぐための出口と設定の断片を出す。これが移行の段階 6。

## NixOS のモジュール(`nix/module.nix`)

```nix
programs.sway = { enable = true; package = pkgs.swayfx; };
services.prelude.enable = true;   # flake の nixosModules.default が package を決める
```

入るもの:

| もの | 中身 |
|---|---|
| `prelude-core`・`prelude` | flake の `packages.prelude`(`nix/prelude.nix`。`crates/` のワークスペース)。Chromium も入る(Web のペイン) |
| `prelude-core.service` | systemd の user サービス。sway の断片が起動する(Plasma では起動しない)。落ちたときは起動したアプリを道連れにしない(`KillMode=process`) |
| `/etc/sway/config.d/prelude.conf` | sway の設定の断片(下)。`services.prelude.sway.enable`(既定は `programs.sway.enable`) |
| ダイアログ | sway のセッションだけ、アプリの「開く」「保存」を PRELUDE にする(`xdg.portal.config.sway`)。`services.prelude.portal.enable = false` で外す |

### GPUI の UI
`services.prelude.ui.enable = true` で、サイドバー・ランチャー・ファイラが入る。サイドバーを起動しないなら `services.prelude.ui.sidebar = false`。ランチャーは `services.prelude.launcher` で替えられる(fuzzel などのプログラムでよい)

### sway の断片
- sway の起動のときに、セッションの環境(`PATH` も)を systemd に渡してから `prelude-core.service` を restart する。sway が終わったらコアを止め、`SWAYSOCK` を消す
- 鍵の保管庫は、ログインで開いた KWallet をそのまま使う(`pam_kwallet_init` を起動する)
- タイトルバーは出さない(`default_border pixel 2`)。`services.prelude.sway.extraConfig` で上書きできる
- キー配列は `services.xserver.xkb.layout`、入力メソッドは `i18n.inputMethod` の fcitx5 を sway でも使う(sway はどちらも自分では読まない)
- `$mod+d` でランチャー(`services.prelude.launcher`。既定は `prelude items | fuzzel --dmenu | prelude open`。GPUI のランチャーを入れたら `prelude-launcher` に替える)
- タブ = 作業領域で、作業領域の名前はタブの番号。`$mod+数字`(sway の既定)でタブを切り替える
- 親のある窓(ダイアログ・ポップアップ)は浮かせる(判断 12)。足したい決まりは `services.prelude.sway.extraConfig`
- `~/.config/sway/config` を自分で書いたときは、`/etc/sway/config.d/*` を include する(sway の既定の設定はしている)。`$mod` もそこで決める

## バー: `prelude watch --bar`

状態が変わるたびに 1 行の JSON を出す。コアが動いていなければ `class: "offline"` の空の行を出し、2 秒ごとにつなぎ直す。

```json
{"text":"Main  2 Konsole","tooltip":"*  2  Konsole\n   1  example.com","class":"online","alt":"Main"}
```

- `text`: スペースの名前と、今のタブの番号・題名(今のペインの窓の題名。pango の文字は逃がしてある)
- `tooltip`: タブの一覧(`*` が今のタブ)
- `class`: `online` / `offline`、`alt`: スペースの名前

waybar の例(タブの番号は `sway/workspaces` が出すので、PRELUDE の側はスペースと題名を出す):

```jsonc
"modules-left": ["sway/workspaces", "custom/prelude"],
"custom/prelude": {
  "exec": "prelude watch --bar",
  "return-type": "json",
  "on-click": "prelude items | fuzzel --dmenu | prelude open"
}
```

ほかのバー(eww・i3blocks など)は、`prelude watch --bar` の行をそのまま読むか、全部の状態が要るなら `prelude watch`(`state` のイベントの JSON)を読む。

## まだのもの
- Hyprland の断片(裏側と一緒に)
- コンポジタ自身のタブの見た目を隠す断片(sway はタブを使わないので要らない。Hyprland のグループのバー)
