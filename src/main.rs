#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

#[cfg(windows)]
slint::include_modules!();

#[cfg(windows)]
mod app;
#[cfg(windows)]
mod platform;

#[cfg(windows)]
fn main() {
    if let Err(error) = app::run() {
        platform::error_dialog(&error);
    }
}

#[cfg(not(windows))]
fn main() {
    eprintln!("电量岛面向 Windows 10/11。核心测试可在本机运行：cargo test --lib --tests");
}
