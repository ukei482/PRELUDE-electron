//! 使用者が置いた .slint を実行時に読み込む(`--load`)。読み込むときに、決まり(`global Prelude`)を満たすかを確かめる
use crate::{contract, core};
use slint::{ComponentHandle, ModelRc, VecModel};
use slint_interpreter::{Compiler, ComponentDefinition, ComponentInstance, Struct, Value, ValueType};
use std::path::Path;

fn type_name(t: ValueType) -> String {
    format!("{t:?}").to_lowercase()
}

/// 決まりを満たすかを確かめる。満たさなければ足りないものの一覧
pub fn check_contract(def: &ComponentDefinition) -> Result<(), Vec<String>> {
    let Some(props) = def.global_properties(contract::GLOBAL) else {
        return Err(vec![format!("global `{}` が無い(ui/prelude.slint を import していない)", contract::GLOBAL)]);
    };
    let props: Vec<(String, String)> = props.map(|(n, t)| (n, type_name(t))).collect();
    let callbacks: Vec<String> = def.global_callbacks(contract::GLOBAL).map(|c| c.collect()).unwrap_or_default();
    let m = contract::missing(&props, &callbacks);
    if m.is_empty() { Ok(()) } else { Err(m) }
}

/// 読み込んで、決まりを確かめてから動かす
pub fn compile(path: &str) -> Result<ComponentDefinition, Vec<String>> {
    let mut compiler = Compiler::default();
    // 決まり(prelude.slint)は PRELUDE が出すものを使う
    compiler.set_include_paths(vec![Path::new(env!("CARGO_MANIFEST_DIR")).join("ui")]);
    let r = spin_on::spin_on(compiler.build_from_path(path));
    let errors: Vec<String> =
        r.diagnostics().filter(|d| d.level() == slint_interpreter::DiagnosticLevel::Error).map(|d| d.to_string()).collect();
    if !errors.is_empty() {
        return Err(errors);
    }
    let def = r.components().last().ok_or_else(|| vec![format!("{path}: 書き出したコンポーネントが無い")])?;
    check_contract(&def)?;
    Ok(def)
}

fn set(ui: &ComponentInstance, name: &str, v: Value) {
    if let Err(e) = ui.set_global_property(contract::GLOBAL, name, v) {
        eprintln!("set {name}: {e}");
    }
}

fn apply(ui: &ComponentInstance, s: core::Snap) {
    let model = |v: Vec<Value>| Value::Model(ModelRc::new(VecModel::from(v)));
    set(ui, "online", Value::Bool(s.online));
    set(ui, "status", Value::String(s.status.into()));
    set(ui, "space", Value::String(s.space.into()));
    set(ui, "spaces", model(s.spaces.into_iter().map(|s| Value::String(s.into())).collect()));
    let bm = s.bookmarks.into_iter().map(|(p, t, f)| {
        Value::Struct(Struct::from_iter([
            ("path".into(), Value::String(p.into())),
            ("title".into(), Value::String(t.into())),
            ("folder".into(), Value::String(f.into())),
        ]))
    });
    set(ui, "bookmarks", model(bm.collect()));
    let tabs = s.tabs.into_iter().map(|t| {
        Value::Struct(Struct::from_iter([
            ("id".into(), Value::Number(t.id.into())),
            ("num".into(), Value::Number(t.num.into())),
            ("title".into(), Value::String(t.title.into())),
            ("kind".into(), Value::String(t.kind.into())),
            ("active".into(), Value::Bool(t.active)),
        ]))
    });
    set(ui, "tabs", model(tabs.collect()));
}

fn arg_str(a: &[Value]) -> String {
    match a.first() {
        Some(Value::String(s)) => s.to_string(),
        _ => String::new(),
    }
}

fn arg_int(a: &[Value]) -> i32 {
    match a.first() {
        Some(Value::Number(n)) => *n as i32,
        _ => 0,
    }
}

pub fn run(path: &str) -> Result<(), slint::PlatformError> {
    let def = match compile(path) {
        Ok(d) => d,
        Err(why) => {
            eprintln!("{path}: サイドバーの決まりを満たさないので読み込まない:");
            for w in why {
                eprintln!("  - {w}");
            }
            std::process::exit(1);
        }
    };
    // 決まりを確かめてから、ウィンドウの用意をする
    slint::set_xdg_app_id("prelude-slint-proto")?;
    let ui = def.create()?;
    let cb = |name: &str, f: Box<dyn Fn(&[Value])>| {
        let _ = ui.set_global_callback(contract::GLOBAL, name, move |a| {
            f(a);
            Value::Void
        });
    };
    cb("switch-space", Box::new(|a| core::switch_space(arg_str(a))));
    cb("open-bookmark", Box::new(|a| core::open_bookmark(arg_str(a))));
    cb("activate-tab", Box::new(|a| core::activate_tab(arg_int(a))));
    cb("close-tab", Box::new(|a| core::close_tab(arg_int(a))));
    cb("open-launcher", Box::new(|_| core::open_launcher()));
    let weak = ui.as_weak();
    cb(
        "toggle-compact",
        Box::new(move |_| {
            let Some(ui) = weak.upgrade() else { return };
            let now = matches!(ui.get_global_property(contract::GLOBAL, "compact"), Ok(Value::Bool(true)));
            set(&ui, "compact", Value::Bool(!now));
        }),
    );
    let weak = ui.as_weak();
    core::watch(move |s| {
        if let Some(ui) = weak.upgrade() {
            apply(&ui, s);
        }
    });
    ui.run()
}
