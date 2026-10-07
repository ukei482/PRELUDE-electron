# PRELUDE シェル化 設計書（Kubuntu / Plasma 6 / Wayland）

## 1. 目的と線引き

Kubuntu の「見える部分」（パネル・ランチャー・通知・トレイ・クイック設定・ファイラ・壁紙）を PRELUDE に置き換える。
ウィンドウ管理・合成・画面ロック・電源管理の本体は **KDE に任せる**。

| 担当 | 中身 |
|---|---|
| **KDE のまま** | KWin（ウィンドウ管理・合成・ロック画面）、kded、powerdevil（電源・明るさキー）、polkit エージェント、xdg-desktop-portal-kde、PipeWire、NetworkManager、SDDM |
| **PRELUDE が担当** | ステータスバー、ランチャー、通知、OSD（音量・明るさ表示）、トレイ、クイック設定、ファイラ、壁紙・ホーム画面、ワークスペース |
| **やらない** | ウィンドウマネージャ、コンポジタ、ロック画面、ログイン画面 |

最終形は「Plasma セッションのまま `plasmashell` だけ止めて、PRELUDE がその役を務める」。
独自セッションは作らない（KDE の各デーモンの起動順を自前で持つことになるため）。

## 2. 前提と制約

- **Electron は Wayland の layer-shell を使えない。** 画面端に「場所を予約する」パネル窓は作れない。
  → PRELUDE 本体を疑似全画面（既存の KWin スクリプト方式）で置き、**バーは本体の中に常設**する。
- **Webビュー（WebContentsView）の上に HTML を重ねられない。** 取り込みアプリ（KWin 窓）も PRELUDE より上にいる。
  → ランチャー・通知・OSD は **別の透明ウィンドウ（オーバーレイ窓）** にして、KWin スクリプトで最前面に置く。
- **plasmashell が持っている役割が多い。** 止めると次のものも一緒に消えるので、すべて PRELUDE 側で引き受ける：
  通知サーバ（`org.freedesktop.Notifications`）、OSD（`org.kde.osdService`。音量・明るさキーの表示先）、
  音量キーの処理（plasma-pa アプレット）、システムトレイ（StatusNotifierWatcher）、クリップボード履歴（Klipper）、壁紙。
- **D-Bus のサービスを公開する必要がある。** 現在の `gdbus` 呼び出しと `dbus-monitor` の横取りでは、
  通知サーバのように「呼ばれる側」になれない。→ `dbus-next`（純JS、ネイティブモジュール不要）を導入する。

## 3. 全体構成

```
┌──────────── KWin ─────────────────────────────────────────┐
│  ┌─ PRELUDE 本体（疑似全画面）───────────────────────────┐  │
│  │ ステータスバー                                          │  │
│  │ ┌サイドバー┐┌─ 分割ペイン ─────────────────────────┐ │  │
│  │ │タブ      ││ Web / フォルダ / アプリ / 端末 / ホーム │ │  │
│  │ │ブックマーク││                                       │ │  │
│  │ └─────────┘└───────────────────────────────────────┘ │  │
│  └──────────────────────────────────────────────────────┘  │
│  ┌─ オーバーレイ窓（透明・最前面）─┐  ← ランチャー / 通知 / OSD / クイック設定
│  └─────────────────────────────────┘                       │
│  取り込みアプリの窓（既存）                                    │
└────────────────────────────────────────────────────────────┘
        ▲ D-Bus（org.prelude.Shell）          ▲ D-Bus
        │ KWin スクリプトからのショートカット・窓一覧   │ UPower / NetworkManager / logind / MPRIS / PipeWire
```

### main プロセスのモジュール分割

`main.js` に足し続けず、システム連携は `sys/` に分ける。各モジュールは同じ形にする：

```js
// sys/xxx.js
module.exports = { init(ctx) { /* ctx = { send, config, bus, ipcMain } */ } };
```

| ファイル | 役割 | 使うもの |
|---|---|---|
| `sys/bus.js` | セッションバス接続、`org.prelude.Shell` の取得、他モジュールへ共有 | dbus-next |
| `sys/status.js` | 電池・ネットワーク・音量・明るさ・電源プロファイルの状態を購読してまとめて送る | UPower / NetworkManager / `pactl subscribe` + `wpctl` / `org.kde.Solid.PowerManagement` / power-profiles-daemon |
| `sys/notify.js` | 通知サーバ（受信・保持・アクション・閉じる） | `org.freedesktop.Notifications` を公開 |
| `sys/osd.js` | OSD サービス（powerdevil 等の明るさ表示を受ける） | `org.kde.osdService` を公開 |
| `sys/media.js` | 再生中のメディア表示と操作 | MPRIS（`org.mpris.MediaPlayer2.*`） |
| `sys/tray.js` | システムトレイ（アイコン・メニュー） | StatusNotifierWatcher / Item、DBusMenu |
| `sys/session.js` | ロック・ログアウト・スリープ・再起動・電源オフ | `loginctl lock-session`、logind、`org.kde.Shutdown` |
| `sys/apps.js` | `.desktop` 一覧・アイコン解決・使用頻度（`appembed.listApps` を移す） | XDG 仕様、アイコンテーマ |
| `sys/windows.js` | 取り込んでいない窓の一覧と切替（バーのタスク表示用） | KWin スクリプト → `org.prelude.Shell` |
| `sys/keys.js` | グローバルショートカット（Super、音量キーなど） | KWin スクリプトの `registerShortcut` → D-Bus |
| `sys/clipboard.js` | クリップボード履歴（任意） | Electron `clipboard` のポーリング |
| `sys/overlay.js` | オーバーレイ窓の生成・表示・最前面化 | BrowserWindow（透明）+ KWin スクリプト |

`appembed.js` の KWin スクリプト機構は残し、KWin → PRELUDE の連絡を `dbus-monitor` の横取りから
`org.prelude.Shell` への正式な呼び出しに置き換える。

### 画面側（renderer）

`src/app.js`（782行）に足さず、機能ごとにファイルを分ける：
`src/bar.js`（ステータスバー）、`src/overlay/`（ランチャー・通知・OSD・クイック設定。オーバーレイ窓で読み込む）、
`src/panes/terminal.js`、`src/panes/home.js`。

### preload の方針

画面側には「用途の決まった関数」だけを出す（`status.setVolume(0.5)` など）。
**任意コマンド実行・任意 D-Bus 呼び出しは画面側に出さない。** 引数は main 側で検証する。
Webペインには今まで通り preload を付けない。

## 4. 機能一覧

優先度：**A** = 置き換えに必須、**B** = 日常で効く、**C** = あると良い

### 4.1 ステータスバー（A）
本体上端（設定で下端も可）に常設。左から：
- ワークスペース切替（4.8）
- 取り込んでいない窓のタスク表示（クリックで前面化、`sys/windows.js`）
- 再生中メディア（曲名・再生/停止、`sys/media.js`）
- トレイアイコン（4.6）
- ネットワーク・音量・電池のアイコン → クリックでクイック設定（4.4）
- 通知ベル（未読数）→ 通知センター（4.3）
- 時計 → カレンダー

### 4.2 ランチャー（A）
Super キー（または設定したキー）でオーバーレイ窓に表示。1つの入力欄で全部を検索する：

| 種類 | 例 | 実行すると |
|---|---|---|
| アプリ | `fire` → Firefox | ペインに取り込み、または通常起動（設定で既定を選ぶ） |
| 開いているタブ・窓 | `gmail` | そのタブ／窓へ切替 |
| ブックマーク | `仕事` | 開く |
| ファイル | `/report` または `f report` | 最近のファイル＋`locate`/`fd` の結果 |
| URL・検索 | `example.com`、その他の文 | Webペインで開く（既存の `normalizeInput`） |
| コマンド | `ロック`、`再起動`、`設定` | `sys/session.js` などを呼ぶ（危険なものは確認を挟む） |
| 計算 | `12*3.5` | 結果をコピー |

並び順は既存の `appUsage` を使った使用頻度＋最近使った順。

### 4.3 通知（A）
- `org.freedesktop.Notifications` を実装（`Notify` / `CloseNotification` / `GetCapabilities` / `GetServerInformation`、
  シグナル `NotificationClosed` / `ActionInvoked`）。
- 右上にトースト（オーバーレイ窓）。アクションボタン・既定アクション（クリック）・画像に対応。
- 通知センター：履歴、アプリごとにまとめる、全消去。
- おやすみモード（トーストを出さず履歴にだけ残す）。
- 履歴は `userData/notifications.json` に保存（件数上限あり）。

### 4.4 クイック設定（A）
バーから開くパネル（オーバーレイ窓）：
- 音量スライダー、ミュート、出力／入力デバイスの切替（`wpctl`）
- Wi-Fi：オン/オフ、一覧、接続（パスワード入力は NetworkManager の秘密情報エージェント＝KDE の kded 側に任せる）
- Bluetooth：オン/オフ、既知デバイスへの接続（`bluetoothctl` または BlueZ D-Bus）
- 明るさスライダー（powerdevil 経由）
- 夜間モード（KWin の NightLight D-Bus）
- 電源プロファイル（`powerprofilesctl`）
- ロック・ログアウト・スリープ・再起動・電源オフ

### 4.5 OSD とメディアキー（A）
- 音量キー／ミュートキー：`sys/keys.js` で受けて `wpctl` を呼び、OSD を表示。
- 明るさキー：powerdevil がそのまま処理する。表示要求は `org.kde.osdService` で受けて OSD を出す。
- 再生キー：MPRIS に送る。

### 4.6 システムトレイ（B）
- `org.kde.StatusNotifierWatcher` と Host を実装。Item のアイコン（名前またはピクセル）・ツールチップ・クリックを扱う。
- 右クリックメニューは `com.canonical.dbusmenu` を読み、PRELUDE のメニューとして描画。
- 古い XEmbed 方式のトレイは対象外（`xembedsniproxy` が変換してくれる）。

### 4.7 ファイラの強化（B）
- 選択（複数・範囲）、コピー/切り取り/貼り付け、名前変更、新規フォルダ
- 削除はゴミ箱へ（`gio trash`）。ゴミ箱の表示・元に戻す
- 外部ドライブの検出とマウント／取り外し（`udisksctl`、または UDisks2 D-Bus で変化を購読）
- ドラッグ＆ドロップ（ペイン間、外部アプリへ）
- 画像のサムネイル（`~/.cache/thumbnails` を読む、無ければ生成）
- 「プログラムから開く」（`.desktop` の MimeType から候補）
- 右クリックメニュー

### 4.8 ワークスペース（B）
- ワークスペース＝タブ群と分割配置のまとまり（「仕事」「調べ物」など）。今の `workspace.json` を複数持つ形にする。
- 切替時、裏のワークスペースの Webビューは隠し、取り込みアプリは最小化する。
- KDE の仮想デスクトップとは連動させない（取り込みアプリの管理が複雑になるため。必要になったら検討）。

### 4.9 ホーム画面・壁紙（B）
- ペイン種類「ホーム」：壁紙の上に時計・カレンダー・最近のファイル・ピン留め・メディア。
- タブが無いときの背景もホーム画面にする（デスクトップの代わり）。

### 4.10 端末ペイン（B）
- xterm.js + node-pty。node-pty はネイティブモジュールなので `@electron/rebuild` が必要。
- 開始フォルダはフォルダペインの現在地を引き継ぐ。

### 4.11 その他（C）
- クリップボード履歴（ランチャーから検索）
- スクリーンショット（Spectacle を呼ぶか、portal の Screenshot）
- 画像・テキストのプレビューペイン
- カレンダー連携

## 5. 安全装置（必須）

plasmashell を止めた状態で PRELUDE が落ちると、**画面に何も操作できるものが無くなる。** 先に作る：

1. **systemd ユーザーサービスで起動する**（`prelude-shell.service`、`Restart=on-failure`、`RestartSec=1`）。
   短時間に何度も落ちたら（`StartLimitBurst`）`plasma-plasmashell.service` を起動し直す（`OnFailure=`）。
2. **非常用ショートカット**：KWin スクリプトとは別に、KDE のカスタムショートカットで
   `Ctrl+Alt+Shift+P` → `systemctl --user start plasma-plasmashell.service`、`Ctrl+Alt+T` → Konsole を登録しておく。
3. **切替スクリプト** `scripts/shell-mode.sh on|off`：
   - `on`：`plasma-plasmashell.service` を mask、`prelude-shell.service` を有効化
   - `off`：その逆。TTY（Ctrl+Alt+F3）からでも戻せるようにする

## 6. 段階的な導入

| 段階 | 内容 | plasmashell |
|---|---|---|
| **0. 下準備** | `sys/` への分割、`dbus-next` 導入、`org.prelude.Shell` 公開、KWin→PRELUDE の連絡を D-Bus に置換、安全装置（5章） | 動かしたまま |
| **1. バーとランチャー** | ステータスバー、オーバーレイ窓、ランチャー、Super キー | 動かしたまま（パネルは自動で隠す設定に） |
| **2. クイック設定・メディア** | 音量・Wi-Fi・BT・明るさ・電源、MPRIS | 動かしたまま |
| **3. 置き換え** | 通知サーバ、OSD、音量キー、壁紙・ホーム画面 → **ここで plasmashell を止める** | 停止（切替スクリプトで戻せる） |
| **4. 仕上げ** | トレイ、ファイラ強化、ワークスペース、端末ペイン | 停止 |

通知・OSD は plasmashell と同じ D-Bus 名を取り合うため、**段階3より前には入れない。**
段階1・2は plasmashell と共存したまま普段使いで試せる。

## 6.0 実機テストで分かったこと（systemd 化）

- 停止時、systemd は Chromium の全子プロセスに同時に SIGTERM を送り、メインプロセスが SIGTRAP でクラッシュして「失敗」扱いになり、
  OnFailure で plasmashell が復活してしまう → `KillMode=mixed`（メインだけに SIGTERM）＋ `main.js` で SIGTERM を通常終了に。
  ユニットは npm ラッパー（`.bin/electron`）を経由せず `node_modules/electron/dist/electron` を直接起動する。
- `org.freedesktop.Notifications` は plasmashell が止まっても「起動可能な名前」として残るが、起動されるのは
  `plasma_waitforname`（名前が取られるのを待つだけ）。段階3で PRELUDE がこの名前を取れば、それまでの通知は待機後に届く。
- plasmashell は他のサービスから要求されていないので、止めてもセッションは落ちない。

## 6.1 段階0の実装状況

- 済：`dbus-next` 導入、`sys/bus.js`（`org.prelude.Shell` を公開。KWinスクリプトは `Report(kind, json)` を呼ぶ）、
  `appembed.js` の `dbus-monitor` 横取りを置換（D-Bus に繋がらない環境では従来のPID監視）、`sys/apps.js`（`XDG_DATA_DIRS` を見る）、
  `systemd/` と `scripts/shell-mode.sh`（実機確認済み。2026-10-08 に `scripts/shell-mode-test.sh` で、`on --now`→PRELUDE起動→`off --now` と、PRELUDE が落ち続けたときの OnFailure による plasmashell 自動復旧を確認）
- 未：非常用ショートカットの登録（KDE の設定画面で手動登録が必要。TTY からの `shell-mode.sh off --now` が最後の手段）、
  `sys/` の共通 `init(ctx)` 化（モジュールが2つのうちは見送り。3つ目から導入する）

## 6.2 段階1の実装状況（バーとランチャー）

決定：バーは**下端・細め(24px)**、アプリは**既定でペインに取り込む**（設定 `behavior.appOpen` で別ウィンドウにも変更可）。

- 済：`sys/status.js`（電池=/sys、音量=wpctl+`pactl subscribe`、ネットワーク=nmcli）、`src/bar.js`、`src/launcher.js`、
  ショートカット `shortcuts.launcher`（既定 Ctrl+Shift+P）、`appUsage` による使用頻度順
- 設計からの変更：ランチャーは**別の透明ウィンドウではなく、既存のポップアップと同じ「Webビューを一時的に隠す」方式**にした
  （取り込みアプリも同時に隠れる）。通知・OSD のように「他のアプリの上に重ねて出したい」ものは、段階3で透明ウィンドウを作る。
- 未：Super キーでの起動（KDE 側の Super 割り当てを外す必要がある。決まるまで Ctrl+Shift+P）、ウィンドウ（取り込んでいない窓）のタスク表示、メディア表示

## 6.3 段階2の実装状況（クイック設定・メディア）

- 済：`sys/quick.js`（音量・出力デバイス=wpctl、Wi-Fi=nmcli、Bluetooth=bluetoothctl、明るさ=powerdevil の D-Bus、
  電源プロファイル=powerprofilesctl、ロック/スリープ=loginctl/systemctl、ログアウト/再起動/電源オフ=`org.kde.LogoutPrompt`）、
  `sys/media.js`（MPRIS）、`sys/keys.js`（KWinスクリプトの `registerShortcut`。既定 Meta+Space）、`src/quick.js`
- 方針：Wi-Fi は**登録済みネットワークへの接続のみ**（パスワードを PRELUDE に入力させない。未登録は KDE のネットワーク設定で一度登録）。
  明るさは下限1%（0%は真っ暗で戻せなくなるので不可。暗い部屋では1〜2%を使う人もいる）。画面側から送る操作は固定の種類だけ受け付け、引数は main 側で検証。
- 未：夜間モード（KWin の NightLight は on/off の永続設定が D-Bus に無く、kwinrc の書き換えが必要なので見送り）、
  Wi-Fi のパスワード入力（KDE の秘密情報エージェント経由にする設計を段階4で検討）、取り込んでいない窓のタスク表示

## 6.4 段階3a の実装状況（通知サーバ）

- 済：`sys/notify.js`（Notify / CloseNotification / GetCapabilities / GetServerInformation、NotificationClosed / ActionInvoked。
  置き換え更新・期限・緊急（消えない）・おやすみモード・履歴100件を `userData/notifications.json` に保存）、
  `sys/overlay.js`（透明・枠なし・最前面の窓。KWinスクリプトがタイトル `PRELUDE-overlay:<種類>` を見て右上に配置）、
  `sys/icons.js`、`src/notifications.js`（通知センター）
- 名前の扱い：置き換えずに待ち行列に入る（`requestName(name, 0)`）。plasmashell が止まれば D-Bus が自動で PRELUDE に渡す。
  テスト時は環境変数 `PRELUDE_NOTIFY_NAME` で別名にできる。
- 実機確認：別名で、表示・アクション・既定アクション・置き換え・期限・緊急・おやすみ・履歴。さらに `scripts/shell-mode-test.sh` で、
  plasmashell を止めた状態で本物の名前を PRELUDE が引き継ぎ `notify-send` がトーストになること、元に戻すと plasmashell が取り返すことを確認。
- 設計上の割り切り：本文はプレーンテキストのみ表示（`body-markup` は広告しない。HTMLは解釈しない）。`image-data` ヒントと
  サウンドは未対応。オーバーレイは KDE 以外では無効（履歴は残る）。
- 未（段階3の残り）：OSD（音量・明るさのキー）、壁紙・ホーム画面

## 6.5 段階3b の実装状況（OSD）

- 済：`sys/osd.js`（音量=status.js の変化通知、明るさ=powerdevil の `brightnessChanged` シグナル）、オーバーレイの `osd` 種類
  （画面中央下、約1.5秒）。PRELUDE 自身のスライダー操作では出さない（`quiet`）。
  緊急用ショートカット Meta+Shift+B で明るさ50%（`keys.js` の `raise:false` = 前面に出さない）。
  明るさが0%（四捨五入して0）のまま8秒続いたら20%へ自動で戻す安全策（`makeLowGuard`。1〜2%は暗いだけで見えるので対象外）。
- 不具合の修正：`pactl subscribe` の出力が日本語ロケールだと `sink` に一致せず、音量の変化が最大15秒遅れていた → `LC_ALL=C` で起動。
- **既知の未対応：plasmashell が止まっている間、キーボードの音量キーが効かない。**
  `Volume Up/Down/Mute` は kglobalaccel 上で停止中の kmix の持ち物のまま残り、PRELUDE の KWin ショートカットには割り当てられない
  （実機で `PRELUDE volumeUp=,none` のまま空になることを確認。ショートカットの呼び出しでは動くが、物理キーでは動かない）。
  音量はバーのホイール・クイック設定・OSD で操作できる。
  取る方法は kmix コンポーネントになりすます登録（`doRegister` + `setShortcut`）だが、PRELUDE の終了時に KDE が
  「kmix は無効」と判断すると、plasmashell が動いている通常時の音量キーまで効かなくなる恐れがあり、慎重な検証が要る。
  明るさキーは powerdevil が処理するので影響なし（OSD は PRELUDE が出す）。
- 未（段階3の残り）：壁紙・ホーム画面

## 7. 決めておきたいこと

- ~~バーは上端か下端か~~ → 下端・細め
- ~~アプリ起動の既定~~ → ペインに取り込む
- ~~Super キー~~ → Super 単独ではなく **Meta+Space**（KWin のグローバルショートカット）
- 外部ディスプレイ（複数画面）を使うか。使うなら PRELUDE 本体を画面ごとに出すか、主画面だけにするか
- Windows 版との共通化をどこまで残すか（`sys/` は Linux 専用にして、無い環境では機能を隠す想定）
