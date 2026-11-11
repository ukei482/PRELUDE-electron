//! つなぎ無し(段 0)の裏側([`Compositor`])。docs/redesign-rust.md の「窓のつなぎ」の段 0。
//! 対応するコンポジタが無いときに使う。ペインのアプリを起動するだけで、窓の置き場所は環境に任せる。窓を探さず、並べず、読み戻さない。
//!
//! - アプリ・レシピのペインは、ペインごとに 1 回だけ起動する(環境変数 `PRELUDE_PANE` も渡す)。窓が閉じても知らない(止まったことにしない)
//! - Web のペインは、窓を探せないので、すぐに「見つけた」と知らせる。Chromium は本当の URL へ移り、窓は環境が置く
//! - 終わるとき、起動したアプリはそのまま残す(窓の持ち主は環境)
use crate::compositor::{Compositor, Level, Note, Scene};
use crate::launch;
use std::collections::HashSet;
use std::future::Future;
use std::pin::Pin;
use std::sync::Mutex;
use tokio::sync::mpsc;

pub struct Plain {
    notes: mpsc::UnboundedSender<Note>,
    /// 起動した・見つけたと知らせたペイン
    done: Mutex<HashSet<u64>>,
}

pub fn start(notes: mpsc::UnboundedSender<Note>) -> Plain {
    Plain { notes, done: Mutex::new(HashSet::new()) }
}

impl Compositor for Plain {
    fn name(&self) -> &'static str {
        "none"
    }
    fn level(&self) -> Level {
        Level::Launch
    }
    fn arrange(&self, scene: Scene) {
        let mut done = self.done.lock().unwrap();
        // 無くなったペインは忘れる(id は使い回さないので、同じ id で起動し直すことは無い)
        let live: HashSet<u64> = scene.panes.iter().map(|w| w.pane).collect();
        done.retain(|p| live.contains(p));
        for w in scene.panes {
            if done.contains(&w.pane) || w.window.is_some() {
                continue;
            }
            if !w.marker.is_empty() {
                let _ = self.notes.send(Note::Claimed { pane: w.pane });
                done.insert(w.pane);
                continue;
            }
            if w.cmd.trim().is_empty() {
                continue;
            }
            match launch::spawn(&w.cmd, &w.cwd, w.pane) {
                Ok(mut child) => {
                    // 終わったら片付ける(ゾンビを残さない)。止まったことはコアに知らせない
                    if let Ok(rt) = tokio::runtime::Handle::try_current() {
                        rt.spawn(async move {
                            let _ = child.wait().await;
                        });
                    }
                }
                Err(e) => eprintln!("prelude-core: cannot launch {} ({e})", w.cmd),
            }
            done.insert(w.pane);
        }
    }
    fn shutdown(&self) -> Pin<Box<dyn Future<Output = ()> + Send + '_>> {
        Box::pin(async {})
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compositor::Want;

    fn want(pane: u64, cmd: &str, marker: &str) -> Want {
        Want { pane, cmd: cmd.into(), cls: String::new(), cwd: "/".into(), rect: None, marker: marker.into(), window: None }
    }

    #[tokio::test]
    async fn 起動は一度だけ_web_はすぐ見つけたことにする() {
        let d = std::env::temp_dir().join(format!("prelude-plain-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let log = d.join("log");
        let (tx, mut rx) = mpsc::unbounded_channel();
        let p = start(tx);
        assert_eq!((p.name(), p.level()), ("none", Level::Launch));
        let cmd = format!("echo \"$PRELUDE_PANE\" >> '{}'", log.display());
        let scene = || Scene { panes: vec![want(3, &cmd, ""), want(4, "", "印:4"), want(5, " ", "")], tabs: vec![], active: None };
        p.arrange(scene());
        p.arrange(scene()); // 2 回目は起動しない
        assert_eq!(rx.try_recv().ok(), Some(Note::Claimed { pane: 4 }));
        assert!(rx.try_recv().is_err());
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
        assert_eq!(std::fs::read_to_string(&log).unwrap(), "3\n");
        // ペインが無くなれば忘れる
        p.arrange(Scene::default());
        assert!(p.done.lock().unwrap().is_empty());
        let _ = std::fs::remove_dir_all(&d);
    }
}
