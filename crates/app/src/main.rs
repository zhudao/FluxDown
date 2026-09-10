//! `fluxdown-desktop` 的薄入口；应用装配集中在本 crate。

mod account_port;
mod actions;
mod agent_client;
mod app;
mod assets;
mod capability_ports;
mod clipboard_watch;
mod downloads_port;
mod instance_ipc;
mod launch;
mod menus;
mod power;
mod service_bootstrap;
mod session;
mod settings_port;
mod tray;
mod windows;

use std::process::ExitCode;

fn main() -> ExitCode {
    match app::run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("failed to start FluxDown desktop client: {error:#}");
            ExitCode::FAILURE
        }
    }
}
