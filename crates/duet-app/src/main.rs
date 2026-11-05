// No console window for the GUI in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::process::ExitCode;

use duet_app::launch::Command;

/// Release `duet.exe` has no console, so the outcome is the exit code and the log.
fn finish(what: &str, result: std::io::Result<()>) -> ExitCode {
    match result {
        Ok(()) => {
            tracing::info!("{what}: done");
            ExitCode::SUCCESS
        }
        Err(err) => {
            tracing::error!(%err, "cannot {what}");
            ExitCode::FAILURE
        }
    }
}

fn main() -> std::process::ExitCode {
    duet_app::app::mark_process_start();
    let logs = duet_app::crash::log_dir();
    duet_app::crash::install_panic_hook(logs.clone());
    if let Err(err) = duet_app::crash::init_logging(&logs) {
        eprintln!("duet: cannot open the log file: {err}");
    }
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    tracing::info!(?args, "duet starting");
    // Before the arguments: an error in them is text.
    duet_app::i18n::startup(&duet_app::crash::data_dir());
    match duet_app::launch::command_from_args(&args, &duet_app::crash::data_dir()) {
        Command::Open(launch) => {
            // Explorer starts `--compare-to` in the clicked item's folder, and an open window
            // would keep that folder from being renamed or deleted. The paths are absolute by
            // now. Only this verb moves: relative paths typed on the Start tab need the
            // current folder.
            if args.first().is_some_and(|flag| flag == "--compare-to") {
                let _ = std::env::set_current_dir(duet_app::crash::data_dir());
            }
            duet_app::app::run(launch);
            ExitCode::SUCCESS
        }
        Command::SelectLeft(path) => {
            match duet_app::shell::store_left(&duet_app::crash::data_dir(), &path) {
                Ok(()) => {
                    tracing::info!(path = %path.display(), "selected as left");
                    ExitCode::SUCCESS
                }
                Err(err) => {
                    tracing::error!(path = %path.display(), %err, "cannot store the left path");
                    ExitCode::FAILURE
                }
            }
        }
        Command::RegisterShell => finish(
            "register the Explorer verbs",
            std::env::current_exe().and_then(|exe| {
                duet_app::shell_verbs::register(&exe, duet_app::shell_verbs::VERB_PREFIX)
            }),
        ),
        Command::UnregisterShell => finish(
            "remove the Explorer verbs",
            duet_app::shell_verbs::unregister(duet_app::shell_verbs::VERB_PREFIX),
        ),
    }
}
