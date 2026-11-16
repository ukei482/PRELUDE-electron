# Omarchy で PRELUDE を使う

Omarchy(Arch + Hyprland + Quickshell の `omarchy-shell`。4.0 から)の上で、Rust 版の PRELUDE(コア・`prelude` コマンド)を動かす手順。
設計は [redesign-rust.md](redesign-rust.md) の「Omarchy から学んだこと」「窓のつなぎ」。

## できること

| どこ | 何 |
|---|---|
| **バー** | 左(Omarchy のメニューの右)に PRELUDE の部品。Omarchy の作業領域の部品(1 2 3…)は外す(タブ = 作業領域なので、PRELUDE の部品とサイドバーが代わりに出す)。スペースと今のタブの題名を出し、乗せるとタブの一覧。**左クリックでサイドバーを開け閉め**(開いている間は P に下線)、中クリックで AI のペイン、右クリックで PRELUDE のメニュー |
| **サイドバー** | 画面の左端に貼り付き、幅の分だけ場所を予約する(窓はその右に並ぶ)。スペース・タブ・ブックマーク・新しいタブのボタン。下の「サイドバー」 |
| **新しいタブ** | `Super + Alt + Space`(Omarchy の既定のアプリの一覧のキーを置き換える)・サイドバーの「+ 新しいタブ」・メニューの「新しいタブ」。空いた番号の作業領域へ移って Omarchy のアプリの一覧を開き、選んだアプリの窓をコアが引き取って新しいタブにする(`prelude-omarchy-newtab`)。PRELUDE 側に種類を選ぶ画面は持たない |
| **メニュー** | Omarchy のメニューに「PRELUDE」(新しいタブ・開く・AI・ファイラ・ブックマークに保存・設定・コアの起動し直し)。`omarchy menu summon prelude` でも開く |
| **キー** | `Super + Alt + P` で PRELUDE のランチャー(タブ・ブックマーク・スペースを Omarchy のメニューで選ぶ。アプリは出さない)、`Super + Alt + Space` で新しいタブ |
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
- 設定が無ければ `~/.config/prelude/settings.toml` に `profile = "omarchy"` を書く(窓は「作業領域だけ指定」、端末は foot、番号の作業領域の窓は PRELUDE を通さずに開いたものもタブにする)
- バーの部品を `~/.config/omarchy/plugins/prelude.bar/` に複写し、`omarchy plugin disable omarchy.workspaces` で Omarchy の作業領域の部品を外し、`omarchy plugin enable prelude.bar left --after omarchy.menu` で並べる。キーは `Super + Alt + P`(ランチャー)と `Super + Alt + Space`(新しいタブ。`o.rebind` で置き換える)。外すと(`uninstall`)作業領域の部品と Omarchy のキーに戻る
- メニューの項目を `~/.config/omarchy/extensions/omarchy-menu.jsonc` に、キーを `~/.config/hypr/bindings.lua` に、コアの自動起動を `~/.config/hypr/autostart.lua` に足す。どれも `PRELUDE >>>` と `PRELUDE <<<` の印の間だけで、流し直しても 1 つ
- Hyprland の中で流せば、その場でコアも起動する

確かめる: `scripts/omarchy.sh status`。外す: `scripts/omarchy.sh uninstall`(`~/.config/prelude` の設定・スペースは残す)。

バーの部品の置き場所は、バーの上でドラッグするか `omarchy bar move prelude.bar --section centerBegin`。部品の設定は `omarchy bar set prelude.bar maxWidth 400`(題名の最大の幅)・`launcher <コマンド>`(左クリック)。

## サイドバー

バーの部品のプラグイン(`omarchy/plugin/Sidebar.qml`)が、Quickshell の layer-shell の窓(名前空間 `prelude-sidebar`)で出す。窓と同じ決まりで置く: 画面の端(上・下・横)との間は Hyprland の `gaps_out`、隣の窓との間は窓と窓の間(`gaps_in` の 2 倍)。枠の太さ・色(選ばれていない窓の色)・角丸も Hyprland の設定(`border_size`・`col.inactive_border`・`rounding`)に合わせる。別のプログラムは要らない。コアとは `prelude` コマンドだけで話す(`prelude watch` の知らせで `prelude raw state.get` などを読み直し、押したら `prelude raw <命令>`)。

| どこ | 押すと |
|---|---|
| 上のスペースの名前 | スペースの一覧。押すと切り替え |
| `‹` / `×` | 細い列(番号と印だけ)にする / 閉じる。細い列の上の印を押すと広げる |
| タブ | 切り替え。中クリック・乗せると出る × で閉じる、右クリックで複製。分割したタブはペインの行が続く(押すとそのペインへ) |
| 閉じたタブを開き直す | 閉じたタブがあるときだけ出る |
| ブックマーク | 開く(開いていればそのタブへ。開いているものは明るい)。右クリックでそのフォルダをファイラで。フォルダは押すと開閉 |
| + 新しいタブ | `Super + Alt + Space` と同じ(空いた作業領域で Omarchy のアプリの一覧) |

設定は、バーの部品の設定(`~/.config/omarchy/shell.json` の `prelude.bar` の項目)に書く。保存するとすぐ効く:

```bash
omarchy bar set prelude.bar sidebarWidth 300
```

| 名前 | 既定 | 中身 |
|---|---|---|
| `sidebarWidth` | 260 | サイドバーの幅(160〜800) |
| `sidebarCompactWidth` | P のボタンの幅 | 細い列の幅(16〜120)。書かなければバーの P のボタンと同じ幅 |
| `sidebarSide` | `left` | 置く側(`left`・`right`) |
| `sidebarOpen` | `false` | シェルの起動のときに開いておく |
| `click` | `sidebar` | 左クリックですること。`launcher` にすると前と同じランチャー |

- 開いているか・細い列かは、シェルが動いている間だけ覚える。部品のある画面に出る(画面が 2 枚なら、押した方の画面)
- Omarchy のシェルはプラグインのファイルの変更を読み直すが、作り直した部品が反映されないことがある。`scripts/omarchy.sh install` の後で効かなければ `omarchy restart shell`
- 名前を変える・並べ替える・フォルダを作るなどは、まだ `prelude` コマンドから(文字の入力欄を持たないため)

## 窓のつなぎ(段 1)

設定の `windows.level` の上限は Hyprland では `workspace`(段 1)。`profile = "omarchy"` もそれを選ぶ。

- アプリは Hyprland の exec に作業領域を付けて起動する(Lua の `hl.dsp.exec_cmd(…, { workspace = "N silent" })`。前の Hyprland なら `exec [workspace N silent] …`)。作業領域は選ばない
- 出てきた窓は、起動したプロセスの環境変数 `PRELUDE_PANE`(Web のペインは題名の印)でペインに結び付ける。既に動いているブラウザが開いた窓のように exec の決まりが効かなかった窓だけ、タブの作業領域へ移す
- Hyprland で作業領域を移ると、PRELUDE の今のタブも変わる(読むだけ)。PRELUDE でタブを選ぶ・開くと、その作業領域へ移る
- **開いている窓もタブにする**(`windows.adopt = "tabs"`。`profile = "omarchy"` の既定): コアを起動したとき、番号の作業領域(1・2…)に開いていた窓を、その番号のタブのペインにする(タブが無ければ作る)。前に PRELUDE が開いた窓なら(`PRELUDE_PANE`・Web の印)、起動し直さずにそのペインに結び付ける。後から PRELUDE を通さずに開いた窓も、少し待って(Web のペインの窓でないと分かってから)引き取る。引き取った窓は PRELUDE の持ち物で、タブ・ペインを閉じると窓も閉じる。窓を閉じるとペインも消える
  - 引き取らない窓: 浮いた窓、`windows.ignore` の窓、番号でない作業領域(special など)の窓。引き取りをやめるなら `prelude config set windows.adopt none`(コアを起動し直すと効く)
  - インストール済みのアプリと分かる窓は、閉じたタブを開き直せる(起動コマンドを持つ)。分からない窓(Omarchy の Web アプリなど)は開き直せない
- 浮いた窓には触らない。間・角丸・窓の決まり(`windows.gaps_*`・`corner_radius`)も Hyprland では当てはめない(見た目は Omarchy のもの)
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

- サイドバーでの名前の変更・ドラッグでの並べ替え・アイコン(今は種類の文字)
- Arch のパッケージ(PKGBUILD)
