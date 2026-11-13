# PRELUDE のファイルの形

## スペース

```
~/スペース/                    # 場所は settings.toml の space_dir で変えられる
└── 論文執筆/                  # スペース = フォルダ 1 つ
    ├── space.toml             # スペースの既定の paths と、ブックマークの並び順
    ├── 本文.prelude            # ブックマーク
    └── 執筆/                   # ブックマークのフォルダ(1 段だけ)
        └── 図.prelude
```

- ファイル名(`.prelude` を除いたもの)が題名として一覧に出る
- 隠しファイル・隠しフォルダ(`.` で始まるもの)は読まない

## `.prelude`(ブックマーク 1 つ)

```toml
version = 2
title = "本文"

[paths]                       # 書いた項目だけが効く。項目は自由に増やせる
files    = "~/研究/論文"        # ファイラの最初のフォルダ
open     = "./文献"             # アプリの「開く」の最初のフォルダ(相対パスはこのファイルのフォルダから)
save     = "~/研究/論文/原稿"   # アプリの「保存」の最初のフォルダ
terminal = "~/研究/論文/src"    # 端末・起動するアプリの作業フォルダ
download = "~/研究/論文/資料"   # ダウンロードの保存先

[layout]                      # 開くもの。分割(split)とペイン(leaf)の木
k = "split"
dir = "row"                   # row = 左右、col = 上下
ratio = 0.4                   # a の側の割合(0〜1)

[layout.a]
k = "leaf"
type = "web"
url = "https://example.com/"

[layout.b]
k = "leaf"
type = "app"
cmd = "kate"                  # 起動コマンド
name = "Kate"
```

ペイン 1 つだけなら `[layout]` に直接 `k = "leaf"` を書く。

### ペインの種類(`type`)
| 種類 | 項目 |
|---|---|
| `web` | `url` |
| `app` | `cmd`(起動コマンド)、`name`、`cls`(窓の名前。省ける) |
| `files` | `path`(開き始める場所。省くと paths の `files`)。既定は端末の中の yazi |
| `console` | 端末。`cwd`(省くと paths の `terminal`) |
| `cli` | PRELUDE のコマンドを打つ端末 |
| `settings` | PRELUDE の設定 |
| ほかの名前 | `~/.config/prelude/panes/<名前>.toml` のレシピ。無ければ「入っていない」と出し、中身はそのまま残る |

- 知らない項目(`[ui.<部品名>]` など)は、PRELUDE が書き戻しても消えない
- 0.11 までの JSON の `.prelude` も読める(PRELUDE が書き直すと TOML になる)

## paths の決まり方

項目ごとに、次の順で最初に見つかったもの:
1. 今のタブを開いたブックマークの `[paths]`
2. スペースの `space.toml` の `[paths]`
3. 代わりの項目(`download` は `save` → `files`、ほかは `files`)を、同じ順で
4. ホーム

`~/…` はホーム、相対パスはそのファイル(`.prelude` か `space.toml`)のフォルダから数える。

アプリのダイアログの最初のフォルダは、「保存し直すファイルのフォルダ → 上の paths(書いてあれば)→ アプリが頼んだフォルダ → ホーム」の順。

## `space.toml`

```toml
order = ["本文.prelude", "執筆/図.prelude"]   # ブックマークの並び順(prelude bookmark reorder が書く。[paths] より前に書く)

[paths]                       # このスペースの既定(ブックマークに書いていない項目)
files    = "~/研究/論文"
terminal = "~/研究/論文"
```
