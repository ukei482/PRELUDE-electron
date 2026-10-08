# 変更履歴

形式は [Keep a Changelog](https://keepachangelog.com/ja/1.1.0/) に倣う。版は `vMAJOR.MINOR.PATCH`（git のタグ）。
設定やデータの互換性が変わる変更には **[互換性]** を付ける。

## [未リリース]

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
- plasmashell を止めている間、キーボードの音量キーが効かない（`docs/shell-design.md` 6.5）

[未リリース]: https://github.com/ukei482/PRELUDE-electron/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/ukei482/PRELUDE-electron/releases/tag/v0.1.0
