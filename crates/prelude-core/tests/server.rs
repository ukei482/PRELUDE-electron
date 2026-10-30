// ソケットのテスト。本物のソケットでコアを動かし、つないで確かめる
use prelude_core::server::{run, Options};
use serde_json::{json, Value};
use std::path::PathBuf;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::unix::{OwnedReadHalf, OwnedWriteHalf};
use tokio::net::UnixStream;
use tokio::sync::oneshot;

fn dir(name: &str) -> PathBuf {
    let d = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("srv").join(name);
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(d.join("spaces")).unwrap();
    d
}

fn opts(d: &std::path::Path) -> Options {
    Options {
        socket: d.join("s.sock"),
        state_file: d.join("state/core.json"),
        home: "/home/u".into(),
        spaces_root: d.join("spaces"),
        recipes_dir: d.join("recipes"),
        embed: false,
    }
}

/// コアを動かす。返す送り口に送ると止まる
async fn start(d: &std::path::Path) -> (oneshot::Sender<()>, tokio::task::JoinHandle<std::io::Result<()>>) {
    let (tx, rx) = oneshot::channel::<()>();
    let h = tokio::spawn(run(opts(d), async {
        let _ = rx.await;
    }));
    for _ in 0..100 {
        if d.join("s.sock").exists() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    (tx, h)
}

struct Client {
    r: BufReader<OwnedReadHalf>,
    w: OwnedWriteHalf,
}

impl Client {
    async fn connect(d: &std::path::Path) -> Client {
        let (r, w) = UnixStream::connect(d.join("s.sock")).await.unwrap().into_split();
        Client { r: BufReader::new(r), w }
    }
    async fn send(&mut self, v: Value) {
        self.w.write_all(format!("{v}\n").as_bytes()).await.unwrap();
    }
    async fn recv(&mut self) -> Value {
        let mut l = String::new();
        tokio::time::timeout(Duration::from_secs(5), self.r.read_line(&mut l)).await.expect("timeout").unwrap();
        serde_json::from_str(&l).unwrap()
    }
    /// 返事が来るまで読む(途中のイベントは捨てる)
    async fn call(&mut self, id: u64, cmd: &str, args: Value) -> Value {
        self.send(json!({ "id": id, "cmd": cmd, "args": args })).await;
        loop {
            let v = self.recv().await;
            if v.get("id").and_then(Value::as_u64) == Some(id) {
                return v;
            }
        }
    }
}

#[tokio::test]
async fn hello_コマンド_イベント() {
    let d = dir("events");
    let (stop, h) = start(&d).await;

    // hello の前のコマンド・知らない版は断る
    let mut a = Client::connect(&d).await;
    assert_eq!(a.call(1, "state.get", json!({})).await["error"]["code"], "protocol");
    assert_eq!(a.call(2, "hello", json!({ "protocol": 99 })).await["error"]["code"], "protocol");
    assert_eq!(a.call(3, "hello", json!({ "protocol": 1, "name": "test" })).await["result"]["protocol"], 1);
    assert_eq!(a.call(4, "no.such", json!({})).await["error"]["code"], "unknown_command");
    assert_eq!(a.call(5, "tab.close", json!({ "tab": "x" })).await["error"]["code"], "bad_request");
    a.send(json!("not a request")).await;
    assert_eq!(a.recv().await["error"]["code"], "bad_request");

    // subscribe すると、返事の後に今の状態が来て、変わるたびに来る
    let mut b = Client::connect(&d).await;
    b.send(json!({ "id": 1, "cmd": "hello", "args": { "protocol": 1, "subscribe": true } })).await;
    assert_eq!(b.recv().await["id"], 1);
    let first = b.recv().await;
    assert_eq!((first["event"].as_str(), first["data"]["space"].as_str()), (Some("state"), Some("Main")));

    let r = a.call(6, "tab.open", json!({ "pane": { "type": "web", "url": "https://a.example/" } })).await;
    let id = r["result"].as_u64().unwrap();
    let ev = b.recv().await;
    assert_eq!(ev["data"]["tabs"][0]["id"].as_u64(), Some(id));
    assert_eq!(ev["data"]["active"].as_u64(), Some(id));
    // 状態の変わらないコマンド(場所の知らせ・state.get)ではイベントは来ない
    a.call(7, "pane.place", json!({ "rects": [] })).await;
    a.call(8, "tab.close", json!({})).await;
    let ev = b.recv().await;
    assert_eq!(ev["data"]["tabs"], json!([]));

    stop.send(()).unwrap();
    h.await.unwrap().unwrap();
    assert!(!d.join("s.sock").exists()); // 止まるとソケットを消す
}

#[tokio::test]
async fn 状態を書き出し_起動し直すと戻る() {
    let d = dir("restart");
    let (stop, h) = start(&d).await;
    let mut a = Client::connect(&d).await;
    a.call(1, "hello", json!({ "protocol": 1 })).await;
    a.call(2, "tab.open", json!({ "pane": { "type": "files", "path": "/home/u/src" } })).await;
    // 2 つ目のコアは起動できない
    let second = run(opts(&d), async {}).await;
    assert_eq!(second.unwrap_err().kind(), std::io::ErrorKind::AddrInUse);
    drop(a);
    stop.send(()).unwrap();
    h.await.unwrap().unwrap();
    let text = std::fs::read_to_string(d.join("state/core.json")).unwrap();
    assert!(text.contains("~/src"), "{text}");

    let (stop, h) = start(&d).await;
    let mut a = Client::connect(&d).await;
    a.call(1, "hello", json!({ "protocol": 1 })).await;
    let s = a.call(2, "state.get", json!({})).await;
    assert_eq!(s["result"]["tabs"][0]["layout"]["path"], "/home/u/src");
    stop.send(()).unwrap();
    h.await.unwrap().unwrap();
}

#[tokio::test]
async fn 壊れた状態のファイルは退けて空から始める() {
    let d = dir("broken");
    std::fs::create_dir_all(d.join("state")).unwrap();
    std::fs::write(d.join("state/core.json"), "{ broken").unwrap();
    let (stop, h) = start(&d).await;
    let mut a = Client::connect(&d).await;
    a.call(1, "hello", json!({ "protocol": 1 })).await;
    assert_eq!(a.call(2, "state.get", json!({})).await["result"]["tabs"], json!([]));
    assert!(d.join("state/core.json.bad").exists());
    stop.send(()).unwrap();
    h.await.unwrap().unwrap();
}
