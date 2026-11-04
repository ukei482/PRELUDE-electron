//! PRELUDE のコア(docs/redesign-rust.md の判断 1)。画面を持たない常駐のプログラムで、状態の持ち主。
//! UI・`prelude` コマンド・プラグインは、Unix ソケットの約束(prelude-proto)でコマンドを送り、イベントを受ける。
//!
//! - [`state`] — スペース・タブ・分割・開いているブックマークと、コマンドの処理
//! - [`server`] — ソケット(1 行 1 つの JSON)、状態の書き出し
//! - [`portal`] — アプリの「開く」「保存」のダイアログ(xdg-desktop-portal の FileChooser のバックエンド)
//! - [`web`] — Web のペイン(Chromium を CDP で動かす)
//! - [`compositor`] — 窓を並べる相手(コンポジタ)の差し替え口。[`embed`] はその KWin の裏側(アプリの窓の取り込み)。[`recipes`] はペインのレシピ、[`apps`] はインストール済みのアプリ、[`procs`] は /proc、[`xdg`] はコマンド・データのフォルダの探し方
pub mod apps;
pub mod compositor;
pub mod embed;
pub mod portal;
pub mod procs;
pub mod recipes;
pub mod server;
pub mod state;
pub mod web;
pub mod xdg;
