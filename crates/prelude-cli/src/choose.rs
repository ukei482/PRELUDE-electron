//! `prelude choose`: アプリの「開く」「保存」のダイアログの選ぶ画面(docs/redesign-rust.md の「アプリのダイアログ」)。
//! コアのポータルが `chooser.toml` の既定として起動する。端末の中で yazi を `--chooser-file` 付きで開き、選んだパスを 1 行ずつ `--output` に書く。
//! 何も書かずに終われば取り消し。
//!
//! 保存: yazi には新しい名前を打つ画面が無いので、最初のフォルダに保存する名前の仮のファイルを置いて選んでもらう
//! (xdg-desktop-portal-termfilechooser と同じやり方)。名前を変えても、別のフォルダへ移って別のファイルを選んでもよい。
//! 終わったら、仮のファイル(中身が [`MARK`] のもの)は名前が変わっていても消す。アプリがそのパスに書く
use std::path::{Path, PathBuf};

/// 既定の端末(この後に `yazi --chooser-file <出力> <場所>` が続く)。`--separate` は、動いている konsole に任せずに自分で待つため
pub const TERMINAL: &str = "konsole --separate -e";

/// 仮のファイルの中身。これと同じ中身のファイルは、選んだ後に消す
pub const MARK: &str = "PRELUDE: 保存の仮のファイルです。この名前で保存するならそのまま選び、変えるなら名前を変えるか、別のフォルダへ移ってから選んでください。\n";

#[derive(Debug, PartialEq)]
pub struct Opts {
    pub mode: String,
    pub output: PathBuf,
    pub name: String,
    pub terminal: String,
    pub folder: PathBuf,
}

pub fn parse(args: &[&str]) -> Result<Opts, String> {
    let usage = "usage: prelude choose <open|open-multiple|directory|save> --output <file> [--name <name>] [--terminal <cmd>] <folder>";
    let mut it = args.iter();
    let mode = it.next().ok_or(usage)?.to_string();
    if !["open", "open-multiple", "directory", "save"].contains(&mode.as_str()) {
        return Err(usage.into());
    }
    let (mut output, mut name, mut terminal, mut folder) = (None, String::new(), TERMINAL.to_string(), None);
    while let Some(a) = it.next() {
        match *a {
            "--output" => output = Some(PathBuf::from(it.next().ok_or(usage)?)),
            "--name" => name = it.next().ok_or(usage)?.to_string(),
            "--terminal" => terminal = it.next().ok_or(usage)?.to_string(),
            f if folder.is_none() => folder = Some(PathBuf::from(f)),
            _ => return Err(usage.into()),
        }
    }
    let output = output.ok_or(usage)?;
    let folder = folder.filter(|f| f.is_dir()).unwrap_or_else(|| PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| "/".into())));
    Ok(Opts { mode, output, name, terminal, folder })
}

fn quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

/// yazi に渡す場所と、置いた仮のファイル。保存なら仮のファイルを置き(同じ名前のファイルがあれば置かずにそれを指す)、そこにカーソルを置く
pub fn prepare(o: &Opts) -> std::io::Result<(PathBuf, Option<PathBuf>)> {
    if o.mode != "save" {
        return Ok((o.folder.clone(), None));
    }
    let name = if o.name.trim().is_empty() { "untitled" } else { o.name.trim() };
    let p = o.folder.join(name.replace('/', "_"));
    if p.exists() {
        return Ok((p, None));
    }
    std::fs::write(&p, MARK)?;
    Ok((p.clone(), Some(p)))
}

fn is_mark(p: &Path) -> bool {
    std::fs::metadata(p).is_ok_and(|m| m.len() == MARK.len() as u64) && std::fs::read_to_string(p).is_ok_and(|s| s == MARK)
}

/// 選び終わった後の片付け。選んだもののうち仮のファイル(名前を変えたものも)と、置いた仮のファイルを消す。選んだパスはそのまま
pub fn finish(output: &Path, placeholder: Option<&Path>) {
    let chosen = std::fs::read_to_string(output).unwrap_or_default();
    for p in chosen.lines().map(Path::new).chain(placeholder) {
        if is_mark(p) {
            let _ = std::fs::remove_file(p);
        }
    }
}

pub fn run(args: &[&str]) -> Result<(), String> {
    let o = parse(args)?;
    let (target, placeholder) = prepare(&o).map_err(|e| format!("{}: {e}", o.folder.display()))?;
    let cmd = format!("{} yazi --chooser-file {} {}", o.terminal, quote(&o.output.to_string_lossy()), quote(&target.to_string_lossy()));
    let r = std::process::Command::new("/bin/sh").arg("-c").arg(&cmd).status();
    finish(&o.output, placeholder.as_deref());
    r.map(|_| ()).map_err(|e| format!("{cmd}: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("prelude-choose-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn 引数() {
        let o = parse(&["save", "--output", "/run/o", "--name", "a.txt", "/"]).unwrap();
        assert_eq!((o.mode.as_str(), o.output.to_str(), o.name.as_str(), o.terminal.as_str()), ("save", Some("/run/o"), "a.txt", TERMINAL));
        assert_eq!(o.folder, PathBuf::from("/"));
        assert!(parse(&["delete", "--output", "/o", "/"]).is_err());
        assert!(parse(&["open", "/"]).is_err()); // --output が要る
                                                 // 無いフォルダはホーム
        assert_ne!(parse(&["open", "--output", "/o", "/nonexistent/x"]).unwrap().folder, PathBuf::from("/nonexistent/x"));
    }

    #[test]
    fn 開くはフォルダをそのまま() {
        let d = tmp("open");
        let o = parse(&["open", "--output", "/o", d.to_str().unwrap()]).unwrap();
        assert_eq!(prepare(&o).unwrap(), (d.clone(), None));
        let _ = std::fs::remove_dir_all(&d);
    }

    /// 端末の代わりに、yazi の引数(--chooser-file <出力> <場所>)を受けて、選んだことにするシェルの関数
    fn fake(choose: &str) -> String {
        format!("f() {{ {choose}; }}; f")
    }

    #[test]
    fn 保存_仮のファイルをそのまま選ぶ() {
        let d = tmp("save");
        let out = d.join("out");
        // $3 = 出力、$4 = 場所(仮のファイル)。仮のファイルをそのまま選ぶ
        let term = fake(r#"printf '%s\n' "$4" > "$3""#);
        run(&["save", "--output", out.to_str().unwrap(), "--name", "新規.txt", "--terminal", &term, d.to_str().unwrap()]).unwrap();
        let chosen = std::fs::read_to_string(&out).unwrap();
        assert_eq!(chosen.trim(), d.join("新規.txt").to_str().unwrap());
        assert!(!d.join("新規.txt").exists()); // 仮のファイルは消える。アプリがここに書く
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn 保存_名前を変えた仮のファイルも消す_取り消しも消す() {
        let d = tmp("rename");
        let out = d.join("out");
        let term = fake(r#"mv "$4" "$(dirname "$4")/別名.txt"; printf '%s\n' "$(dirname "$4")/別名.txt" > "$3""#);
        run(&["save", "--output", out.to_str().unwrap(), "--name", "新規.txt", "--terminal", &term, d.to_str().unwrap()]).unwrap();
        assert_eq!(std::fs::read_to_string(&out).unwrap().trim(), d.join("別名.txt").to_str().unwrap());
        assert!(!d.join("別名.txt").exists() && !d.join("新規.txt").exists());
        // 何も選ばずに終わる(取り消し)と、出力は空で、仮のファイルも残らない
        let out2 = d.join("out2");
        run(&["save", "--output", out2.to_str().unwrap(), "--name", "x.txt", "--terminal", &fake("true"), d.to_str().unwrap()]).unwrap();
        assert!(!out2.exists() && !d.join("x.txt").exists());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn 保存_同じ名前のファイルがあれば消さない() {
        let d = tmp("exists");
        std::fs::write(d.join("a.txt"), "大事").unwrap();
        let out = d.join("out");
        let term = fake(r#"printf '%s\n' "$4" > "$3""#);
        run(&["save", "--output", out.to_str().unwrap(), "--name", "a.txt", "--terminal", &term, d.to_str().unwrap()]).unwrap();
        assert_eq!(std::fs::read_to_string(d.join("a.txt")).unwrap(), "大事");
        let _ = std::fs::remove_dir_all(&d);
    }
}
