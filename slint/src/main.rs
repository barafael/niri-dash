#[cfg(not(target_arch = "wasm32"))]
fn main() -> std::process::ExitCode {
    niri_dash::main()
}

#[cfg(target_arch = "wasm32")]
fn main() {
    niri_dash::web::start();
}
