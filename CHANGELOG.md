# 変更履歴

形式は [Keep a Changelog](https://keepachangelog.com/ja/1.1.0/) に倣う。版は `vMAJOR.MINOR.PATCH`（git のタグ）。
設定やデータの互換性が変わる変更には **[互換性]** を付ける。

## [未リリース]

### 変更
- 設計メモの「plasmashell を止めている間は音量キーが効かない」を訂正。kded6 の `audioshortcutsservice` が処理していて、シェルモードでも効く。PRELUDE 側では処理しない（二重に変わるため）

## [0.3.1] - 2026-10-09

### 修正
- 「Kubuntuに戻る」が押しても何も起きなかった。`src/quick.js` が読み込み時に、まだ使えない `api` を呼んで例外になり、ファイルの残り（`returning` の初期化）が実行されていなかったため。
  状態の取得は起動処理（`boot`）から行う。クイック設定・設定ペイン・ランチャーのどれから押しても同じ原因で動いていなかった

## [0.3.0] - 2026-10-09

### 追加
- `scripts/prelude.sh`：起動・デバッグ起動・停止・状態表示・ログ・シェルの切り替え・リリース管理を1つにまとめた入口（README の表を参照）
- 「Kubuntuに戻る」（クイック設定の「デスクトップ」、ランチャーのコマンド、`scripts/shell-mode.sh return`）。PRELUDE がシェルとして動いているときだけ出る。
  手順は `scripts/prelude-return.sh`：事前確認 → PRELUDE を穏やかに終了 → キーリング（KWallet）の応答確認（固まっていれば再起動）→ plasmashell を起動して通知・トレイの名前を取るまで待つ → 自動起動を解除。
  途中で失敗したら PRELUDE のまま残す。経過は `~/.local/state/prelude/return.log`
- 設定ペインの最後に「デスクトップ」欄と「Kubuntuに戻る」ボタン（2回押しで実行）。シェルとして動いていないときは押せず、理由を表示する
- 端末で動くアプリ（`.desktop` が `Terminal=true` の htop など）は Konsole の中で起動して取り込む
- Web ペインが Google のログイン画面（accounts.google.com のログイン・アカウント選択・OAuth）に進もうとしたら、そのペインを本物の Chromium のペインに自動で切り替える。
  Electron は Google に埋め込みブラウザとして「このブラウザは安全でない可能性があります」と弾かれ、UA や Client Hints の偽装では安定して通らないため。
  ログインは Chromium の専用プロファイルに残る。他サイトの「Googleでログイン」のポップアップは、元のページごと Chromium で開き直す（Chromium が無い・取り込み非対応の環境では従来どおり）

### 変更
- 終了時、取り込んだアプリに SIGTERM を送って最大8秒閉じるのを待つ（以前は PRELUDE が終わった直後に systemd が残りを SIGKILL していた）
- `prelude-shell.service` に `TimeoutStopSec=20` を追加。SIGTERM から14秒たっても終わらなければ自分で正常終了する（以前は90秒待たされたうえ、強制終了で「失敗」扱いになり、フォールバックが走って PRELUDE が再起動していた）
- フォールバックは、「Kubuntuに戻る」の最中は何もしない

### 修正（終了・切り替え）
- 終了処理の途中で D-Bus の呼び出しが届くと、切断済みの接続に返事を送ろうとして例外（`Cannot send message, stream is closed`）になり、
  Electron のエラーダイアログでメインの処理が止まって PRELUDE が終われなくなっていた。切断後の送信は捨てる
- 捕まらなかった例外ではダイアログを出さずに記録し、終了処理中ならそのまま終わる（シェルとして動いているとき、ダイアログで切り替えが止まらないように）
- 「固まっても14秒で自分で終わる」仕組みが働いていなかった（SIGTERM は Electron が自分で受けるため、仕掛けていた場所に来なかった）。終了の開始時に必ず仕掛ける
- `shell-mode.sh on --now` は、手で起動した PRELUDE（固まって残っているものも含む）が動いていれば切り替えずに止める（サービスの PRELUDE が二重起動として即終了し、シェルが無い状態になっていた）
- `shell-mode.sh on` は、シェルとして起動するリリースが作業フォルダと違うコードなら知らせる
- `prelude.sh start` / `stop` は、D-Bus の名前を持たずに残っている PRELUDE も見つける

### 修正（アプリの取り込み）
- 同じアプリを2つのタブで開くと、先のタブが後のタブのウィンドウまで取り込み、後のタブが空のままになっていた。
  ウィンドウは一度ペインに取り込んだらウィンドウIDで固定する
- 既に動いているプロセスに起動を任せるアプリ（Zed・システム設定・Dolphin など）のウィンドウを見つけられず、20秒後に「終了した」と判定されて空のペインに戻り、
  タブを開き直すたびに起動し直して空のウィンドウが増えていた。`.desktop` の ID（Wayland の app_id）・StartupWMClass・実行ファイル名で照合する。
  対象は起動後に現れたウィンドウだけ。新しいウィンドウを出さずに既存のウィンドウを前に出すアプリ（システム設定など）は、その既存のウィンドウを借りる
- 借りた既存のウィンドウは、タブを閉じても PRELUDE を終了しても閉じず、普通のウィンドウに戻す。ペインが開いたウィンドウは閉じ、PRELUDE が起動したプロセスだけを終了させる
- ウィンドウが一度も見つからなかったときは、タブを開き直すたびに起動し直すのをやめ、通知だけ出す
- 起動したアプリに Electron 用の環境変数（`CHROME_DESKTOP`・`NO_AT_BRIDGE`）を渡していた。作業ディレクトリもホームにする
- `Exec=/usr/bin/false` のような何もしない項目をアプリ一覧から除く。上位のフォルダで `Hidden=true` にされた `.desktop` も一覧から消す
- PRELUDE がクラッシュ・強制終了すると、取り込んでいたアプリのウィンドウが「最小化・枠なし・最前面・タスクバーにも切り替えにも出ない」まま残り、
  そのアプリを起動し直しても（既存のプロセスに引き継がれるだけで）開けなくなっていた。
  KWin スクリプトが PRELUDE 本体のウィンドウが消えたのを検知して普通のウィンドウに戻す。
  保険として取り込み中のウィンドウIDを `$XDG_RUNTIME_DIR/prelude-embed-wids.json` に控え、次の起動時に残っていれば戻す

**[互換性]** ユニットファイルの更新を反映するには `scripts/shell-mode.sh on`（または `release.sh install-bin` とユニットの再配置）が必要。

## [0.2.0] - 2026-10-08

### 追加
- システムトレイ（StatusNotifierItem のホスト）。アイコン・ツールチップ・メニュー（DBusMenu）・注意状態。plasmashell が止まっても表示される
- リリース用の配置（`scripts/release.sh`）と、起動に失敗し続けたときに1つ前の版へ自動で戻るフォールバック（`scripts/prelude-fallback.sh`）。手順は `docs/release.md`
- `scripts/shell-mode-test.sh` に自動ロールバックの実機テスト（TEST3）

### 変更
- ホームの「最近使ったアプリ」が空のときの案内文が、狭い列に折り返されていた表示を修正
- `scripts/shell-mode.sh on` は、リリース（`current`）があればそれを、無ければ作業フォルダを起動する
- フォールバックは、plasmashell に戻す前に、まず1つ前の版を試す

## [0.1.0]

PRELUDE を KDE Plasma 6（Wayland）のデスクトップシェルとして使えるようにした最初の版。

### 追加
- アプリ取り込み、疑似全画面、本物の Chromium ペイン
- D-Bus サービス `org.prelude.Shell`
- 下端のステータスバー（音量・Wi-Fi・電池・時計・メディア・通知）、ランチャー（Ctrl+Shift+P / Meta+Space）
- クイック設定、MPRIS のメディア表示
- 通知サーバ、通知センター、おやすみモード
- OSD（音量・明るさ）と、明るさが0%のまま続いたときの自動復帰、緊急用ショートカット（Meta+Shift+B）
- ホーム画面（壁紙・時計・最近使ったアプリ・ブックマーク）
- plasmashell との入れ替え（`scripts/shell-mode.sh`、systemd ユニット）

### 既知の未対応
- ~~plasmashell を止めている間、キーボードの音量キーが効かない~~ → 誤り。音量キーは kded6 の `audioshortcutsservice` が処理するので効く（0.3.1 以降の確認。`docs/shell-design.md` 6.5）

[未リリース]: https://github.com/ukei482/PRELUDE-electron/compare/v0.3.1...HEAD
[0.3.1]: https://github.com/ukei482/PRELUDE-electron/compare/v0.3.0...v0.3.1
[0.3.0]: https://github.com/ukei482/PRELUDE-electron/compare/v0.2.0...v0.3.0
[0.2.0]: https://github.com/ukei482/PRELUDE-electron/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/ukei482/PRELUDE-electron/releases/tag/v0.1.0
