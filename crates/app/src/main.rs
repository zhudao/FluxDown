//! `fluxdown-desktop` 的薄入口；应用装配集中在本 crate。

mod app;
mod assets;

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
