//! Chromium の native messaging とコアの間の橋渡し(`prelude-core --web-host`)。Chromium が起動する。
//!
//! - Chromium とは標準入出力で、4 バイトの長さ(この機械の並び)+ JSON。コアとは Unix ソケットで、1 行 1 つの JSON
//! - どちらかが閉じたら終わる
use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::net::UnixStream;

/// 拡張機能への 1 通は 1MB まで(Chromium の決まり)
const MAX_TO_EXTENSION: usize = 1024 * 1024;

/// 1 通を読む。閉じていれば None
pub fn read_frame(r: &mut impl Read) -> Option<Vec<u8>> {
    let mut head = [0u8; 4];
    r.read_exact(&mut head).ok()?;
    let mut body = vec![0u8; u32::from_ne_bytes(head) as usize];
    r.read_exact(&mut body).ok()?;
    Some(body)
}

/// 1 通を書く
pub fn write_frame(w: &mut impl Write, body: &[u8]) -> std::io::Result<()> {
    w.write_all(&(body.len() as u32).to_ne_bytes())?;
    w.write_all(body)?;
    w.flush()
}

/// 橋渡しを動かす。ソケットは `PRELUDE_WEB_SOCK`(コアが Chromium の環境に入れる)
pub fn run() -> Result<(), String> {
    let path = std::env::var("PRELUDE_WEB_SOCK").map_err(|_| "PRELUDE_WEB_SOCK is not set".to_string())?;
    let sock = UnixStream::connect(&path).map_err(|e| format!("cannot connect to {path} ({e})"))?;
    let mut to_core = sock.try_clone().map_err(|e| e.to_string())?;
    std::thread::spawn(move || {
        let mut out = std::io::stdout().lock();
        for line in BufReader::new(sock).lines() {
            let Ok(line) = line else { break };
            if line.len() <= MAX_TO_EXTENSION && write_frame(&mut out, line.as_bytes()).is_err() {
                break;
            }
        }
        std::process::exit(0);
    });
    let mut stdin = std::io::stdin().lock();
    while let Some(mut body) = read_frame(&mut stdin) {
        // JSON.stringify の結果に改行は入らない
        body.push(b'\n');
        if to_core.write_all(&body).is_err() {
            break;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 長さ付きの読み書き() {
        let mut b = vec![];
        write_frame(&mut b, b"{\"a\":1}").unwrap();
        write_frame(&mut b, "日本語".as_bytes()).unwrap();
        let mut r = &b[..];
        assert_eq!(read_frame(&mut r).unwrap(), b"{\"a\":1}");
        assert_eq!(read_frame(&mut r).unwrap(), "日本語".as_bytes());
        assert_eq!(read_frame(&mut r), None);
        // 途中で切れていたら終わり
        assert_eq!(read_frame(&mut &b[..6]), None);
    }
}
