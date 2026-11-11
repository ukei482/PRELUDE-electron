# rice とつなぐ(Rust 版・NixOS)

バー・通知・OSD・トレイ・ロック・壁紙などのウィジェット類は rice の部品に任せる(`redesign-rust.md` の「環境に任せるもの」)。PRELUDE が持つのは自分の層(スペース・ブックマーク・タブ・paths)だけで、つなぐための出口と設定の断片を出す。これが移行の段階 6。

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
- `$mod+d` でランチャー(`services.prelude.launcher`。既定は `prelude items | fuzzel --dmenu | prelude open`。GPUI のランチャーを入れたら `prelude-launcher` に替える)
- タブ = 作業領域で、作業領域の名前はタブの番号。`$mod+数字`(sway の既定)でタブを切り替える
- 親のある窓(ダイアログ・ポップアップ)は浮かせる(`redesign-rust.md` の「PRELUDE を通さずに開いた窓」)。足したい決まりは `services.prelude.sway.extraConfig`
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

## この PC の rice(`nix/rice.nix`)
PRELUDE の外にある好みの部分は、モジュールではなく `nix/rice.nix` に置く。ほかの PC では丸ごと替えてよい。

- バーは waybar(`nix/rice/waybar.css`。色は `theme.toml` と手で合わせる)
  - 左: 今のタブの題名(押すとランチャー)と、今のペインの操作 `[ ◫ ]` 左右に分割・`[ ⊟ ]` 上下に分割(ランチャーを分割の向きで開き、出すものを選ぶ)・`[ × ]` 今のペインを閉じる(`prelude pane close`)。Electron 版のペインの見出しのボタンにあたる
  - 右: Wi-Fi・電池・音量・時計を押すと、通知とクイック設定(swaync)が開く。未読の通知があれば `[ * ]`(右クリックで通知を止める)。音量の右クリックで消音
- 通知とクイック設定は swaync(`nix/rice/swaync.css`)。Electron 版の通知センターとクイック設定にあたる
  - 通知は × (乗せると出る)で 1 つずつ、Clear all で全部消せる。Do not disturb、メディア、音量・明るさ
  - QUICK: Wi-Fi・Bluetooth の入/切、ネットワークの設定、スクリーンショット(範囲・画面全体。`~/Pictures` に保存してクリップボードへ)、PRELUDE の設定
  - SESSION: Lock・Sleep・Log out・Restart・Shut down
  - Wi-Fi の一覧と接続はトレイの nm-applet、Bluetooth の機器はトレイの blueman
- タイトルバーを出さない、sway の素のバーを隠す、キー配列、fcitx5、鍵の保管庫(`pam_kwallet_init`)
- **分割は sway の機能を使う:** `$mod+b`(次の窓を右へ)・`$mod+v`(次の窓を下へ)の後に開いた窓は、その場所のペインになり、配置はコアに戻る

## yazi(ファイラ)
PRELUDE の `files` のペインは、既定で端末の中の yazi を開く(`konsole --workdir {path} -e yazi {path}`)。ほかのファイラにするなら `~/.config/prelude/panes/files.toml`。

yazi で移ったフォルダを PRELUDE に伝えると、タブを開き直したときにそのフォルダで開く(伝えなければ、ブックマークに書いた場所から始まる)。PRELUDE が起動したペインには `PRELUDE_PANE` が入っているので、`prelude pane path` を呼ぶだけでよい。

```lua
-- ~/.config/yazi/init.lua(まだ試していない)
ps.sub("cd", function()
	if os.getenv("PRELUDE_PANE") then
		Command("prelude"):arg({ "pane", "path", tostring(cx.active.current.cwd) }):spawn()
	end
end)
```

## まだのもの
- GPUI の UI(`ui/`。サイドバー・ランチャー・ファイラ)の Nix のパッケージ。GPUI は Zed のリポジトリを git で使うので、`cargoLock.outputHashes` が要る。手元でのビルドが重い(下の「nix-daemon のビルドには上限が効かない」)ので、GitHub Actions でビルドしてキャッシュで配るとき(段階 8)にまとめる
- Hyprland の断片(裏側と一緒に)
- コンポジタ自身のタブの見た目を隠す断片(sway はタブを使わないので要らない。Hyprland のグループのバー)

## この PC の OS: NixOS

PRELUDE は OS を前提にしない(`redesign-rust.md` の「環境に任せるもの」)。これは作者の PC を NixOS にした理由と記録。

PRELUDE が頼っているのは **KDE Plasma 6 + Wayland(KWin)** で、Ubuntu そのものではない。Ubuntu に固有なのは snap 版の Chromium の扱いと、SDDM の設定ファイルの名前くらいで、どれも得をしていない。
そこで、乗る先を PRELUDE の思想に照らして選び直し、**NixOS** にする。ディストリビューションを自分で作ることはしない(カーネル・ドライバ・KDE の保守まで持つことになる)。

### 思想と照らす

| 思想 | 出どころ | NixOS では |
|---|---|---|
| 任せられるものは任せる。PRELUDE は自分の層だけを持つ | shell-design.md 1章、Web のペイン | KDE・Chromium・カーネルは nixpkgs が保守する。PRELUDE はモジュール 1 つ |
| ファイルには使用者が決めたことだけを書く。PRELUDE が変えるものは別に置く | スペースとブックマーク | OS 全体を 1 つの設定で宣言する。`.prelude` と同じ考え方 |
| 落ちても・失敗しても戻れる | コアと UI の境目、release.md | 前の世代へ戻せる(起動の画面でも選べる)。`release.sh switch` が OS ごとになる |
| 土台と付け替える部分を分ける | 全体の形・UI の付け替え | 読み取り専用の `/nix/store` と、使用者の設定 |
| ミニマル | 目的 | 書いたものしか入らない |

比べた候補: Kubuntu(手で積み重ねる・snap・Plasma が遅れる)、Fedora KDE、Arch(戻せない)、Fedora Kinoite + bootc(思想にはよく合い、移行中の Electron 版も普通に動く。NixOS が合わなかったときの次の候補)。

### PRELUDE 側で変えること

| 今 | NixOS では |
|---|---|
| `/usr/bin/konsole`・`/snap/bin/chromium`・`/usr/share/icons`・`/bin/bash` などの決め打ち | `PATH` と `XDG_DATA_DIRS` から探す(`sys/xdg.js`・`crates/prelude-core/src/xdg.rs`)。**済** |
| npm の `electron`(`node_modules/electron/dist/electron`) | NixOS では動かない(普通のファイルの配置が無い)。nixpkgs の `electron`(版を合わせる)で `electron .`。ネイティブモジュールは無いので、ほかの依存はそのまま |
| `scripts/theme.sh`(sudo で SDDM・ロック画面のテーマを置く、`quiet splash`) | NixOS のモジュールに書く(SDDM のテーマ・`boot.kernelParams` など) |
| `systemd/prelude-shell*.service`、`scripts/shell-mode.sh` | モジュールの `systemd.user.services` |
| `release.sh` + `sys/update.js`(タグから作って切り替え・前の版に戻す) | flake の入力をタグに合わせて `nixos-rebuild switch`。戻すのは NixOS の世代 |
| 「Kubuntu に戻る」 | 「plasmashell に戻る」(中身は同じ) |
| 以前の配布の案(GitHub Actions でビルドしてリリースに添付) | このリポジトリの `flake.nix` と NixOS のモジュール(`services.prelude.enable`)。ビルドの結果は Nix のキャッシュで配れる |

### 確かめること(NixOS を試すとき)

- nixpkgs の `electron` で今の Electron 版が動くか(版の違い)
- 日本語入力(`i18n.inputMethod` で fcitx5)、Iris Xe
- アプリの取り込み: NixOS では包まれたプログラムのプロセス名が `.konsole-wrapped` のようになる。`/proc` で窓の持ち主を見分ける所(`appembed.js`・`embed.rs`・`procs.rs`)が合うか
- KWin スクリプト・シェルモード・ロック画面と SDDM のテーマ
- `.desktop` の `Exec` が `/nix/store/…` の絶対パスになるアプリで、人からもらったブックマークの起動コマンドの確かめが働くか

### 移行の方針(2026-11-04)
PRELUDE の開発を止めても Ubuntu を使い続ける利点は薄いので、**試してから戻すのではなく、NixOS へ移る** つもりで進める。ただし Kubuntu はすぐ消さず、横に入れて確かめてから消す。
- `flake.nix` に開発環境、`nix/configuration.nix` に最小の OS の設定だけを書く。今の Kubuntu の中身は書き写さず、足りないものをその都度足す
- メモリの節約にはならない(この PC で使っているのはアプリで、減るのは Discover と snapd の 130MB ほど)。スワップの不足は zram で補う
- この PC では仮想マシンを使わない(メモリ 7.6GB に 3.5GB を割り当てて、OOM で落ちた)
- 2026-11-05: NixOS を `nvme0n1p3` に入れ、起動・ログイン・`nixos-rebuild` まで確かめた(Kubuntu は `nvme0n1p2` に残っている)。Claude のデスクトップアプリは公式の .deb を `nix/claude-desktop.nix` で包む(Kubuntu の上の Nix で起動を確認)。pCloud は nixpkgs にある。open-pdf-studio も公式の .deb を `nix/open-pdf-studio.nix` で包む(MCP は 9223 番で答える)。ペンタブレットのドライバ(xppenlinux)は入れない
- flake は `path:` で指さない(リポジトリ全体、ビルド結果の 16GB までストアにコピーされる)。`.#…` なら git の追跡分だけ
- nixos-26.05 の既定は rustc 1.95・electron 43 で、PRELUDE には古い。`flake.nix` で `rust-overlay` の配布済みの 1.98.1(GPUI の指定に合わせる)と `electron_44` を選ぶ
- Kubuntu の上の Nix で確かめた(`nix develop`。2026-11-04): コアとコマンドのテスト、`npm test`(47)、GPUI の 3 つのビルド(2 並列で 8 分半)が通り、ファイラは Nix の Mesa を `VK_DRIVER_FILES` で指定すると Iris Xe の Vulkan で動く。Electron 44 は PRELUDE の画面まで読み込む。Kubuntu の上では GL のドライバが見つからないエラーが出る(`/run/opengl-driver` が無いため。NixOS で確かめる)
- `nix-daemon` のビルドには `systemd-run` の上限が効かない。手元で Rust をコンパイルさせると OOM になるので、実行前に `nix build --dry-run` でビルドするものを見る
- `nix/hardware-configuration.nix` は入れる PC で作る。EFI 区画が 300MB なので、カーネルをルートの `/boot` に置く GRUB にする

### 順番

1. 今の Kubuntu のまま準備する: 決め打ちのパスを無くす(済)、この設計を書く(済)
2. NixOS を別の区画か USB に入れて、上の「確かめること」を試す。`flake.nix`(Electron 版・Rust 版のパッケージ、`nix develop`)はここで作る(NixOS の上で確かめないと動くか分からないため)
3. NixOS のモジュールを書き、シェルとして常用する。更新の仕組みを世代に置き換える
