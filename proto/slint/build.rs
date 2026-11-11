fn main() {
    // .slint をビルドのときに Rust のコードにする。決まり(ui/prelude.slint)と合わなければ、ここでビルドが止まる
    let config = slint_build::CompilerConfiguration::new().with_style("fluent".into());
    slint_build::compile_with_config("ui/sidebar.slint", config).expect("ui/sidebar.slint");
}
