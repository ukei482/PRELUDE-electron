//! ソケットの側。1 行 1 つの JSON を読み、[`Core`] に渡し、返事と `state` のイベントを書く。状態が変わったら書き出す。
//!
//! - つないだら最初に `hello`。版がコアの話せるもの(`SUPPORTED`)でなければ断る
//! - `hello` で `subscribe` したつなぎには、すぐに今の `state` を送り、その後は変わるたびに送る
//! - ソケットは自分だけが使えるようにする(フォルダ 0700・ソケット 0600)
use crate::embed::{self, Note};
use crate::recipes;
use crate::state::{Core, Saved};
use prelude_proto::{Command, ErrorCode, Event, Hello, Request, Response, MAX_LINE, PROTOCOL, SUPPORTED};
use serde_json::{json, Value};
use std::future::Future;
use std::io;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::{broadcast, mpsc, Notify};

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
/// 状態が変わってから書き出すまで待つ時間(続けて変わったときにまとめる)
const SAVE_DELAY: Duration = Duration::from_millis(200);

pub struct Options {
    pub socket: PathBuf,
    /// 残す状態のファイル
    pub state_file: PathBuf,
    pub home: String,
    /// スペースのフォルダ(この下のフォルダ 1 つがスペース 1 つ)
    pub spaces_root: PathBuf,
    /// ペインのレシピのフォルダ
    pub recipes_dir: PathBuf,
    /// アプリの窓を取り込む(KDE のとき)。テストでは false
    pub embed: bool,
}

/// このコアの印。既定のソケットなら空、別のソケット(試すときなど)ならその場所から作った短い印。
/// D-Bus の名前・KWin スクリプトの名前に付けて、ふだんのコアと取り合わないようにする
pub fn instance_tag(socket: &Path) -> String {
    if socket == prelude_proto::default_socket_path() {
        return String::new();
    }
    // FNV-1a(版が変わっても同じ値になるように、自前で計算する)
    let mut h: u64 = 0xcbf29ce484222325;
    for b in socket.as_os_str().as_encoded_bytes() {
        h = (h ^ u64::from(*b)).wrapping_mul(0x100000001b3);
    }
    format!(".d{:08x}", h as u32)
}

struct Shared {
    core: Mutex<Core>,
    events: broadcast::Sender<Arc<str>>,
    save: Notify,
    embed: Option<embed::Handle>,
}

impl Shared {
    /// 取り込みに、今の状態の取り込むペインを送る
    fn sync_embed(&self, core: &Core) {
        if let Some(e) = &self.embed {
            e.want(core.wants());
        }
    }

    fn state_line(core: &Core) -> Arc<str> {
        line(&Event { event: "state".into(), data: json!(core.state()) }).into()
    }
}

fn line(v: &impl serde::Serialize) -> String {
    serde_json::to_string(v).unwrap_or_default()
}

/// 残した状態を読む。無ければ None。壊れていれば、横に `.bad` として退けて None
pub fn load_saved(path: &Path) -> Option<Saved> {
    let text = std::fs::read_to_string(path).ok()?;
    match serde_json::from_str(&text) {
        Ok(s) => Some(s),
        Err(e) => {
            eprintln!("prelude-core: cannot read {} ({e}). Moved it aside", path.display());
            let _ = std::fs::rename(path, path.with_extension("json.bad"));
            None
        }
    }
}

/// 書き出す(一時ファイルに書いてから置き換える。途中で落ちても前のファイルが残る)
fn write_saved(path: &Path, s: &Saved) -> io::Result<()> {
    if let Some(d) = path.parent() {
        std::fs::create_dir_all(d)?;
    }
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_string_pretty(s).map_err(io::Error::other)? + "\n")?;
    std::fs::rename(tmp, path)
}

/// ソケットを置けるようにする。もう動いているコアがあれば Err
fn prepare_socket(path: &Path) -> io::Result<()> {
    if let Some(d) = path.parent() {
        std::fs::create_dir_all(d)?;
        std::fs::set_permissions(d, std::fs::Permissions::from_mode(0o700))?;
    }
    if path.exists() {
        if std::os::unix::net::UnixStream::connect(path).is_ok() {
            return Err(io::Error::new(io::ErrorKind::AddrInUse, format!("prelude-core is already running ({})", path.display())));
        }
        std::fs::remove_file(path)?; // 前に落ちたときの残り
    }
    Ok(())
}

/// コアを動かす。shutdown が終わったら、状態を書き出し、ソケットを消して戻る
pub async fn run(opt: Options, shutdown: impl Future<Output = ()>) -> io::Result<()> {
    prepare_socket(&opt.socket)?;
    let listener = UnixListener::bind(&opt.socket)?;
    std::fs::set_permissions(&opt.socket, std::fs::Permissions::from_mode(0o600))?;

    let mut core = Core::new(opt.home.clone(), opt.spaces_root.clone(), load_saved(&opt.state_file));
    core.set_recipes(recipes::load(&opt.recipes_dir));
    let (events, _) = broadcast::channel(64);
    let (notes_tx, mut notes) = tokio::sync::mpsc::unbounded_channel();
    let embed = if opt.embed { embed::start(&instance_tag(&opt.socket), notes_tx).await } else { None };
    let shared = Arc::new(Shared { core: Mutex::new(core), events, save: Notify::new(), embed });
    // 前回のタブのアプリを起動し直す
    shared.sync_embed(&shared.core.lock().unwrap());

    // 取り込みからの知らせ(窓が閉じた・題名が変わった)を状態に入れる
    let noter = {
        let shared = shared.clone();
        tokio::spawn(async move {
            while let Some(n) = notes.recv().await {
                let core = &mut *shared.core.lock().unwrap();
                let changed = match n {
                    Note::Exited { pane, had_window } => {
                        if !had_window {
                            eprintln!("prelude-core: no window found for pane {pane}");
                        }
                        core.note_exited(pane)
                    }
                    Note::Title { pane, title } => core.note_title(pane, title),
                };
                if changed {
                    let _ = shared.events.send(Shared::state_line(core));
                }
            }
        })
    };

    // 書き出し
    let saver = {
        let shared = shared.clone();
        let path = opt.state_file.clone();
        tokio::spawn(async move {
            loop {
                shared.save.notified().await;
                tokio::time::sleep(SAVE_DELAY).await;
                let s = shared.core.lock().unwrap().saved();
                if let Err(e) = write_saved(&path, &s) {
                    eprintln!("prelude-core: cannot save state ({e})");
                }
            }
        })
    };

    eprintln!("prelude-core {VERSION}: listening on {}", opt.socket.display());
    tokio::pin!(shutdown);
    loop {
        tokio::select! {
            _ = &mut shutdown => break,
            r = listener.accept() => match r {
                Ok((stream, _)) => { tokio::spawn(serve(stream, shared.clone())); }
                Err(e) => eprintln!("prelude-core: accept failed ({e})"),
            },
        }
    }

    saver.abort();
    noter.abort();
    if let Some(e) = &shared.embed {
        e.shutdown().await; // 起動したアプリを閉じる
    }
    let s = shared.core.lock().unwrap().saved();
    let saved = write_saved(&opt.state_file, &s);
    let _ = std::fs::remove_file(&opt.socket);
    saved
}

/// 1 つのつなぎ
async fn serve(stream: UnixStream, shared: Arc<Shared>) {
    let (r, mut w) = stream.into_split();
    let (out, mut out_rx) = mpsc::channel::<Arc<str>>(64);
    let writer = tokio::spawn(async move {
        while let Some(l) = out_rx.recv().await {
            if w.write_all(l.as_bytes()).await.is_err() || w.write_all(b"\n").await.is_err() {
                break;
            }
        }
    });
    let mut reader = BufReader::new(r);
    let mut greeted = false;
    let mut forward: Option<tokio::task::JoinHandle<()>> = None;
    loop {
        let mut buf = Vec::new();
        match (&mut reader).take(MAX_LINE as u64 + 1).read_until(b'\n', &mut buf).await {
            Ok(0) | Err(_) => break,
            Ok(_) => {}
        }
        if buf.len() > MAX_LINE {
            let _ = out.send(line(&Response::err(0, ErrorCode::BadRequest, "Line is too long")).into()).await;
            break;
        }
        if buf.iter().all(u8::is_ascii_whitespace) {
            continue;
        }
        let resp = match parse(&buf) {
            Err(r) => r,
            Ok(Request { id, command: Command::Hello { protocol, name: _, subscribe } }) => {
                if !SUPPORTED.contains(&protocol) {
                    Response::err(id, ErrorCode::Protocol, format!("Protocol {protocol} is not supported (supported: {SUPPORTED:?})"))
                } else {
                    greeted = true;
                    let resp = Response::ok(id, Hello { protocol, core: VERSION.into() });
                    if subscribe && forward.is_none() {
                        // 返事の後に今の状態を送り、その後は変わるたびに送る
                        let mut rx = shared.events.subscribe();
                        let first = Shared::state_line(&shared.core.lock().unwrap());
                        let out = out.clone();
                        let _ = out.send(line(&resp).into()).await;
                        forward = Some(tokio::spawn(async move {
                            if out.send(first).await.is_err() {
                                return;
                            }
                            loop {
                                match rx.recv().await {
                                    Ok(l) => {
                                        if out.send(l).await.is_err() {
                                            return;
                                        }
                                    }
                                    Err(broadcast::error::RecvError::Lagged(_)) => continue, // 次の state にすべて入っている
                                    Err(broadcast::error::RecvError::Closed) => return,
                                }
                            }
                        }));
                        continue;
                    }
                    resp
                }
            }
            Ok(Request { id, .. }) if !greeted => Response::err(id, ErrorCode::Protocol, format!("Send hello first (protocol {PROTOCOL})")),
            Ok(Request { id, command }) => {
                let (o, ev) = {
                    let mut core = shared.core.lock().unwrap();
                    let o = core.handle(command);
                    shared.sync_embed(&core);
                    let ev = o.changed.then(|| Shared::state_line(&core));
                    (o, ev)
                };
                if let Some(ev) = ev {
                    let _ = shared.events.send(ev);
                    shared.save.notify_one();
                }
                match o.reply {
                    Ok(v) => Response::ok(id, v),
                    Err((code, msg)) => Response::err(id, code, msg),
                }
            }
        };
        if out.send(line(&resp).into()).await.is_err() {
            break;
        }
    }
    if let Some(f) = forward {
        f.abort();
    }
    drop(out);
    let _ = writer.await;
}

/// 1 行 → Request。読めなければ返す Response
fn parse(buf: &[u8]) -> Result<Request, Response> {
    let v: Value = serde_json::from_slice(buf).map_err(|e| Response::err(0, ErrorCode::BadRequest, format!("Not JSON ({e})")))?;
    let id = v.get("id").and_then(Value::as_u64).unwrap_or(0);
    let cmd = v.get("cmd").and_then(Value::as_str).unwrap_or("").to_string();
    Request::from_value(v).map_err(|e| {
        let msg = e.to_string();
        if msg.contains("unknown variant") {
            Response::err(id, ErrorCode::UnknownCommand, format!("Unknown command: {cmd}"))
        } else {
            Response::err(id, ErrorCode::BadRequest, msg)
        }
    })
}
