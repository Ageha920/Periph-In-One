//! Development helper: run one full device scan and print the readings.
//! Not part of the shipped binary; run with `cargo run --example scan --locked`.

#[cfg(windows)]
fn main() {
    match periph_in_one::devices::scan() {
        Ok(readings) => println!("{readings:#?}"),
        Err(error) => {
            eprintln!("scan failed: {error}");
            std::process::exit(1);
        }
    }
}

#[cfg(not(windows))]
fn main() {
    eprintln!("scan example is Windows-only");
    std::process::exit(1);
}
