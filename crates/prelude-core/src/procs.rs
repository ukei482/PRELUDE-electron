//! /proc から、起動したアプリのプロセスを集める(Electron 版の appembed.js と同じ考え方)。
//! - sh -c やラッパー経由で起動したアプリも拾うため、起動した sh の子孫をたどる
//! - kate のように自分で fork して親から切り離されるアプリは子孫に居ないので、「起動の後に始まった、コマンド名が同じプロセス」も候補にする
use std::collections::HashMap;

const TICKS: f64 = 100.0; // CLK_TCK(Linux の標準値)

/// 起動してからの秒数
pub fn uptime() -> f64 {
    std::fs::read_to_string("/proc/uptime").ok().and_then(|s| s.split_whitespace().next()?.parse().ok()).unwrap_or(0.0)
}

struct Stat {
    pid: u32,
    ppid: u32,
    comm: String,
    /// 起動してから始まるまでの秒数
    start: f64,
}

fn stats() -> Vec<Stat> {
    let Ok(rd) = std::fs::read_dir("/proc") else { return vec![] };
    let mut out = Vec::new();
    for e in rd.flatten() {
        let Some(pid) = e.file_name().to_str().and_then(|s| s.parse::<u32>().ok()) else { continue };
        let Ok(st) = std::fs::read_to_string(format!("/proc/{pid}/stat")) else { continue };
        let (Some(a), Some(b)) = (st.find('('), st.rfind(')')) else { continue };
        let rest: Vec<&str> = st[b + 2..].split(' ').collect();
        let ppid = rest.get(1).and_then(|s| s.parse().ok()).unwrap_or(0);
        let start = rest.get(19).and_then(|s| s.parse::<f64>().ok()).unwrap_or(0.0) / TICKS;
        out.push(Stat { pid, ppid, comm: st[a + 1..b].to_lowercase(), start });
    }
    out
}

/// pid とその子孫
pub fn descendants(root: u32) -> Vec<u32> {
    let mut kids: HashMap<u32, Vec<u32>> = HashMap::new();
    for s in stats() {
        kids.entry(s.ppid).or_default().push(s.pid);
    }
    let mut out = vec![root];
    let mut i = 0;
    while i < out.len() {
        if let Some(k) = kids.get(&out[i]) {
            out.extend(k);
        }
        i += 1;
    }
    out
}

/// コマンドの実行ファイルの名前(小文字。引用符とフォルダは除く)
pub fn cmd_base(cmd: &str) -> String {
    let first = cmd.split_whitespace().next().unwrap_or("").replace(['\'', '"'], "");
    first.rsplit('/').next().unwrap_or("").to_lowercase()
}

/// launched_at(uptime の秒)より後に始まった、コマンド名が cmd と同じプロセス
pub fn fresh_pids(cmd: &str, launched_at: f64) -> Vec<u32> {
    let base = cmd_base(cmd);
    if base.is_empty() {
        return vec![];
    }
    stats()
        .into_iter()
        .filter(|s| s.start >= launched_at - 0.5 && !s.comm.is_empty() && (base.starts_with(&s.comm) || s.comm.starts_with(&base)))
        .map(|s| s.pid)
        .collect()
}

pub fn alive(pid: u32) -> bool {
    // SAFETY: シグナル 0 は送らずに、プロセスがあるかだけ確かめる
    unsafe { libc::kill(pid as i32, 0) == 0 }
}

/// 終了の合図を送る。権限が無ければ(snap のアプリなど)false
pub fn signal(pid: u32, sig: i32) -> bool {
    // SAFETY: 決まったシグナルを送るだけ
    let r = unsafe { libc::kill(pid as i32, sig) };
    r == 0 || std::io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH)
}
