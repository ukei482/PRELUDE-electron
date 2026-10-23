# CLAUDE.md — PRELUDE (Electron版)

ブラウザ・エクスプローラ・ウィンドウ管理を1つにした、Kubuntu 用のデスクトップシェル。
返答・コメント・コミットメッセージ・CHANGELOG は日本語で書く。

## 構成
- `main.js` — メインプロセス。窓・Webビュー・IPC・ダウンロード
- `preload.js` — レンダラへ出す API(`window.preludeApi`)
- `config.js` — 設定の既定値と設定画面のスキーマ
- `fileops.js` — ファイラの IPC(`fs:*`。一覧・コピー・削除・監視など)
- `appembed.js` — 外部アプリの窓をペインに取り込む(KWin 経由)
- `sys/` — バー・トレイ・通知・音量・更新(`sys/update.js`)などシステム連携
- `src/` — レンダラ。`app.js`(タブ・分割・ペイン)、`files.js`(フォルダ)、`field.js`(フィールド表示)、`fieldlayout.js`(フィールドのカードの配置の計算)、`history.js`(開いた履歴と点数)、ほか
  - `app.js` 以外は `app.js` より先に読み込まれ、共通関数(`h`、`ico`、`api` など)は呼び出し時に使う
- `scripts/` — `prelude.sh`(起動・停止・デバッグ)、`release.sh`(版の作成・切替)、`shell-mode.sh`(plasmashell との切替)
- `test/` — `npm test`(Node の `node:test`)。DOM・Electron なしで動く計算だけをテストする
- `docs/release.md` — リリースと更新の詳しい手順

## 開発版を試す
シェルの PRELUDE は `~/.local/share/prelude/current`(リリース)を起動していて、作業フォルダの変更は届かない。
シェルを動かしたまま試すときは、別プロファイルで起動する(`prelude.sh debug` はシェルが動いていると止まる)。

```bash
PRELUDE_DEBUG=1 setsid nohup node_modules/electron/dist/electron . --no-sandbox \
  --user-data-dir=$HOME/.local/state/prelude/debug-profile --remote-debugging-port=9333 >/tmp/prelude-debug.log 2>&1 &
```

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
