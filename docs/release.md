# リリースと更新の手順

デスクトップシェルとして常用する版（**安定版**）と、開発中の作業フォルダ（**開発版**）を分けて、
更新で壊しても戻れるようにするための手順です。道具は `scripts/release.sh`（配置の管理）と
`scripts/shell-mode.sh`（plasmashell との切替）、`scripts/shell-mode-test.sh`（実機テスト）です。

## 配置

```
~/.local/share/prelude/            （$PRELUDE_ROOT で変更可）
├── releases/v0.1.0/               その版のコード + node_modules（git worktree）
├── releases/v0.2.0/
├── current  -> releases/v0.2.0    常用する版。systemd のユニットはここを起動する
├── previous -> releases/v0.1.0    1つ前の版。自動で戻るときの行き先
└── bin/prelude-fallback.sh        起動に失敗し続けたときの復旧。新しい版が壊れていても動くよう、単独で置く

~/.config/PRELUDE-electron/        設定・ブックマーク・履歴。版をまたいで共有される
```

開発版（この作業フォルダ）は、常用とは別に動かせます。常用のシェルを止めずに試すときは、別のデータフォルダで起動します。

```bash
npx electron . --no-sandbox --user-data-dir=/tmp/prelude-dev
```

## 更新の流れ

1. 開発してコミットし、PR を `main` にマージする。
2. タグを打つ（例 `git tag -a v0.2.0 -m "..."`、`git push origin v0.2.0`）。
3. 実機で確認する（plasmashell が一時的に止まるので、別の TTY を用意しておく）。

   ```bash
   scripts/shell-mode-test.sh
   ```

4. 新しい版を作る（`npm ci` と構文チェックまで行う。切り替えはしない）。

   ```bash
   scripts/release.sh create v0.2.0
   ```

5. 切り替えて、サービスを再起動する。

   ```bash
   scripts/release.sh switch v0.2.0
   systemctl --user restart prelude-shell
   ```

6. 古い版を整理する（`current` と `previous` は消えない）。

   ```bash
   scripts/release.sh prune 3
   ```

## 戻す

- **手動で1つ前へ**: `scripts/release.sh rollback` のあと `systemctl --user restart prelude-shell`
- **自動**: 新しい版が起動直後に落ち続けると（60秒に3回）、`bin/prelude-fallback.sh` が `previous` へ戻して起動し直す。
  戻した版も落ちると、plasmashell に戻す。理由は `~/.local/state/prelude/last-rollback` と `journalctl --user -t prelude-fallback` に残る。
- **画面が操作不能になったら**: TTY（Ctrl+Alt+F3）でログインして `scripts/shell-mode.sh off --now`

## 注意

- **設定の互換性**: 設定は版をまたいで共有される。読み込みは「既知のキーだけを同じ型で上書き」するので、
  キーを足すのは安全。**キーの名前や型を変えるとき**は、古い形式を読み替える処理を入れてから出すこと
  （戻したときに、新しい版が書いた設定を古い版が読む場合もある）。
- **Electron の取得**: `npm ci` だけでは Electron の実行ファイルは入らない（v44 の仕様）。`release.sh create` が自動で取得する。
- **ディスク**: 1版あたり約300MB（node_modules）。`prune` で整理する。
- 既に動いている PRELUDE がある状態で、別の版を同じデータフォルダで起動すると、後から起動した側は終了する（単一インスタンス）。
