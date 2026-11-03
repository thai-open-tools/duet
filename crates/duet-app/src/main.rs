// No console window for the GUI in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    duet_app::app::mark_process_start();
    let logs = duet_app::crash::log_dir();
    duet_app::crash::install_panic_hook(logs.clone());
    if let Err(err) = duet_app::crash::init_logging(&logs) {
        eprintln!("duet: cannot open the log file: {err}");
    }
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    tracing::info!(?args, "duet starting");
    duet_app::app::run(duet_app::launch::from_args(&args));
}
