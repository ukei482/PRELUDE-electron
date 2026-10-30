//! PRELUDE のコア(docs/redesign-rust.md の判断 1)。画面を持たない常駐のプログラムで、状態の持ち主。
//! UI・`prelude` コマンド・プラグインは、Unix ソケットの約束(prelude-proto)でコマンドを送り、イベントを受ける。
//!
//! - [`state`] — スペース・タブ・分割・開いているブックマークと、コマンドの処理
//! - [`server`] — ソケット(1 行 1 つの JSON)、状態の書き出し
pub mod server;
pub mod state;
