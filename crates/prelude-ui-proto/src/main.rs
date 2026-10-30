// 項目 1: 日本語入力とフォント。fcitx5 で打って、変換中の表示と候補の窓の位置を見る
slint::slint! {
    import { LineEdit, TextEdit, VerticalBox } from "std-widgets.slint";

    export component Ime inherits Window {
        title: "PRELUDE Slint 試作 1: 日本語入力";
        preferred-width: 720px;
        preferred-height: 520px;
        in property <string> backend;

        VerticalBox {
            Text { text: "表示: 日本語と English の混ざった文。漢字・ひらがな・カタカナ・記号「」（）…"; wrap: word-wrap; }
            Text { text: "太字: 東京都千代田区 Rust と Slint で作る PRELUDE"; font-weight: 700; }
            Text { text: "大きな字: 情報理工学部"; font-size: 28px; }
            Text { text: "LineEdit(1 行):"; }
            LineEdit { placeholder-text: "ここで日本語を打つ(例: にほんごにゅうりょく)"; }
            Text { text: "TextEdit(複数行):"; }
            TextEdit { height: 140px; }
            Text { text: "描画: " + root.backend; color: #888; }
        }
    }
}

fn main() -> Result<(), slint::PlatformError> {
    let ui = Ime::new()?;
    let backend = std::env::var("SLINT_BACKEND").unwrap_or_else(|_| "既定".into());
    ui.set_backend(backend.into());
    ui.run()
}
