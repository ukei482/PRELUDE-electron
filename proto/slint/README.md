# Slint の試作

PRELUDE の UI(サイドバー・ランチャー・設定)を、GPUI の代わりに Slint で作れるかを確かめる試作。ルートのワークスペースには入れない。

## 中身

| ファイル | 中身 |
|---|---|
| `ui/prelude.slint` | **サイドバーの決まり**(`global Prelude`)。表示するもの(`in property`)と頼むこと(`callback`)。サイドバーはこれだけを通してコアとやり取りする |
| `ui/sidebar.slint` | 試作のサイドバー。上が本物の形(スペース・ブックマーク・タブ)、下が確かめること(日本語の字・入力欄・1 万件の一覧) |
| `ui/broken.slint` | 決まりを破った見本(名前の間違い・型の間違い・決まりに無い頼みごと) |
| `ui/fake.slint` | 決まりを使わず、似た `global` を自分で作った見本 |
| `src/core.rs` | コアとのやり取り(`prelude-proto`)。別のスレッドで見張り、`slint::invoke_from_event_loop` で UI を書き換える |
| `src/loaded.rs` | 使用者が置いた `.slint` を実行時に読み込む(`--load`。`--features interpreter`)。読み込むときに決まりを確かめる |
| `tests/contract.rs` | 決まりを破った `.slint` がコンパイルで止まることのテスト |
| `examples/screenshot.rs` | 画面の無い所で、ソフトウェア描画で PNG に書き出す |

## このリポジトリのクラウドの環境で確かめたこと(2026-11-11。画面・日本語入力なし、4 コア)

- **決まり**: `ui/sidebar.slint` の `Prelude.tabs` を `Prelude.tab` にすると、`cargo build` が `'Prelude' does not have a property 'tab'` で止まる。`broken.slint` の 3 つの破り方は、どれもコンパイルのエラーになる(`cargo test`)
- **実行時に読み込む場合**: `--load ui/fake.slint` は「property `online` が無い」などを挙げて断る。`--load ui/broken.slint` は行番号つきで断る。`--load ui/sidebar.slint` は確かめを通る
- **ビルド**: 初回の debug が 5 分 5 秒、release が 4 分 19 秒。実行ファイルは release で 28.8MB(strip すると 21.6MB)。Rust 1.97 で通る(Slint 1.18.1 は 1.92 から)。ビルドに足したライブラリは無い
- **字**: ソフトウェア描画で、漢字・かな・半角カナ・丸数字・絵文字が出る(システムのフォントから探す)
- **入力欄**: Slint の `LineEdit` をそのまま使う(GPUI では入力欄を自分で約 690 行書いていた)

## この PC で確かめること

```bash
cd proto/slint
cargo run --release                       # 作った 1 万件
cargo run --release -- ~/Downloads        # フォルダの中のファイル名
SLINT_BACKEND=winit-software cargo run --release   # CPU で描く
WAYLAND_DISPLAY= cargo run --release      # X11(XWayland)で
cargo run --release --features interpreter -- --load ui/sidebar.slint   # 実行時に読み込む
cargo run --release --features interpreter -- --load ui/fake.slint      # 断られる
```

1. **日本語入力(fcitx5)**: 入力欄で、変換しながら打てるか(変換中の下線・候補の窓・確定)。Wayland と X11、`winit-femtovg`(既定)と `winit-software` の両方
2. **ウィンドウ**: 普通のウィンドウとして出るか(sway ならタイルに並ぶ)。app_id は `prelude-slint-proto`。一番上の青い行を押すと、細い列 ⇄ 広いに切り替わり、浮いた窓なら幅が変わる
3. **1 万件の一覧**: ホイールで動かしてカクつかないか
4. **コアとのやり取り**: `prelude-core` が動いていれば、スペース・ブックマーク・タブが出る。押すと切り替わる・開く・閉じる(×)。コアを止めると「つながっていない」になり、起動し直すと戻る
5. **メモリと起動**: 起動の 0.5 秒後に、標準エラーに `started in … ms, VmRSS …` が出る
6. GPU が不調なとき: `LIBGL_ALWAYS_SOFTWARE=1` で描けるか

### ビルドに要るもの
- Kubuntu: 足りなければ `libfontconfig-dev`(と `pkg-config`)。実行には `libxkbcommon`・Wayland・EGL(Mesa)が要る(ふつうは入っている)
- NixOS: `nix develop` の中で。実行のときに開くライブラリ(`libxkbcommon`・`wayland`・`libGL`・`fontconfig`)が見つからなければ、`LD_LIBRARY_PATH` に足す
