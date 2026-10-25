#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() -> eframe::Result {
    let mut args = std::env::args_os();
    let _program = args.next();
    if matches!(
        args.next().as_deref(),
        Some(value) if value == "--recovery-smoke-check"
    ) {
        if args.next().is_some() {
            eprintln!("dangomushi-desktop: --recovery-smoke-check accepts no additional arguments");
            std::process::exit(2);
        }
        println!(
            "dangomushi-desktop {} recovery-smoke-ok",
            env!("CARGO_PKG_VERSION")
        );
        return Ok(());
    }

    dangomushi::native_ui::run()
}
