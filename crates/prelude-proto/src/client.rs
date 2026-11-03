//! コアにつなぐ側(`prelude` コマンド・サイドバーなど)の、型付きのクライアント。std だけで動く(読むのは止まって待つ)。
//! 非同期の UI からは、別のスレッドで [`Client::read`] を回してイベントを受ける。
use crate::{BookmarkInfo, Incoming, State, PROTOCOL};
use serde::de::DeserializeOwned;
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;

pub struct Client {
    w: UnixStream,
    r: BufReader<UnixStream>,
    next: u64,
}

impl Client {
    /// コア([`crate::socket_path`])につないで `hello` を送る。subscribe なら、すぐに今の状態、その後は変わるたびに `state` のイベントが来る
    pub fn connect(name: &str, subscribe: bool) -> Result<Client, String> {
        let path = crate::socket_path();
        let w = UnixStream::connect(&path).map_err(|e| format!("Cannot connect to prelude-core at {} ({e})", path.display()))?;
        let r = BufReader::new(w.try_clone().map_err(|e| e.to_string())?);
        let mut c = Client { w, r, next: 0 };
        c.call("hello", json!({ "protocol": PROTOCOL, "name": name, "subscribe": subscribe }))?;
        Ok(c)
    }

    /// 次の 1 行(返事かイベント)を待って読む
    pub fn read(&mut self) -> Result<Incoming, String> {
        let mut l = String::new();
        if self.r.read_line(&mut l).map_err(|e| e.to_string())? == 0 {
            return Err("prelude-core closed the connection".into());
        }
        serde_json::from_str(&l).map_err(|e| format!("Bad answer from prelude-core ({e})"))
    }

    /// コマンドを送り、返事の result を返す(途中に来たイベントは捨てる)
    pub fn call(&mut self, cmd: &str, args: Value) -> Result<Value, String> {
        self.next += 1;
        let id = self.next;
        let req = json!({ "id": id, "cmd": cmd, "args": args });
        writeln!(self.w, "{req}").map_err(|e| e.to_string())?;
        loop {
            match self.read()? {
                Incoming::Response(r) if r.id == id || r.id == 0 => {
                    return match r.error {
                        Some(e) => Err(e.message),
                        None => Ok(r.result.unwrap_or(Value::Null)),
                    }
                }
                _ => continue,
            }
        }
    }

    /// [`Client::call`] の返事を型に読む
    pub fn get<T: DeserializeOwned>(&mut self, cmd: &str, args: Value) -> Result<T, String> {
        serde_json::from_value(self.call(cmd, args)?).map_err(|e| e.to_string())
    }

    pub fn state(&mut self) -> Result<State, String> {
        self.get("state.get", json!({}))
    }

    pub fn bookmarks(&mut self) -> Result<Vec<BookmarkInfo>, String> {
        self.get("bookmark.list", json!({}))
    }

    pub fn spaces(&mut self) -> Result<Vec<String>, String> {
        self.get("space.list", json!({}))
    }
}
