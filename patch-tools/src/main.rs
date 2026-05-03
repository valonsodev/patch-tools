mod cli;
mod daemon;
mod diff;
mod engine_jni;
mod fingerprint;
mod install;
mod morphe_render;
mod output;
mod search;
mod syndiff;
mod types;

use crate::cli::Cli;
use clap::Parser;
use std::process::ExitCode;

#[tokio::main]
async fn main() -> ExitCode {
    match cli::app::run(Cli::parse()).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) if error.is::<output::PrintedDaemonError>() => ExitCode::FAILURE,
        Err(error) => {
            eprintln!("Error: {error:#}");
            ExitCode::FAILURE
        }
    }
}
