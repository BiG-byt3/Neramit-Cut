#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod cg;
mod dlg;
mod engine;
mod export;
mod forms;
mod i18n;
mod mainwin;
mod model;
mod qa;
mod results;
mod settings;
mod setup;
mod update;
mod ui;
mod xmlio;

fn main() {
    std::panic::set_hook(Box::new(|info| {
        let msg = format!("{info}\n{}", std::backtrace::Backtrace::force_capture());
        let path = std::env::temp_dir().join("NeramitCut-panic.txt");
        let _ = std::fs::write(path, msg);
    }));
    let args: Vec<String> = std::env::args().skip(1).collect();
    // install support, run by NeramitCut-Setup and Settings > Apps (see setup.rs)
    match args.first().map(String::as_str) {
        Some("--register") => std::process::exit(setup::register(args.get(1).map(String::as_str).unwrap_or(""))),
        Some("--uninstall") => std::process::exit(setup::uninstall(args.iter().any(|a| a == "--quiet"))),
        Some("--uninstall-finish") => {
            let dir = std::path::PathBuf::from(args.get(1).cloned().unwrap_or_default());
            let pid = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(0);
            std::process::exit(setup::uninstall_finish(&dir, pid));
        }
        Some("--finish-portable-update") => std::process::exit(update::finish_portable_update(&args[1..])),
        Some("--check-update") => std::process::exit(update::cli_check(args.iter().any(|a| a == "--download"))),
        _ => {}
    }
    // screenshot mode keeps its settings next to the pictures
    if let Some(i) = args.iter().position(|a| a == "--shot") {
        if let Some(d) = args.get(i + 1) {
            std::env::set_var("NERAMIT_WORKDIR", std::path::Path::new(d).join("work"));
        }
    }
    let stack: usize = std::env::var("NERAMIT_STACK").ok().and_then(|s| s.parse().ok()).unwrap_or(16) << 20;
    let t = std::thread::Builder::new().stack_size(stack).spawn(move || unsafe { app::start(&args) }).expect("ui thread");
    let code = t.join().unwrap_or(1);
    std::process::exit(code);
}
