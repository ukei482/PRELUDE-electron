# CLAUDE.md — PRELUDE (Electron版)

ブラウザ・エクスプローラ・ウィンドウ管理を1つにした、Kubuntu 用のデスクトップシェル。
返答・コメント・コミットメッセージ・CHANGELOG は日本語で書く。

## 構成
- `main.js` — メインプロセス。窓・Webビュー・IPC・ダウンロード
- `preload.js` — レンダラへ出す API(`window.preludeApi`)
- `config.js` — 設定の既定値と設定画面のスキーマ
- `fileops.js` — ファイラの IPC(`fs:*`。一覧・コピー・削除・監視など)
- `bmcodec.js` — `.prelude` の文字列(TOML。0.11 までの JSON も読む)⇔ 中身。中身の形は `src/bmfile.js`
- `appembed.js` — 外部アプリの窓をペインに取り込む(KWin 経由)
- `sys/` — バー・トレイ・通知・音量・更新(`sys/update.js`)などシステム連携。ホームの端末は `sys/term.js`(疑似端末は `sys/ptyhost.py`)。コマンド・アプリ・アイコンの場所は決め打ちにせず `sys/xdg.js`(`PATH`・`XDG_DATA_DIRS`)で探す
- `cli/` — ホームの端末で使う `PRELUDE` コマンド(`cli/PRELUDE` → `cli/prelude.js`)と、その bash の設定 `cli/bashrc`
- `src/` — レンダラ。`app.js`(タブ・分割・ペイン)、`files.js`(フォルダ)、`field.js`(フィールド表示)、`fieldlayout.js`(フィールドのカードの配置の計算)、`bmfile.js`(ブックマークのファイル `.prelude` の読み書き)、`space.js`(スペース。ブックマークの並び・旧ブックマークの移し方・タブの保存の形)、`history.js`(開いた履歴と点数)、`term.js`(ホーム = 起動直後の白い端末)、`cli.js`(PRELUDE コマンドの命令)、ほか
  - `app.js` 以外は `app.js` より先に読み込まれ、共通関数(`h`、`ico`、`api` など)は呼び出し時に使う
- `theme/` — ロック画面(kscreenlocker のシェルのパッケージ)と SDDM のテーマ。QML。見た目は `theme/common/Prompt.qml` で共通。入れる/外すのは `scripts/theme.sh`
- `scripts/` — `prelude.sh`(起動・停止・デバッグ)、`release.sh`(版の作成・切替)、`shell-mode.sh`(plasmashell との切替)、`portal.sh`(アプリのダイアログを PRELUDE にする/戻す)
- `test/` — `npm test`(Node の `node:test`)。DOM・Electron なしで動く計算だけをテストする
- `crates/` — Rust 版(`docs/redesign-rust.md`。設計は話題ごとの節で、コードのコメントは「`docs/redesign-rust.md` の「窓のつなぎ」」のように節の名前で指す。節の名前を変えたらコメントも直す)。`cargo test`(Rust は `~/.cargo/bin`)。CI(`.github/workflows/test.yml`)は `cargo clippy --all-targets -- -D warnings` と `cargo test` を回す
  - `prelude-format`(`.prelude`・`space.toml`)。`test/fixtures/format/` は JS と Rust の両方のテストが読む
  - `prelude-proto`(コアと UI・コマンドの約束)、`prelude-core`(コア。`state.rs` が状態とコマンド、`server.rs` がソケット、`compositor.rs` が窓を並べる相手の差し替え口、`embed.rs` と `embed.js` が KWin の裏側(アプリの窓の取り込み)、`sway.rs` が sway・SwayFX の裏側、`plain.rs` がつなぎ無し(対応するコンポジタが無いときにアプリを起動だけする)、`recipes.rs` がペインのレシピ(既定のファイラ `files` は yazi)、`portal.rs` がアプリのダイアログのポータル、`web.rs` と `extension/` と `webhost.rs` が Web のペイン(消音・ファビコンも)、`bmwrite.rs` がブックマークを書く命令(保存・名前・移動・並び・フォルダ)、`trash.rs` がゴミ箱、`icons.rs` がアプリのアイコン)、`prelude-cli`(`prelude` コマンド。`choose.rs` はダイアログの選ぶ画面 `prelude choose`(yazi))
  - コアの命令はどれも `prelude` コマンドからも使えるようにする(サイドバーなどの UI にだけ許された操作を作らない)。ソケットの約束(`prelude-proto`)は、足すだけなら版を上げない
  - 別のソケットで動かしたコアは、D-Bus の名前(`org.prelude.Core.d…`)と KWin スクリプトの名前に印が付き、ふだんのコア・Electron 版と取り合わない。窓を出さずに試すなら `--no-embed`
  - コアを試すときは、ソケット・状態・スペースを別にする: `PRELUDE_CORE_SOCK=/tmp/…sock target/debug/prelude-core --state <ファイル> --spaces <フォルダ>`(ソケットのパスは 108 バイトまで)
  - 設定は `~/.config/prelude/theme.toml`・`settings.toml`(`prelude-format` の `config`)。Electron 版からの引き継ぎは `prelude-core` の `migrate.rs`(別のソケットのコアでは動かない)
- `ui/` — GPUI で作った前の UI(`prelude-ui`・`prelude-sidebar`・`prelude-launcher`・`prelude-filer`・`prelude-settings`)。**UI は Slint で作り直すことにした**(`docs/redesign-rust.md` の「UI」)ので、手を入れない。Slint の UI ができたら、`prelude-filer`(ファイラは yazi に任せた)も含めて消す。ルートとは別のワークスペースで、ビルドには GPUI の開発用のライブラリと Rust 1.98.1 が要る
- `proto/slint/` — Slint の試作と、**サイドバーの部品の決まり**(`ui/parts.slint`。部品ごとの `global`: `SidebarFrame`・`Spaces`・`Bookmarks`・`Tabs`・`NewTab`・`PaneBar`・`BookmarkButton`)。決まりを破るとビルドで止まることのテストを含む(`cargo test`。画面なしで動く)。ルートのワークスペースの外。手順は `proto/slint/README.md`
- `proto/gpui/` — GPUI の試作(段階 0)。Slint に替えたので、Slint の UI ができたら消す
- `flake.nix`・`nix/` — NixOS へ移るための開発環境(`nix develop`)と作者の PC の OS の設定(`docs/rice.md`。PRELUDE の設計は `docs/redesign-rust.md`)。`nix/prelude.nix` はコア・コマンドのパッケージ、`nix/module.nix` は NixOS のモジュール(`services.prelude`。コアのサービス・sway の断片・ダイアログ)。rice とのつなぎ方は `docs/rice.md`
- `docs/release.md` — リリースと更新の詳しい手順

## 開発版を試す
シェルの PRELUDE は `~/.local/share/prelude/current`(リリース)を起動していて、作業フォルダの変更は届かない。
シェルを動かしたまま試すときは、別プロファイルで起動する(`prelude.sh debug` はシェルが動いていると止まる)。

```bash
PRELUDE_DEBUG=1 setsid nohup node_modules/electron/dist/electron . --no-sandbox \
  --user-data-dir=$HOME/.local/state/prelude/debug-profile --remote-debugging-port=9333 >/tmp/prelude-debug.log 2>&1 &
```

- ユーザーは PRELUDE の中からも開発版を開ける(ランチャー「PRELUDE 開発版を開く」。同じ debug-profile・ポート 9333 で、ペインに取り込まれる)。9333 が既に動いていれば、それを CDP で使えばよい
- デバッグ用の PRELUDE は全画面にしない(debug-profile は `startFullscreen=false`)。全画面だとユーザーの作業を覆う
- 操作の確認は CDP(`http://127.0.0.1:9333/json`)。`main.js` を変えたらレンダラの再読込では足りず、起動し直す

## コミットと PR
- 作業は機能ブランチで行い、PR を `main` へ出す。**PR のマージはユーザーが行う**
- git の作者設定が無いので `git -c user.name=ukeiyamada -c user.email=you10.0521@gmail.com commit ...` で書く
- 変更は CHANGELOG の「未リリース」に追記する

## リリース(PR のマージだけでは更新されない)
PRELUDE の「アップデート」は **GitHub の一番新しい `v*` タグ** を見る(`release.sh latest`)。
**タグを push するまで、マージした変更はどの PC にも届かない。** 手順:

1. `release/vX.Y.Z` ブランチを `origin/main` から作る
2. `npm version X.Y.Z --no-git-tag-version`(package.json と package-lock.json)
3. CHANGELOG の「未リリース」の中身を `## [X.Y.Z] - 日付` に移し、末尾の比較リンクを足す
4. PR を出す → **ユーザーがマージ**
5. **リリース PR のマージコミットに注釈付きタグを打って push する**(これを忘れない)

   ```bash
   git fetch origin
   git tag -a vX.Y.Z <マージコミット> -m "vX.Y.Z"
   git push origin vX.Y.Z
   ```

6. 反映: PRELUDE の設定「アップデート」→「更新して再起動」(または `scripts/prelude.sh update`)
   - 手動なら `scripts/release.sh create vX.Y.Z` → `scripts/release.sh switch vX.Y.Z` → `systemctl --user restart prelude-shell`

確認: `scripts/release.sh latest` が新しいタグを返せば、画面の「確認」で更新が出る。
