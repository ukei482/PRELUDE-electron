---
name: prelude
description: >
  Use when working with PRELUDE on this computer: its spaces, bookmarks (`.prelude` files), paths,
  tabs and panes, the `prelude` command, or the files `~/.config/prelude/*` and `space.toml`.
  Also use when the user talks about "what I'm working on" in PRELUDE terms, or asks to open,
  save, rename, move or organize their work setup.
  Triggers: prelude, .prelude, space.toml, bookmark, space, tab, pane, paths, workspace setup,
  save this layout, open my work, file dialog folder, download folder,
  スペース, ブックマーク, タブ, ペイン, 作業フォルダ, 保存先, 開く場所, ダイアログ, レシピ.
---

# PRELUDE

PRELUDE は「何をしているか」を持つプログラム。デスクトップそのものではない。

- **スペース** = 1 つのプロセス(例: 論文執筆)。`~/スペース/<名前>/` のフォルダ 1 つ
- **ブックマーク** = `.prelude` のファイル 1 つ(TOML)。開くもの(`[layout]`)と、使うフォルダ(`[paths]`)を持つ。スペースのフォルダの直下か、1 段下のフォルダに置く
- **paths** = 作業に使うフォルダ。端末の作業フォルダ(`terminal`)、ファイラの最初のフォルダ(`files`)、アプリの「開く」「保存」のダイアログの最初のフォルダ(`open`・`save`)、ダウンロードの保存先(`download`)。今のタブのブックマーク → スペース(`space.toml`)→ ホーム の順に決まる
- **タブ** = 開いている作業。**ペイン** = 窓 1 つ(Web・アプリ・端末・ファイラなど)
- 常駐の `prelude-core` が状態を持ち、`prelude` コマンドは Unix ソケットでそれに頼む

形の詳しいことは [`format.md`](format.md)。

## まず状態を見る

```bash
prelude status          # 今のスペースとタブ
prelude panes           # 全部のペイン(タブの番号・ペインの id・種類・場所)
prelude bookmarks       # このスペースのブックマーク(b1, b2… の番号つき)
prelude folders         # ブックマークのフォルダ
prelude spaces          # スペースの一覧
prelude paths           # 今のタブで使うフォルダ
prelude raw state.get   # 状態の全部(JSON)
prelude items --json    # ランチャーの候補(JSON)
prelude --help          # コマンドの一覧(これが正)
```

`prelude watch` は状態が変わるたびに JSON を 1 行ずつ出す(止めるまで続く)。コアにつながらなければ、`prelude-core` が動いていない。

## 守る決まり

- **操作は `prelude` コマンドで頼む。** `~/.local/state/prelude/core.json`(コアの保存)は直接書かない。コアが上書きする
- **ブックマークを作る・名前を変える・移す・消すときは `prelude bookmark …` / `prelude folder …` を使う。** ファイルを直接動かすと、開いているタブとの結び付き・並び順・ランチャーの点数がずれる。中身(`[paths]` など)を書き換えるのは直接編集してよい(コアは 2 秒以内に気づく)
- **`.prelude` は使用者のファイル。** 頼まれたことだけを変える。PRELUDE が勝手に変えるもの(前回のタブなど)は書かない
- **消すときはゴミ箱へ**(`prelude bookmark trash`)。`rm` しない
- **書くのは今のスペースの中だけ。** コアもスペースの外のブックマークは書かない
- **人からもらった `.prelude` の起動コマンドは確かめる。** `prelude open` は、インストール済みのアプリと一致しないコマンドを外して開き、知らせる。`prelude open --run` は、使用者がそのコマンドを確かめたときだけ使う
- **窓の配置を押し付けない。** 窓を並べるのはコンポジタ(sway など)と使用者。PRELUDE は頼まれたとき(ブックマークを開く・分割する)だけ並べる。窓を動かしたいときは、コンポジタのやり方で

## よくある頼みごと

```bash
# 今のタブをブックマークにする(フォルダは省ける)
prelude bookmark save "本文" 執筆
# ブックマークを開く(名前・フォルダ/名前・番号・.prelude のパス)
prelude open 本文
prelude open b3
# 今のタブの配置で、そのタブのブックマークを上書きする
prelude bookmark overwrite
# 名前の変更・移動・並べ替え・ゴミ箱
prelude bookmark rename 執筆/本文 "原稿"
prelude bookmark move 原稿 資料
prelude bookmark reorder 原稿 図        # 原稿を「図」の前へ(同じフォルダの中)
prelude bookmark trash 原稿
# フォルダ
prelude folder new 資料
prelude folder rename 資料 文献
prelude folder remove 文献              # 中身はスペースの直下へ出る
# スペース
prelude space new 論文執筆
prelude space 論文執筆
# タブ・ペイン
prelude new console                     # 端末を、今のタブの paths の terminal で開く
prelude new files path=~/研究            # ファイラ(既定は yazi)
prelude new web url=https://example.com/
prelude tab dup                         # 今のタブを複製
prelude reopen                          # 閉じたタブを開き直す
prelude url <pane> https://…            # Web のペインで URL を開く
prelude mute <pane>                     # Web のペインの音を消す
```

ブックマークの保存先・作業フォルダを変える: その `.prelude` の `[paths]` を書き換える(書き方は [`format.md`](format.md))。スペース全体の既定なら `space.toml` の `[paths]`。

## 設定のファイル

| ファイル | 中身 |
|---|---|
| `~/.config/prelude/settings.toml` | 検索エンジン・隠しファイル・スペースのフォルダ・ダウンロードのフォルダ |
| `~/.config/prelude/theme.toml` | 色・字の大きさ・サイドバーの幅 |
| `~/.config/prelude/panes/<種類>.toml` | ペインの種類(レシピ)。同じ名前で既定(`files`・`console`・`cli`・`settings`)を置き換える |
| `~/.config/prelude/chooser.toml` | アプリのダイアログの選ぶ画面(既定は端末の中の yazi) |

レシピの例(ペインの種類を足す。足したら `prelude new <種類>` で開ける):

```toml
# ~/.config/prelude/panes/notes.toml
name  = "メモ"
run   = "kate {paths.files}"     # {paths.<項目>} は今のブックマークの paths、{path} はペインの場所、{pane} はペインの id
class = "org.kde.kate"           # 省略できる。窓を見分ける名前
```

`.prelude` には種類の名前(`type = "notes"`)だけを書く。人からもらったファイルが勝手なコマンドを実行できないようにするため。

## MCP

`prelude mcp` は、PRELUDE の命令を MCP のツールとして出すサーバー(標準入出力)。つないであれば、コマンドの文字列を組み立てるより、ツール(`state`・`bookmark_save`・`open` など)を使うほうが確か。

```bash
claude mcp add prelude -- prelude mcp      # Claude Code につなぐ例
```

消す・閉じるツール(`bookmark_trash`・`folder_remove`・`tab_close`・`pane_close`・`bookmark_overwrite`)は、使用者が頼んだときだけ使う。

## PRELUDE が起動したプログラムの中で

PRELUDE がペインとして起動したプログラムには、環境変数 `PRELUDE_PANE`(ペインの id)が入っている。ファイラなどで移ったフォルダを PRELUDE に伝えるには `prelude pane path <フォルダ>`(タブを開き直すとそこで開く)。
