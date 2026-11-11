//! サイドバーの決まり(ui/prelude.slint)を破った .slint が、コンパイルで止まることを確かめる。
//! 組み込む UI は build.rs の slint-build が同じコンパイラで変換するので、ここで止まるものはビルドでも止まる
use slint_interpreter::{Compiler, DiagnosticLevel};
use std::path::{Path, PathBuf};

fn ui(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("ui").join(name)
}

/// コンパイルのエラーの一覧
fn errors(file: &str) -> Vec<String> {
    let mut compiler = Compiler::default();
    compiler.set_style("fluent".into());
    let r = spin_on::spin_on(compiler.build_from_path(ui(file)));
    r.diagnostics().filter(|d| d.level() == DiagnosticLevel::Error).map(|d| d.message().to_string()).collect()
}

#[test]
fn 既定のサイドバーは決まりを満たす() {
    assert_eq!(errors("sidebar.slint"), Vec::<String>::new());
}

#[test]
fn 決まりを破るとコンパイルで止まる() {
    let e = errors("broken.slint");
    eprintln!("{e:#?}");
    // 名前の間違い・型の間違い・決まりに無い頼みごとが、それぞれエラーになる
    assert!(e.iter().any(|m| m.contains("tab")), "{e:?}");
    assert!(e.iter().any(|m| m.contains("string") || m.contains("int")), "{e:?}");
    assert!(e.iter().any(|m| m.contains("delete-everything") || m.contains("delete_everything")), "{e:?}");
}

#[test]
fn サイドバーの部品の決まりはコンパイルが通り_全部使える() {
    // parts.slint は global だけなので、全部を使う parts-check.slint を通して確かめる
    assert_eq!(errors("parts-check.slint"), Vec::<String>::new());
}

#[test]
fn 似せた_global_はコンパイルは通る() {
    // 決まりの確かめは読み込むとき(--load の check_contract)にする。ここではコンパイルが通ることだけ
    assert_eq!(errors("fake.slint"), Vec::<String>::new());
}
