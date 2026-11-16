# Omarchy で PRELUDE を使う

Omarchy(Arch + Hyprland + Quickshell の `omarchy-shell`。4.0 から)の上で、Rust 版の PRELUDE(コア・`prelude` コマンド)を動かす手順。
設計は [redesign-rust.md](redesign-rust.md) の「Omarchy から学んだこと」「窓のつなぎ」。

## できること

| どこ | 何 |
|---|---|
| **バー** | 左(作業領域の右)に PRELUDE の部品。スペースと今のタブの題名を出し、乗せるとタブの一覧。**左クリックで PRELUDE のランチャー**(タブ・ブックマーク・アプリ・スペースを Omarchy のメニューで選んで開く)、中クリックで AI のペイン、右クリックで PRELUDE のメニュー |
| **メニュー** | Omarchy のメニューに「PRELUDE」(開く・AI・ファイラ・ブックマークに保存・設定・コアの起動し直し)。`omarchy menu summon prelude` でも開く |
| **キー** | `Super + Alt + P` で PRELUDE のランチャー(Omarchy の既定のキーと重ならない) |
| **窓** | Hyprland のつなぎ(`crates/prelude-core/src/hyprland.rs`)。**タブ = 作業領域**。ブックマークを開くと、アプリがタブの番号の作業領域に開く(`Super + 数字` でそのまま行き来できる)。並べ方・大きさ・浮かせ方は Omarchy に任せ、PRELUDE は並べ直さない |
| **AI** | `prelude skill install` で `~/.agents/skills` にスキルを置く(Omarchy の AI 全部が読む) |

## 入れる

Omarchy の PC で、このリポジトリを置いたフォルダから:

```bash
sudo pacman -S --needed rust
```

```bash
scripts/omarchy.sh install
```

- コア(`prelude-core`)と `prelude` コマンドをビルドして `~/.local/bin` に入れる。ビルド済みなら `--no-build`
- 設定が無ければ `~/.config/prelude/settings.toml` に `profile = "omarchy"` を書く(窓は「作業領域だけ指定」、端末は foot、PRELUDE を通さずに開いた窓は引き取らない)
- バーの部品を `~/.config/omarchy/plugins/prelude.bar/` に複写し、`omarchy plugin enable prelude.bar left --after omarchy.workspaces` で並べる
- メニューの項目を `~/.config/omarchy/extensions/omarchy-menu.jsonc` に、キーを `~/.config/hypr/bindings.lua` に、コアの自動起動を `~/.config/hypr/autostart.lua` に足す。どれも `PRELUDE >>>` と `PRELUDE <<<` の印の間だけで、流し直しても 1 つ
- Hyprland の中で流せば、その場でコアも起動する

確かめる: `scripts/omarchy.sh status`。外す: `scripts/omarchy.sh uninstall`(`~/.config/prelude` の設定・スペースは残す)。

バーの部品の置き場所は、バーの上でドラッグするか `omarchy bar move prelude.bar --section centerBegin`。部品の設定は `omarchy bar set prelude.bar maxWidth 400`(題名の最大の幅)・`launcher <コマンド>`(左クリック)。

## 窓のつなぎ(段 1)

設定の `windows.level` の上限は Hyprland では `workspace`(段 1)。`profile = "omarchy"` もそれを選ぶ。

- アプリは Hyprland の exec に作業領域を付けて起動する(Lua の `hl.dsp.exec_cmd(…, { workspace = "N silent" })`。前の Hyprland なら `exec [workspace N silent] …`)。作業領域は選ばない
- 出てきた窓は、起動したプロセスの環境変数 `PRELUDE_PANE`(Web のペインは題名の印)でペインに結び付ける。既に動いているブラウザが開いた窓のように exec の決まりが効かなかった窓だけ、タブの作業領域へ移す
- Hyprland で作業領域を移ると、PRELUDE の今のタブも変わる(読むだけ)。PRELUDE でタブを選ぶ・開くと、その作業領域へ移る
- 浮いた窓・PRELUDE を通さずに開いた窓には触らない。間・角丸・窓の決まり(`windows.gaps_*`・`corner_radius`)も Hyprland では当てはめない(見た目は Omarchy のもの)
- 分割で開く・ブックマークの配置どおりに並べる(段 2 から)は、Hyprland ではまだ。hy3 のつなぎを書けば段 2 以上にできる

コアを試すときは、ふだんのコアと別にする(タブの作業領域の名前に印が付くので、ふだんの作業領域と取り合わない):

```bash
PRELUDE_CORE_SOCK=/tmp/prelude-test.sock ~/.local/bin/prelude-core --state /tmp/prelude-test.json --spaces /tmp/prelude-spaces
```

## 前の Omarchy(3.x。waybar)

`scripts/omarchy.sh` は `~/.config/hypr/hyprland.conf` があればそこにキーと自動起動を足す。バーは waybar なので、`~/.config/waybar/config.jsonc` に手で足す:

```jsonc
"custom/prelude": {
  "exec": "prelude watch --bar",
  "return-type": "json",
  "on-click": "prelude-omarchy-launcher",
  "on-click-right": "prelude new agent"
}
```

## まだ

- PRELUDE のサイドバー(UI は作り直し中)。今は バー・メニュー・ランチャー・`prelude` コマンドから使う
- Arch のパッケージ(PKGBUILD)
