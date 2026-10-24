fn main() {
    println!("cargo:rerun-if-changed=ui/app.slint");
    println!("cargo:rerun-if-changed=assets");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        winresource::WindowsResource::new()
            .set_icon("assets/app.ico")
            .set("ProductName", "Periph In One")
            .set("FileDescription", "Periph In One")
            .set("OriginalFilename", "periph-in-one.exe")
            .compile()
            .expect("Windows icon resource compilation failed");
        slint_build::compile_with_config(
            "ui/app.slint",
            slint_build::CompilerConfiguration::new().with_style("fluent-light".into()),
        )
        .expect("Slint UI compilation failed");
    }
}
